// SPDX-License-Identifier: GPL-3.0-or-later
//! File-access scope for agent (broker) callers.
//!
//! The desktop GUI is unrestricted: a path it sends was chosen by the user in
//! a file dialog. Every broker call — MCP relays and the in-app agent chat,
//! which reaches the engine through the same relay — may only touch files in
//! the app data directory, in directories the GUI opened or saved a project in
//! during this session, or under an `--allow-path DIR`. `--allow-any-path`
//! lifts the scope. Both flags are accepted by the `--mcp` relay (forwarded
//! with each authenticated request, so an agent's MCP configuration sets them)
//! and by the process serving the broker (`--engine`, or the desktop app);
//! a request is checked against the union of the two. The scope is session
//! state and is never written to disk.
use super::{
    SharedWorkspace, registry,
    workspace::{ProjectFiles, workspace_dir},
};
use registry::FileAccess;
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};

/// Who issued a command.
pub enum Caller {
    /// The desktop user interface.
    Gui,
    /// An agent reaching the engine through the loopback broker.
    Broker(Scope),
}

/// Directories an agent may reach beyond the app data directory and the
/// session's opened directories.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scope {
    allow: Vec<Allowed>,
    any: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Allowed {
    /// The directory as written on the command line.
    given: String,
    /// Its canonical path.
    root: PathBuf,
}

impl Scope {
    /// Parses `--allow-path DIR` (repeatable, also `--allow-path=DIR`) and
    /// `--allow-any-path` from process arguments.
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let mut scope = Self::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let given = if arg == "--allow-path" {
                args.next().ok_or("--allow-path requires a directory")?.clone()
            } else if let Some(dir) = arg.strip_prefix("--allow-path=") {
                dir.to_owned()
            } else {
                scope.any |= arg == "--allow-any-path";
                continue;
            };
            let root = expand(&given)
                .canonicalize()
                .map_err(|e| format!("--allow-path {given}: {e}"))?;
            if !root.is_dir() {
                return Err(format!("--allow-path {given}: not a directory"));
            }
            scope.allow.push(Allowed { given, root });
        }
        Ok(scope)
    }

    /// The scope as forwarded by the relay inside its authenticated envelope.
    pub fn envelope(&self) -> Value {
        json!({
            "allow": self.allow.iter().map(|a| json!({"given": a.given, "root": a.root})).collect::<Vec<_>>(),
            "any": self.any,
        })
    }

    /// Reads a relay envelope; every root is canonicalized again here.
    pub fn from_envelope(value: &Value) -> Result<Self, String> {
        let mut scope = Self { allow: Vec::new(), any: value["any"] == true };
        for allowed in value["allow"].as_array().into_iter().flatten() {
            let (Some(given), Some(root)) = (allowed["given"].as_str(), allowed["root"].as_str()) else {
                return Err("Invalid relay scope".into());
            };
            let root = Path::new(root).canonicalize().map_err(|e| format!("--allow-path {given}: {e}"))?;
            scope.allow.push(Allowed { given: given.to_owned(), root });
        }
        Ok(scope)
    }

    /// The union of two scopes.
    pub fn merge(mut self, other: &Scope) -> Self {
        self.any |= other.any;
        for allowed in &other.allow {
            if !self.allow.contains(allowed) {
                self.allow.push(allowed.clone());
            }
        }
        self
    }

    /// Checks every path a command (or each step of a batch) names.
    pub(super) fn check(&self, workspace_file: &Path, session: &[PathBuf], name: &str, args: &Value) -> Result<(), String> {
        if self.any {
            return Ok(());
        }
        let spec = registry::lookup(name)?;
        match spec.files {
            FileAccess::None => Ok(()),
            FileAccess::Steps => args["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .try_for_each(|step| self.check(workspace_file, session, step["command"].as_str().unwrap_or(""), &step["args"])),
            FileAccess::Read | FileAccess::Write => match args["path"].as_str() {
                // An empty path is the command's own error.
                Some(path) if !path.trim().is_empty() => self.check_path(workspace_file, session, path, spec.files == FileAccess::Write),
                Some(_) => Ok(()),
                // Commands without a path still touch fixed locations; a link
                // there must not lead out of the scope.
                None => match implicit(name, workspace_file) {
                    Some(path) => self.check_path(workspace_file, session, &path.to_string_lossy(), false),
                    None => Ok(()),
                },
            },
        }
    }

    fn check_path(&self, workspace_file: &Path, session: &[PathBuf], path: &str, save: bool) -> Result<(), String> {
        let data = workspace_dir(workspace_file).canonicalize().map_err(|e| e.to_string())?;
        let located = ProjectFiles::locate(workspace_file, path, save)?;
        let (Some(parent), Some(name)) = (located.parent(), located.file_name()) else {
            return Err(format!("{path}: not a file path"));
        };
        // The parent as it really is: canonical when it exists; otherwise its
        // nearest existing ancestor, canonical, with the missing rest folded.
        let (resolved, missing) = match parent.canonicalize() {
            Ok(parent) => {
                let joined = parent.join(name);
                // An existing file may itself be a symbolic link out of scope.
                (joined.canonicalize().unwrap_or(joined), false)
            }
            Err(_) => {
                let existing = parent.ancestors().find(|a| a.exists()).unwrap_or(Path::new("/"));
                let base = existing.canonicalize().map_err(|e| format!("{path}: {}: {e}", existing.display()))?;
                let rest = parent.strip_prefix(existing).unwrap_or(parent);
                (lexical(&base.join(rest).join(name)), true)
            }
        };
        self.within(&data, session, path, &located, &resolved)?;
        if missing && save {
            return Err(format!(
                "Cannot write {path}: it resolves to {}, whose parent directory {} does not exist. Agents may only create files in existing directories; create the directory first, or start the MCP relay with --allow-any-path.",
                resolved.display(),
                resolved.parent().unwrap_or(&resolved).display()
            ));
        }
        Ok(())
    }

    /// Whether `resolved` lies in the scope; the refusal names the requested
    /// and resolved paths, the scope, any `--allow-path` the request seemed to
    /// fall under, and how to allow it.
    fn within(&self, data: &Path, session: &[PathBuf], path: &str, located: &Path, resolved: &Path) -> Result<(), String> {
        let session = session.iter().filter_map(|d| d.canonicalize().ok());
        if std::iter::once(data.to_path_buf()).chain(session).chain(self.allow.iter().map(|a| a.root.clone())).any(|root| resolved.starts_with(root)) {
            return Ok(());
        }
        let near = self
            .allow
            .iter()
            .find(|a| located.starts_with(&a.root) || located.starts_with(expand(&a.given)))
            .map(|a| format!(" It lies under --allow-path {} but resolves outside it through a symbolic link or '..'.", a.given))
            .unwrap_or_default();
        Err(format!(
            "File access denied: {path} resolves to {}, outside the agent file scope (the app data directory {}, directories opened or saved in the app this session{}).{near} Start the MCP relay with --allow-path <dir> to allow a directory, or --allow-any-path to lift the scope.",
            resolved.display(),
            data.display(),
            self.allow.iter().map(|a| format!(", --allow-path {}", a.given)).collect::<String>(),
        ))
    }
}

/// The fixed location a path-less file command reads or writes.
fn implicit(name: &str, workspace_file: &Path) -> Option<PathBuf> {
    let dir = workspace_dir(workspace_file);
    let dir = if dir.is_absolute() { dir.to_path_buf() } else { std::env::current_dir().ok()?.join(dir) };
    match name {
        "project_list" => Some(dir.join("projects")),
        "workspace_retry" | "workspace_recover" => Some(dir.join(workspace_file.file_name()?)),
        _ => None,
    }
}

/// Folds `.` and `..` components without touching the file system.
fn lexical(path: &Path) -> PathBuf {
    let mut folded = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                folded.pop();
            }
            other => folded.push(other),
        }
    }
    folded
}

/// Expands a leading `~` the way project paths do.
fn expand(path: &str) -> PathBuf {
    match (Path::new(path).strip_prefix("~"), std::env::home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// After a successful GUI call, grants the directory of the project file it
/// opened or saved to agents for the rest of the session. For a batch, only
/// the steps whose effects were kept count: all of them when it succeeded,
/// the successful prefix of a failed batch run with `rollback: false`.
pub(super) fn remember(workspace: &SharedWorkspace, name: &str, partial: bool, result: &Value) {
    let mut dirs = Vec::new();
    granted(name, partial, result, &mut dirs);
    if !dirs.is_empty()
        && let Ok(mut workspace) = workspace.write()
    {
        for dir in dirs {
            workspace.grant(dir);
        }
    }
}

fn granted(name: &str, partial: bool, result: &Value, dirs: &mut Vec<PathBuf>) {
    match registry::lookup(name).map(|s| s.files) {
        Ok(FileAccess::Read | FileAccess::Write) => {
            if let Some(dir) = result["path"].as_str().and_then(|p| Path::new(p).parent()) {
                dirs.push(dir.to_path_buf());
            }
        }
        Ok(FileAccess::Steps) if result["ok"] == Value::Bool(true) || partial => {
            for step in result["results"].as_array().into_iter().flatten() {
                if step["ok"] == Value::Bool(true)
                    && let Some(command) = step["command"].as_str()
                {
                    // A nested batch's own rollback flag is not echoed; only
                    // a fully successful nested batch is known to be kept.
                    granted(command, false, &step["result"], dirs);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Workspace, dispatch};
    use super::*;
    use std::sync::{Arc, RwLock};

    struct Fixture {
        _root: tempfile::TempDir,
        data: PathBuf,
        outside: PathBuf,
        workspace: SharedWorkspace,
    }

    /// A workspace in `root/data` and an unrelated directory `root/outside`
    /// holding a saved project.
    fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let (data, outside) = (root.path().join("data"), root.path().join("outside"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let workspace = Arc::new(RwLock::new(Workspace::open(data.join("system.json")).unwrap()));
        let fixture = Fixture { _root: root, data, outside, workspace };
        let text = dispatch(&Caller::Gui, &fixture.workspace, "project_export", json!({"format": "hissabeth"}), None).unwrap();
        std::fs::write(fixture.outside.join("lens.hissabeth.json"), text["text"].as_str().unwrap()).unwrap();
        fixture
    }

    impl Fixture {
        fn revision(&self) -> u64 {
            dispatch(&Caller::Gui, &self.workspace, "system_get", json!({}), None).unwrap()["revision"].as_u64().unwrap()
        }
        fn save(&self, caller: &Caller, path: &str) -> Result<String, String> {
            let args = json!({"path": path, "revision": self.revision()});
            dispatch(caller, &self.workspace, "project_save_as", args, None).map(|r| r["path"].as_str().unwrap().to_owned())
        }
        fn open(&self, caller: &Caller, path: &str) -> Result<Value, String> {
            let args = json!({"path": path, "revision": self.revision()});
            dispatch(caller, &self.workspace, "project_open", args, None)
        }
    }

    fn agent(args: &[&str]) -> Caller {
        Caller::Broker(Scope::from_args(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>()).unwrap())
    }

    #[test]
    fn agents_read_and_write_inside_the_data_directory_and_allowed_paths() {
        let f = fixture();
        let default = agent(&[]);
        // Relative project paths live in the data directory's projects folder.
        f.save(&default, "design").unwrap();
        std::fs::create_dir_all(f.data.join("projects/fresh")).unwrap();
        f.save(&default, "fresh/design").unwrap();
        f.open(&default, "fresh/design.hissabeth.json").unwrap();
        let allowed = agent(&["--allow-path", f.outside.to_str().unwrap()]);
        f.open(&allowed, f.outside.join("lens.hissabeth.json").to_str().unwrap()).unwrap();
        f.save(&allowed, f.outside.join("copy").to_str().unwrap()).unwrap();
        assert!(f.outside.join("copy.hissabeth.json").is_file());
    }

    #[test]
    fn agents_are_refused_outside_their_scope() {
        let f = fixture();
        let target = f.outside.join("lens.hissabeth.json");
        let error = f.open(&agent(&[]), target.to_str().unwrap()).unwrap_err();
        assert!(error.contains("File access denied") && error.contains("--allow-path <dir>"), "{error}");
        assert!(error.contains(&target.canonicalize().unwrap().display().to_string()), "{error}");
        // Missing files outside are refused, not reported as missing.
        let error = f.open(&agent(&[]), f.outside.join("absent/lens.hissabeth.json").to_str().unwrap()).unwrap_err();
        assert!(error.contains("File access denied"), "{error}");
        let error = f.save(&agent(&[]), f.outside.join("new").to_str().unwrap()).unwrap_err();
        assert!(error.contains("File access denied"), "{error}");
        assert!(!f.outside.join("new.hissabeth.json").exists());
        // Inside a batch, every step is checked before any runs.
        let steps = json!({"steps": [
            {"command": "project_save_as", "args": {"path": "kept"}},
            {"command": "project_open", "args": {"path": target}},
        ]});
        let error = dispatch(&agent(&[]), &f.workspace, "batch", steps, None).unwrap_err();
        assert!(error.contains("File access denied"), "{error}");
        assert!(!f.data.join("projects/kept.hissabeth.json").exists());
    }

    #[test]
    fn escapes_through_dot_dot_and_symbolic_links_are_refused() {
        let f = fixture();
        let allowed = f.data.join("allowed");
        std::fs::create_dir_all(&allowed).unwrap();
        let only = agent(&["--allow-path", allowed.to_str().unwrap()]);
        std::fs::create_dir_all(f.data.join("projects")).unwrap();
        // `..` leaves the data directory.
        let error = f.open(&only, "../../outside/lens.hissabeth.json").unwrap_err();
        assert!(error.contains("File access denied"), "{error}");
        #[cfg(unix)]
        {
            let link = f.data.join("projects");
            std::os::unix::fs::symlink(&f.outside, link.join("away")).unwrap();
            std::os::unix::fs::symlink(f.outside.join("lens.hissabeth.json"), allowed.join("lens.hissabeth.json")).unwrap();
            // A linked directory out of the data directory.
            let error = f.open(&only, "away/lens.hissabeth.json").unwrap_err();
            assert!(error.contains("File access denied"), "{error}");
            let error = f.save(&only, "away/written").unwrap_err();
            assert!(error.contains("File access denied"), "{error}");
            assert!(!f.outside.join("written.hissabeth.json").exists());
            // A linked file under an --allow-path names the rule it seemed to match.
            let error = f.open(&only, allowed.join("lens.hissabeth.json").to_str().unwrap()).unwrap_err();
            assert!(error.contains("lies under --allow-path") && error.contains(allowed.to_str().unwrap()), "{error}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn path_less_file_commands_do_not_follow_links_out_of_scope() {
        let f = fixture();
        dispatch(&agent(&[]), &f.workspace, "project_list", json!({}), None).unwrap();
        let projects = f.data.join("projects");
        std::fs::remove_dir_all(&projects).unwrap();
        std::os::unix::fs::symlink(&f.outside, &projects).unwrap();
        let error = dispatch(&agent(&[]), &f.workspace, "project_list", json!({}), None).unwrap_err();
        assert!(error.contains("File access denied") && error.contains(&f.outside.canonicalize().unwrap().display().to_string()), "{error}");
        let listed = dispatch(&Caller::Gui, &f.workspace, "project_list", json!({}), None).unwrap();
        assert!(listed.to_string().contains("lens.hissabeth.json"));
    }

    #[test]
    fn gui_batches_grant_the_directories_of_the_steps_they_keep() {
        let f = fixture();
        let dir = |name: &str| {
            let dir = f.outside.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        };
        let revision = |w: &SharedWorkspace| dispatch(&Caller::Gui, w, "system_get", json!({}), None).unwrap()["revision"].clone();
        let save = |path: PathBuf| json!({"command": "project_save_as", "args": {"path": path}});
        let fail = json!({"command": "project_open", "args": {"path": f.data.join("projects/absent.hissabeth.json")}});
        let batch = |steps: Vec<Value>, rollback: bool| {
            dispatch(&Caller::Gui, &f.workspace, "batch", json!({"steps": steps, "rollback": rollback}), None).unwrap()
        };
        let reachable = |dir: &Path| {
            let args = json!({"path": dir.join("design.hissabeth.json"), "revision": revision(&f.workspace)});
            dispatch(&agent(&[]), &f.workspace, "project_open", args, None).is_ok()
        };
        let (kept, rolled, prefix, after) = (dir("kept"), dir("rolled"), dir("prefix"), dir("after"));
        assert_eq!(batch(vec![save(kept.join("design"))], true)["ok"], true);
        assert!(reachable(&kept));
        // A failed batch that rolls back keeps nothing.
        assert_eq!(batch(vec![save(rolled.join("design")), fail.clone()], true)["ok"], false);
        std::fs::write(rolled.join("design.hissabeth.json"), std::fs::read(kept.join("design.hissabeth.json")).unwrap()).unwrap();
        assert!(!reachable(&rolled));
        // Without rollback, the steps before the failure are kept.
        let report = batch(vec![save(prefix.join("design")), fail, save(after.join("design"))], false);
        assert_eq!(report["ok"], false);
        assert!(reachable(&prefix));
        std::fs::write(after.join("design.hissabeth.json"), std::fs::read(kept.join("design.hissabeth.json")).unwrap()).unwrap();
        assert!(!reachable(&after));
    }

    #[test]
    fn new_files_outside_the_data_directory_need_an_existing_parent() {
        let f = fixture();
        let allowed = agent(&["--allow-path", f.outside.to_str().unwrap()]);
        let error = f.save(&allowed, f.outside.join("missing/deeper/new").to_str().unwrap()).unwrap_err();
        assert!(error.contains("does not exist") && error.contains("create the directory first"), "{error}");
        assert!(!f.outside.join("missing").exists());
        // The data directory is no exception.
        let error = f.save(&agent(&[]), "missing/new").unwrap_err();
        let target = f.data.canonicalize().unwrap().join("projects/missing/new.hissabeth.json");
        assert!(error.contains(&target.display().to_string()) && error.contains("--allow-any-path"), "{error}");
        assert!(!f.data.join("projects/missing").exists());
        // A missing subtree that climbs out with `..` is outside the scope.
        let error = f.save(&agent(&[]), "missing/../../../outside/new").unwrap_err();
        assert!(error.contains("File access denied"), "{error}");
        #[cfg(unix)]
        {
            // Below a link out of an --allow-path, the refusal shows the
            // linked target and the rule the path seemed to match.
            let allowed_dir = f.data.join("allowed");
            std::fs::create_dir_all(&allowed_dir).unwrap();
            std::os::unix::fs::symlink(&f.outside, allowed_dir.join("link")).unwrap();
            let only = agent(&["--allow-path", allowed_dir.to_str().unwrap()]);
            let error = f.save(&only, allowed_dir.join("link/missing/new").to_str().unwrap()).unwrap_err();
            let target = f.outside.canonicalize().unwrap().join("missing/new.hissabeth.json");
            assert!(error.contains(&target.display().to_string()) && error.contains("lies under --allow-path"), "{error}");
        }
    }

    #[test]
    fn allow_any_path_and_the_gui_are_unrestricted() {
        let f = fixture();
        let target = f.outside.join("lens.hissabeth.json");
        f.open(&agent(&["--allow-any-path"]), target.to_str().unwrap()).unwrap();
        f.save(&agent(&["--allow-any-path"]), f.outside.join("missing/new").to_str().unwrap()).unwrap();
        f.open(&Caller::Gui, target.to_str().unwrap()).unwrap();
    }

    #[test]
    fn a_directory_the_gui_opens_is_granted_to_agents_for_the_session() {
        let f = fixture();
        let target = f.outside.join("lens.hissabeth.json");
        assert!(f.open(&agent(&[]), target.to_str().unwrap()).is_err());
        f.open(&Caller::Gui, target.to_str().unwrap()).unwrap();
        f.open(&agent(&[]), target.to_str().unwrap()).unwrap();
        // A fresh session starts without it.
        let Fixture { _root, data, workspace, .. } = f;
        drop(workspace);
        let reopened = Arc::new(RwLock::new(Workspace::open(data.join("system.json")).unwrap()));
        let revision = dispatch(&Caller::Gui, &reopened, "system_get", json!({}), None).unwrap()["revision"].clone();
        let args = json!({"path": target, "revision": revision});
        let error = dispatch(&agent(&[]), &reopened, "project_open", args, None).unwrap_err();
        assert!(error.contains("File access denied"), "{error}");
    }

    #[test]
    fn relay_scopes_travel_in_the_envelope_and_merge_with_the_server() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap().to_owned();
        let relay = Scope::from_args(&["--mcp".into(), format!("--allow-path={path}")]).unwrap();
        let forwarded = Scope::from_envelope(&relay.envelope()).unwrap();
        assert_eq!(forwarded, relay);
        let server = Scope::from_args(&["--engine".into(), "--allow-any-path".into()]).unwrap();
        let merged = Scope::default().merge(&forwarded).merge(&server);
        assert!(merged.any && merged.allow.len() == 1);
        assert_eq!(Scope::from_envelope(&Value::Null).unwrap(), Scope::default());
        assert!(Scope::from_args(&["--allow-path".into()]).is_err());
        assert!(Scope::from_args(&["--allow-path".into(), dir.path().join("absent").to_str().unwrap().into()]).is_err());
    }
}
