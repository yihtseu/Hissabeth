use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn executable(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|p| {
            #[cfg(windows)]
            let names = vec![format!("{name}.exe"), format!("{name}.cmd")];
            #[cfg(not(windows))]
            let names = vec![name.to_string()];
            names.into_iter().map(move |n| p.join(n))
        })
        .find(|p| {
            if !p.is_file() {
                return false;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                p.metadata()
                    .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            {
                true
            }
        })
}
fn run(command: &mut Command) -> Result<std::process::Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) => {}
            Err(e) => return Err(e.to_string()),
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Agent CLI operation timed out".into());
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
pub fn detect() -> Value {
    json!(
        ["codex", "claude"]
            .iter()
            .map(|name| {
                let path = executable(name);
                json!({"name":name,"installed":path.is_some(),"executable":path})
            })
            .collect::<Vec<_>>()
    )
}
fn register(name: &str, path: &Path, app: &Path, workspace: &Path) -> Value {
    let mut get = Command::new(path);
    get.args(["mcp", "get", "hissabeth"]);
    let existing = run(&mut get);
    if let Ok(output) = existing {
        if output.status.success() {
            let description = String::from_utf8_lossy(&output.stdout);
            if description.contains(app.to_string_lossy().as_ref())
                && description.contains(workspace.to_string_lossy().as_ref())
                && description.contains("--mcp")
            {
                return json!({"name":name,"success":true,"message":"Already registered"});
            }
            if !description.contains("--mcp") {
                return json!({"name":name,"success":false,"message":"A different server already uses the name hissabeth"});
            }
            let mut remove = Command::new(path);
            remove.args(["mcp", "remove"]);
            if name == "claude" {
                remove.args(["--scope", "user"]);
            }
            remove.arg("hissabeth");
            match run(&mut remove) {
                Ok(o) if o.status.success() => {}
                Ok(o) => {
                    return json!({"name":name,"success":false,"message":String::from_utf8_lossy(&o.stderr)});
                }
                Err(e) => return json!({"name":name,"success":false,"message":e}),
            }
        }
    }
    let mut add = Command::new(path);
    add.args(["mcp", "add", "hissabeth"]);
    if name == "claude" {
        add.args(["--transport", "stdio", "--scope", "user"]);
    }
    add.arg("--env")
        .arg(format!("HISSABETH_DATA_DIR={}", workspace.display()))
        .arg("--")
        .arg(app)
        .arg("--mcp");
    match run(&mut add) {
        Ok(o) => {
            json!({"name":name,"success":o.status.success(),"message":String::from_utf8_lossy(if o.status.success(){&o.stdout}else{&o.stderr})})
        }
        Err(e) => json!({"name":name,"success":false,"message":e}),
    }
}
pub fn configure() -> Value {
    let app = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return json!([{"success":false,"message":e.to_string()}]),
    };
    let workspace = crate::service::data_dir();
    Value::Array(
        ["codex", "claude"]
            .iter()
            .map(|name| match executable(name) {
                Some(path) => register(name, &path, &app, &workspace),
                None => json!({"name":name,"installed":false,"status":"not installed"}),
            })
            .collect(),
    )
}
