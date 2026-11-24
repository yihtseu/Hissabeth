use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub(crate) fn executable(name: &str) -> Option<PathBuf> {
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
pub(crate) fn run(command: &mut Command) -> Result<std::process::Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.read_to_end(&mut bytes)
                .map(|_| bytes)
                .map_err(|e| e.to_string())
        })
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(std::process::Output {
                    status,
                    stdout: out.join().map_err(|_| "CLI stdout reader failed")??,
                    stderr: err.join().map_err(|_| "CLI stderr reader failed")??,
                });
            }
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
fn owned(config: &Value, app: &Path) -> bool {
    let transport = config.get("transport").unwrap_or(config);
    let command = transport
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("");
    let file = Path::new(command).file_stem().and_then(|s| s.to_str());
    (Path::new(command) == app || file == Some("hissabeth"))
        && transport.get("args") == Some(&json!(["--mcp"]))
}
fn exact(config: &Value, app: &Path, workspace: &Path) -> bool {
    let transport = config.get("transport").unwrap_or(config);
    transport.get("command").and_then(Value::as_str) == app.to_str()
        && transport.get("args") == Some(&json!(["--mcp"]))
        && transport
            .pointer("/env/HISSABETH_DATA_DIR")
            .and_then(Value::as_str)
            == workspace.to_str()
}
fn registrations(name: &str, path: &Path) -> Result<Vec<(String, Value)>, String> {
    if name == "codex" {
        let output = run(Command::new(path).args(["mcp", "list", "--json"]))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        let entries: Vec<Value> =
            serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
        return entries
            .into_iter()
            .map(|entry| {
                let key = entry
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("MCP entry has no name")?
                    .to_string();
                Ok((key, entry))
            })
            .collect();
    }
    let root = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
        })
        .ok_or("Cannot locate Claude configuration directory")?;
    let file = root.join(".claude.json");
    let content = match std::fs::read(&file) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let config: Value = serde_json::from_slice(&content).map_err(|e| e.to_string())?;
    match config.get("mcpServers") {
        None => Ok(Vec::new()),
        Some(Value::Object(entries)) => Ok(entries
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()),
        Some(_) => Err("Claude mcpServers must be an object".into()),
    }
}
fn register(name: &str, path: &Path, app: &Path, workspace: &Path) -> Value {
    let result = (|| -> Result<String, String> {
        let entries = registrations(name, path)?;
        if entries
            .iter()
            .any(|(key, value)| key == "hissabeth" && !owned(value, app))
        {
            return Err("A different server already uses the name hissabeth".into());
        }
        let current = entries
            .iter()
            .any(|(key, value)| key == "hissabeth" && exact(value, app, workspace));
        // Establish the canonical entry before removing duplicate aliases.
        if !current {
            if entries.iter().any(|(key, _)| key == "hissabeth") {
                remove(name, path, "hissabeth")?;
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
            let output = run(&mut add)?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned());
            }
        }
        let mut removed = 0;
        for (key, value) in &entries {
            let transport = value.get("transport").unwrap_or(value);
            if key != "hissabeth"
                && owned(value, app)
                && transport
                    .pointer("/env/HISSABETH_DATA_DIR")
                    .and_then(Value::as_str)
                    == workspace.to_str()
            {
                remove(name, path, key)?;
                removed += 1;
            }
        }
        Ok(format!(
            "{}; removed {removed} duplicate registrations",
            if current {
                "Already registered"
            } else {
                "Registered"
            }
        ))
    })();
    match result {
        Ok(message) => json!({"name":name,"success":true,"message":message}),
        Err(message) => json!({"name":name,"success":false,"message":message}),
    }
}
fn remove(name: &str, path: &Path, key: &str) -> Result<(), String> {
    let mut command = Command::new(path);
    command.args(["mcp", "remove"]);
    if name == "claude" {
        command.args(["--scope", "user"]);
    }
    let output = run(command.arg(key))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}
pub fn configure(targets: &[crate::preferences::Agent]) -> Value {
    static REGISTRATION: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = match REGISTRATION.lock() {
        Ok(guard) => guard,
        Err(e) => return json!([{"success":false,"message":e.to_string()}]),
    };
    let app = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return json!([{"success":false,"message":e.to_string()}]),
    };
    let workspace = crate::service::data_dir();
    Value::Array(
        targets
            .iter()
            .enumerate()
            .filter(|(index, target)| !targets[..*index].contains(target))
            .map(|(_, target)| {
                let name = target.name();
                match executable(name) {
                    Some(path) => register(name, &path, &app, &workspace),
                    None => json!({"name":name,"installed":false,"status":"not installed"}),
                }
            })
            .collect(),
    )
}
