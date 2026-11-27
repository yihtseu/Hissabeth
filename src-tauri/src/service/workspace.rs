// SPDX-License-Identifier: GPL-3.0-or-later
// One workspace lease spans loaded systems and recoverable startup failures.
use super::*;

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(super) struct ProjectList {
    active_file: Option<PathBuf>,
    workspace_file: PathBuf,
    projects_dir: PathBuf,
    saved_projects: Vec<PathBuf>,
    recent: Vec<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(super) enum Status {
    Ready {
        revision: u64,
        path: String,
    },
    Recovery {
        revision: u64,
        path: String,
        reason: String,
        source_text: Option<String>,
        source_bytes: Option<usize>,
    },
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(super) struct Recovered {
    revision: u64,
    preserved_path: Option<String>,
}

const PROJECT_META: &str = "workspace-meta.json";

#[derive(Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub(super) struct ProjectFiles {
    pub linked_path: Option<PathBuf>,
    recent_projects: Vec<String>,
}

pub(super) fn workspace_dir(workspace_file: &Path) -> &Path {
    workspace_file
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn create_project_dir(path: &Path, created: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let missing: Vec<_> = path
        .ancestors()
        .take_while(|dir| !dir.as_os_str().is_empty() && !dir.is_dir())
        .collect();
    for dir in missing.into_iter().rev() {
        match std::fs::create_dir(dir) {
            Ok(()) => created.push(dir.to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && dir.is_dir() => (),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

impl ProjectFiles {
    pub fn load(workspace_file: &Path) -> Self {
        std::fs::read(workspace_dir(workspace_file).join(PROJECT_META))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn is_zmx(path: &Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zmx"))
    }

    /// Where a project path points before any directory is created or link
    /// resolved: `~` expands, relative paths live in the workspace's projects
    /// directory, and saved files without an extension get one.
    pub fn locate(workspace_file: &Path, path: &str, save: bool) -> Result<PathBuf, String> {
        let path = Path::new(path);
        let mut located = if let Ok(relative) = path.strip_prefix("~") {
            std::env::home_dir()
                .ok_or("Home directory is unavailable")?
                .join(relative)
        } else if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace_dir(workspace_file).join("projects").join(path)
        };
        if save && located.extension().is_none() {
            located.set_extension("hissabeth.json");
        }
        Ok(located)
    }

    pub fn resolve(
        workspace_file: &Path,
        path: &str,
        save: bool,
        journal: Option<&mut FileJournal>,
    ) -> Result<PathBuf, String> {
        const SAVE_ZMX: &str =
            "Use project_export to write ZMX; project_save_as writes Hissabeth JSON";
        if path.trim().is_empty() {
            return Err("Project path is required".into());
        }
        if save && Self::is_zmx(Path::new(path)) {
            return Err(SAVE_ZMX.into());
        }
        let resolved = Self::locate(workspace_file, path, save)?;
        let parent = resolved
            .parent()
            .ok_or("Project file has no parent directory")?;
        let name = resolved.file_name().ok_or("Project file has no name")?;
        if save {
            if let Some(log) = journal {
                create_project_dir(parent, &mut log.directories).map_err(|e| e.to_string())?;
            } else {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let workspace_dir = workspace_dir(workspace_file)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let resolved = parent.canonicalize().map_err(|e| e.to_string())?.join(name);
        let target = match resolved.canonicalize() {
            Ok(path) if path.is_file() => path,
            Ok(_) => return Err("Project path must be a file".into()),
            Err(e) if save && e.kind() == std::io::ErrorKind::NotFound => resolved,
            Err(e) => return Err(e.to_string()),
        };
        if save && Self::is_zmx(&target) {
            return Err(SAVE_ZMX.into());
        }
        let workspace_file = workspace_dir.join(workspace_file.file_name().unwrap());
        if [
            workspace_dir.join(PROJECT_META),
            workspace_file.with_extension("lock"),
            workspace_file.with_extension("pending"),
        ]
        .contains(&target)
        {
            return Err("Project path is reserved for workspace storage".into());
        }
        Ok(target)
    }

    pub fn select(&mut self, path: &Path, linked: bool) {
        self.linked_path = linked.then(|| path.to_path_buf());
        let path = path.display().to_string();
        self.recent_projects.retain(|recent| recent != &path);
        self.recent_projects.insert(0, path);
    }

    pub fn list(&self, workspace_file: &Path) -> ProjectList {
        let projects_dir = workspace_dir(workspace_file).join("projects");
        let mut saved_projects: Vec<_> = std::fs::read_dir(&projects_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                    && path.is_file()
            })
            .collect();
        saved_projects.sort();
        ProjectList {
            active_file: self.linked_path.clone(),
            workspace_file: workspace_file.to_owned(),
            projects_dir,
            saved_projects,
            recent: self.recent_projects.clone(),
        }
    }
}

fn stage_file(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile, String> {
    let mut file =
        tempfile::NamedTempFile::new_in(workspace_dir(path)).map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    Ok(file)
}

type Backup = Option<tempfile::NamedTempFile>;

fn backup_file(path: &Path) -> Result<Backup, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(stage_file(path, &bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn restore_file(path: &Path, previous: Backup) -> std::io::Result<()> {
    match previous {
        Some(file) => file.persist(path).map(|_| ()).map_err(|e| e.error),
        None => std::fs::remove_file(path),
    }
}

#[derive(Default)]
pub(super) struct FileJournal {
    originals: std::collections::BTreeMap<PathBuf, Backup>,
    directories: Vec<PathBuf>,
}

impl FileJournal {
    pub fn keep_directories(&mut self) {
        self.directories.clear();
    }

    pub fn restore(self) -> Result<(), String> {
        let mut errors = Vec::new();
        for (path, original) in self.originals {
            if let Err(e) = restore_file(&path, original) {
                errors.push(format!("could not restore {}: {e}", path.display()));
            }
        }
        for path in self.directories.into_iter().rev() {
            if let Err(e) = std::fs::remove_dir(&path) {
                errors.push(format!("could not remove {}: {e}", path.display()));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

struct ProjectWrite {
    path: PathBuf,
    next: tempfile::NamedTempFile,
    previous: Option<tempfile::NamedTempFile>,
}

impl ProjectWrite {
    fn stage(path: PathBuf, bytes: &[u8]) -> Result<Self, String> {
        let previous = backup_file(&path)?;
        let next = stage_file(&path, bytes)?;
        Ok(Self {
            path,
            next,
            previous,
        })
    }
}

pub(super) fn persist_system(
    path: &Path,
    system: &System,
    projects: &ProjectFiles,
    save_metadata: bool,
    journal: Option<&mut FileJournal>,
) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(system).map_err(|e| e.to_string())?;
    let mut writes = Vec::new();
    if let Some(linked) = &projects.linked_path {
        let workspace_file = path
            .canonicalize()
            .or_else(|_| {
                workspace_dir(path)
                    .canonicalize()
                    .map(|dir| dir.join(path.file_name().unwrap()))
            })
            .map_err(|e| e.to_string())?;
        if linked != &workspace_file {
            writes.push(ProjectWrite::stage(linked.clone(), &bytes)?);
        }
    }
    if save_metadata {
        let metadata = serde_json::to_vec_pretty(projects).map_err(|e| e.to_string())?;
        writes.push(ProjectWrite::stage(
            workspace_dir(path).join(PROJECT_META),
            &metadata,
        )?);
    }

    let original = journal
        .as_ref()
        .filter(|log| !log.originals.contains_key(path))
        .map(|_| backup_file(path))
        .transpose()?;
    let pending_path = path.with_extension("pending");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&pending_path)
        .map_err(|e| e.to_string())?;
    let pending = Pending(pending_path);
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);

    let mut installed = Vec::new();
    let result = (|| {
        for ProjectWrite {
            path,
            next,
            previous,
        } in writes
        {
            next.persist(&path)
                .map_err(|e| format!("{}: {}", path.display(), e.error))?;
            installed.push((path, previous));
        }
        std::fs::rename(&pending.0, path).map_err(|e| e.to_string())
    })();
    if let Err(mut error) = result {
        for (path, previous) in installed.into_iter().rev() {
            if let Err(e) = restore_file(&path, previous) {
                error.push_str(&format!("; could not restore {}: {e}", path.display()));
            }
        }
        return Err(error);
    }
    if let Some(journal) = journal {
        if let Some(original) = original {
            journal.originals.insert(path.to_owned(), original);
        }
        for (path, previous) in installed {
            journal.originals.entry(path).or_insert(previous);
        }
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
enum Observed {
    Bytes(Vec<u8>),
    Absent,
    Unreadable(String),
}
impl Observed {
    fn read(path: &std::path::Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => Self::Bytes(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::Absent,
            Err(e) => Self::Unreadable(e.to_string()),
        }
    }
}
struct Recovery {
    revision: u64,
    reason: String,
    observed: Observed,
}
enum State {
    Ready(Box<Engine>),
    Recovery(Recovery),
}
pub struct Workspace {
    path: PathBuf,
    lease: Arc<std::fs::File>,
    state: State,
    /// Directories the GUI opened or saved a project in this session; agents
    /// may reach them too. Never persisted.
    session: Vec<PathBuf>,
}
#[derive(schemars::JsonSchema, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Source {
    Default {},
    Project {
        format: params::ProjectFormat,
        text: String,
    },
}
struct Pending(PathBuf);
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
impl Workspace {
    pub(super) fn file(&self) -> &Path {
        &self.path
    }
    pub(super) fn session(&self) -> &[PathBuf] {
        &self.session
    }
    pub(super) fn grant(&mut self, dir: PathBuf) {
        if !self.session.contains(&dir) {
            self.session.push(dir);
        }
    }
    pub(super) fn revision(&self) -> u64 {
        match &self.state {
            State::Ready(engine) => engine.revision,
            State::Recovery(recovery) => recovery.revision,
        }
    }

    pub(super) fn engine(&self) -> Result<&Engine, String> {
        match &self.state {
            State::Ready(engine) => Ok(engine),
            State::Recovery(_) => Err(Self::recovery_required()),
        }
    }

    pub(super) fn engine_mut(&mut self) -> Result<&mut Engine, String> {
        match &mut self.state {
            State::Ready(engine) => Ok(engine),
            State::Recovery(_) => Err(Self::recovery_required()),
        }
    }

    fn recovery_required() -> String {
        "Workspace recovery is required. Read workspace_status; optical tools are unavailable until a valid system is loaded.".into()
    }

    pub fn open(path: PathBuf) -> Result<Self, String> {
        let lease = acquire_lease(&path)?;
        let revision = fresh_revision()?;
        let observed = Observed::read(&path);
        let state = match &observed {
            Observed::Absent if Self::recovery_artifact(&path).is_some() => {
                State::Recovery(Recovery {
                    revision,
                    reason: Self::missing_reason(&path),
                    observed,
                })
            }
            Observed::Unreadable(reason) => State::Recovery(Recovery {
                revision,
                reason: reason.clone(),
                observed,
            }),
            _ => {
                let bytes = match &observed {
                    Observed::Bytes(b) => Some(b.as_slice()),
                    _ => None,
                };
                match Engine::load_bytes(path.clone(), lease.clone(), revision, bytes) {
                    Ok(engine) => State::Ready(Box::new(engine)),
                    Err(reason) => State::Recovery(Recovery {
                        revision,
                        reason,
                        observed,
                    }),
                }
            }
        };
        // Relative project paths save here, and agents may only create files
        // in existing directories.
        let _ = std::fs::create_dir_all(workspace_dir(&path).join("projects"));
        Ok(Self {
            path,
            lease,
            state,
            session: Vec::new(),
        })
    }
    fn recovery_artifact(path: &std::path::Path) -> Option<PathBuf> {
        let stem = path.file_stem()?.to_string_lossy();
        std::fs::read_dir(path.parent()?)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|p| {
                p.file_name().is_some_and(|v| {
                    let n = v.to_string_lossy();
                    n.ends_with(".json")
                        && (n.starts_with(&format!("{stem}.recovery-"))
                            || n.starts_with(&format!("{stem}.rejected-")))
                })
            })
    }
    fn missing_reason(path: &std::path::Path) -> String {
        match Self::recovery_artifact(path) {
            Some(p) => format!(
                "Saved file is missing. A recovery file remains at {}. Restore project text or create a new system.",
                p.display()
            ),
            None => "Saved file is missing. Restore project text or create a new system.".into(),
        }
    }
    pub fn status(&self) -> Value {
        json!(self.status_report())
    }

    pub(super) fn status_report(&self) -> Status {
        match &self.state {
            State::Ready(e) => Status::Ready {
                revision: e.revision,
                path: self.path.display().to_string(),
            },
            State::Recovery(r) => {
                let (text, bytes) = match &r.observed {
                    Observed::Bytes(b) => (std::str::from_utf8(b).ok(), Some(b.len())),
                    Observed::Absent => (None, Some(0)),
                    Observed::Unreadable(_) => (None, None),
                };
                Status::Recovery {
                    revision: r.revision,
                    path: self.path.display().to_string(),
                    reason: r.reason.clone(),
                    source_text: text.map(str::to_owned),
                    source_bytes: bytes,
                }
            }
        }
    }
    fn recovery(&self, revision: u64) -> Result<&Recovery, String> {
        let State::Recovery(r) = &self.state else {
            return Err("Workspace is ready; recovery is not active".into());
        };
        if revision != r.revision {
            return Err("Workspace changed; read workspace_status before recovering".into());
        }
        Ok(r)
    }
    pub(super) fn retry(&mut self, revision: u64) -> Result<Status, String> {
        self.recovery(revision)?;
        let next = revision
            .checked_add(1)
            .ok_or("Workspace revision exhausted")?;
        let observed = Observed::read(&self.path);
        let loaded = match &observed {
            Observed::Bytes(bytes) => {
                Engine::load_bytes(self.path.clone(), self.lease.clone(), next, Some(bytes))
            }
            Observed::Absent => Err(Self::missing_reason(&self.path)),
            Observed::Unreadable(reason) => Err(reason.clone()),
        };
        self.state = match loaded {
            Ok(engine) => State::Ready(Box::new(engine)),
            Err(reason) => State::Recovery(Recovery {
                revision: next,
                reason,
                observed,
            }),
        };
        Ok(self.status_report())
    }
    fn unique_file(&self, suffix: &str) -> Result<(PathBuf, std::fs::File), String> {
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
        let name = self
            .path
            .file_stem()
            .ok_or("Workspace file has no name")?
            .to_string_lossy();
        let token = nonce.iter().map(|v| format!("{v:02x}")).collect::<String>();
        let path = self
            .path
            .with_file_name(format!("{name}.{suffix}-{token}.json"));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        Ok((path, file))
    }
    pub(super) fn recover(&mut self, revision: u64, source: Source) -> Result<Recovered, String> {
        let observed = self.recovery(revision)?.observed.clone();
        if let Observed::Unreadable(reason) = &observed {
            return Err(format!(
                "Cannot preserve the unreadable saved file: {reason}. Retry from disk after correcting access."
            ));
        }
        let mut system: System = match source {
            Source::Default {} => System::default(),
            Source::Project { format, text } => match format {
                params::ProjectFormat::Hissabeth => serde_json::from_str(&text)
                    .map_err(|e| format!("Invalid optical project: {e}"))?,
                params::ProjectFormat::Geopter => project::import(&text)?,
                params::ProjectFormat::Zmx => zmx::import(&text)?,
            },
        };
        system.update()?;
        system.validate()?;
        let bytes = serde_json::to_vec_pretty(&system).map_err(|e| e.to_string())?;
        let next = revision
            .checked_add(1)
            .ok_or("Workspace revision exhausted")?;
        if Observed::read(&self.path) != observed {
            return Err(
                "Saved file changed on disk. Use Retry from disk before recovering.".into(),
            );
        }
        let (pending_path, mut pending_file) = self.unique_file("recovery")?;
        let pending = Pending(pending_path);
        pending_file
            .write_all(&bytes)
            .and_then(|_| pending_file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(pending_file);
        let preserved = match &observed {
            Observed::Bytes(original) => {
                let (path, placeholder) = self.unique_file("rejected")?;
                drop(placeholder);
                if let Err(e) = std::fs::rename(&self.path, &path) {
                    let _ = std::fs::remove_file(&path);
                    return Err(format!("Could not preserve the saved file: {e}"));
                }
                // The moved file is inspected after the rename; later path creation cannot be overwritten.
                let check = std::fs::read(&path).and_then(|b| {
                    if &b == original {
                        std::fs::File::open(&path)?.sync_all()
                    } else {
                        Err(std::io::Error::other("Saved file changed during recovery"))
                    }
                });
                if let Err(e) = check {
                    let _ = std::fs::hard_link(&path, &self.path);
                    return Err(format!(
                        "{e}. Original file preserved at {}. Use Retry from disk.",
                        path.display()
                    ));
                }
                Some(path)
            }
            Observed::Absent => None,
            Observed::Unreadable(_) => unreachable!(),
        };
        if let Err(e) = std::fs::hard_link(&pending.0, &self.path) {
            if let Some(path) = &preserved {
                let _ = std::fs::hard_link(path, &self.path);
            }
            return Err(format!(
                "Recovery could not install the system: {e}. Original file preserved at {}. Use Retry from disk.",
                preserved
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "no source file was present".into())
            ));
        }
        self.state = State::Ready(Box::new(Engine::new(
            self.path.clone(),
            self.lease.clone(),
            next,
            system,
            ProjectFiles::load(&self.path),
        )));
        Ok(Recovered {
            revision: next,
            preserved_path: preserved.map(|p| p.display().to_string()),
        })
    }
    pub fn call(&mut self, name: &str, args: Value) -> Result<Value, String> {
        registry::call(registry::Target::Workspace(self), name, args)
    }

    /// Like `call`, but holds file paths to an agent scope first.
    pub fn call_scoped(&mut self, scope: &super::Scope, name: &str, args: Value) -> Result<Value, String> {
        registry::validate_args(name, &args)?;
        scope.check(&self.path, &self.session, name, &args)?;
        self.call(name, args)
    }
}
impl ToolCaller for Workspace {
    fn call_tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        self.call(name, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn path(label: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "workspace-{label}-{}-{}.json",
                std::process::id(),
                fresh_revision().unwrap()
            ))
    }
    fn bad(label: &str, bytes: &[u8]) -> Workspace {
        let p = path(label);
        std::fs::write(&p, bytes).unwrap();
        Workspace::open(p).unwrap()
    }
    fn revision(w: &Workspace) -> u64 {
        w.status()["revision"].as_u64().unwrap()
    }
    #[test]
    fn invalid_startup_retains_bytes_and_lease_without_an_optical_system() {
        let bytes = b"{ broken project\n";
        let mut w = bad("invalid", bytes);
        assert_eq!(w.status()["state"], "recovery");
        assert_eq!(
            w.status()["source_text"],
            std::str::from_utf8(bytes).unwrap()
        );
        assert_eq!(w.status()["source_bytes"], bytes.len());
        assert!(Engine::open(w.path.clone()).is_err());
        for (name, args) in [
            ("system_get", json!({})),
            ("layout", json!({})),
            ("capabilities", json!({})),
            (
                "system_set",
                json!({"system":System::default(),"revision":revision(&w)}),
            ),
            ("system_validate", json!({"system":System::default()})),
        ] {
            assert!(w.call(name, args).unwrap_err().contains("recovery"));
        }
        let r = rpc(
            json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
            &mut w,
        )
        .unwrap();
        assert!(
            r["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("workspace_status")
        );
        assert!(
            rpc(json!({"id":2,"method":"tools/list"}), &mut w).unwrap()["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == "workspace_recover")
        );
        assert_eq!(std::fs::read(&w.path).unwrap(), bytes);
        let rev = revision(&w);
        assert!(
            w.call("workspace_retry", json!({"revision":rev+1}))
                .is_err()
        );
        assert_eq!(revision(&w), rev);
    }
    #[test]
    fn invalid_candidate_and_changed_disk_reject_without_preservation_or_replacement() {
        let mut w = bad("validation", b"initial invalid");
        let rev = revision(&w);
        for source in [
            json!({"kind":"project","format":"hissabeth","text":"{}"}),
            json!({"kind":"default","extra":true}),
            json!({"kind":"project","format":"bad","text":"{}"}),
        ] {
            assert!(
                w.call("workspace_recover", json!({"revision":rev,"source":source}))
                    .is_err()
            );
        }
        assert_eq!(std::fs::read(&w.path).unwrap(), b"initial invalid");
        assert!(Workspace::recovery_artifact(&w.path).is_none());
        std::fs::write(&w.path, b"new external edit").unwrap();
        assert!(
            w.call(
                "workspace_recover",
                json!({"revision":rev,"source":{"kind":"default"}})
            )
            .unwrap_err()
            .contains("changed on disk")
        );
        assert_eq!(std::fs::read(&w.path).unwrap(), b"new external edit");
        let status = w.call("workspace_retry", json!({"revision":rev})).unwrap();
        assert_eq!(status["state"], "recovery");
        assert_eq!(status["revision"], rev + 1);
        assert_eq!(status["source_text"], "new external edit");
        let mut fixed = System {
            name: "Externally repaired".into(),
            ..Default::default()
        };
        fixed.update().unwrap();
        std::fs::write(&w.path, serde_json::to_vec(&fixed).unwrap()).unwrap();
        let status = w
            .call("workspace_retry", json!({"revision":rev+1}))
            .unwrap();
        assert_eq!(status["state"], "ready");
        assert_eq!(status["revision"], rev + 2);
        assert_eq!(
            w.call("system_get", json!({})).unwrap()["system"],
            serde_json::to_value(&fixed).unwrap()
        );
        assert!(Engine::open(w.path.clone()).is_err());
        assert!(
            w.call(
                "workspace_recover",
                json!({"revision":rev+2,"source":{"kind":"default"}})
            )
            .is_err()
        );
    }
    #[test]
    fn explicit_recovery_preserves_exact_binary_source_and_imports_both_project_formats() {
        for format in ["default", "hissabeth", "geopter"] {
            let original = [0xff, 0xfe, 0x00, 0x7b, 0x01];
            let mut w = bad(format, &original);
            let rev = revision(&w);
            assert!(w.status()["source_text"].is_null());
            let mut candidate = System {
                name: "Chosen optical project".into(),
                ..Default::default()
            };
            if format == "geopter" {
                candidate.pupil_mapping = optics::PupilMapping::ReferenceEntrancePupil;
                candidate.object_distance = Some(1e10);
                candidate.temperature = 25.;
            }
            candidate.update().unwrap();
            let source = match format {
                "default" => json!({"kind":"default"}),
                "hissabeth" => {
                    json!({"kind":"project","format":format,"text":serde_json::to_string(&candidate).unwrap()})
                }
                _ => {
                    json!({"kind":"project","format":format,"text":project::export(&candidate).unwrap().to_string()})
                }
            };
            let result = w
                .call("workspace_recover", json!({"revision":rev,"source":source}))
                .unwrap();
            assert_eq!(result["revision"], rev + 1);
            assert_eq!(
                std::fs::read(result["preserved_path"].as_str().unwrap()).unwrap(),
                original
            );
            assert_eq!(w.status()["state"], "ready");
            let system = w.call("system_get", json!({})).unwrap();
            assert_eq!(system["revision"], rev + 1);
            assert_eq!(
                system["system"]["name"],
                if format == "default" {
                    "BK7 singlet"
                } else {
                    "Chosen optical project"
                }
            );
            assert!(w.call("paraxial", json!({"wavelength":587.5618})).is_ok());
            let path = w.path.clone();
            drop(w);
            let restored = Workspace::open(path).unwrap();
            assert_eq!(restored.status()["state"], "ready");
        }
    }
    #[test]
    fn missing_unreadable_and_interrupted_sources_never_create_a_system_implicitly() {
        let p = path("fresh");
        let w = Workspace::open(p.clone()).unwrap();
        assert_eq!(w.status()["state"], "ready");
        assert!(!p.exists());
        drop(w);
        let mut w = bad("missing", b"bad source");
        let rev = revision(&w);
        std::fs::remove_file(&w.path).unwrap();
        let status = w.call("workspace_retry", json!({"revision":rev})).unwrap();
        assert_eq!(status["state"], "recovery");
        assert_eq!(status["source_bytes"], 0);
        assert!(!w.path.exists());
        let recovered = w
            .call(
                "workspace_recover",
                json!({"revision":rev+1,"source":{"kind":"default"}}),
            )
            .unwrap();
        assert!(recovered["preserved_path"].is_null());
        let p = path("directory");
        std::fs::create_dir(&p).unwrap();
        let mut w = Workspace::open(p.clone()).unwrap();
        assert!(w.status()["source_bytes"].is_null());
        assert!(
            w.call(
                "workspace_recover",
                json!({"revision":revision(&w),"source":{"kind":"default"}})
            )
            .is_err()
        );
        assert!(p.is_dir());
        drop(w);
        std::fs::remove_dir(p).unwrap();
        let p = path("interrupted");
        let archive = p.with_file_name(format!(
            "{}.rejected-interrupted.json",
            p.file_stem().unwrap().to_string_lossy()
        ));
        std::fs::write(&archive, b"preserved original").unwrap();
        let w = Workspace::open(p.clone()).unwrap();
        assert_eq!(w.status()["state"], "recovery");
        assert!(!p.exists());
        assert!(
            w.status()["reason"]
                .as_str()
                .unwrap()
                .contains("recovery file remains")
        );
        assert_eq!(std::fs::read(archive).unwrap(), b"preserved original");
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    #[test]
    fn derived_write_failure_keeps_readable_source_in_recovery_until_retry_succeeds() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "workspace-write-failure-{}-{}.json",
                std::process::id(),
                fresh_revision().unwrap()
            ));
        let bytes = serde_json::to_vec(&System::default()).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let pending = path.with_extension("pending");
        std::fs::create_dir(&pending).unwrap();
        let mut w = Workspace::open(path.clone()).unwrap();
        let status = w.status();
        assert_eq!(status["state"], "recovery");
        assert_eq!(status["source_text"], std::str::from_utf8(&bytes).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_dir(pending).unwrap();
        let status = w
            .call("workspace_retry", json!({"revision":status["revision"]}))
            .unwrap();
        assert_eq!(status["state"], "ready");
        assert_ne!(std::fs::read(&path).unwrap(), bytes);
        assert!(w.call("system_get", json!({})).is_ok());
    }
}
