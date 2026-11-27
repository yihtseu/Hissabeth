use hissabeth_lib::service::{Scope, Workspace, data_dir};
use serde_json::{Value, json};

fn open_workspace() -> Result<Workspace, String> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Workspace::open(dir.join("system.json"))
}

/// Runs a batch script; scripts are often agent-written, so their file paths
/// are held to the same scope as broker calls.
fn run_script(path: Option<&String>, scope: &Scope) -> Result<(), String> {
    let run = || -> Result<Value, String> {
        let path = path.ok_or("--run requires a script path")?;
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let args = serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))?;
        open_workspace()?.call_scoped(scope, "batch", args)
    };
    let report = run().unwrap_or_else(|error| json!({"ok": false, "error": error}));
    println!("{report}");
    if report["ok"] == true {
        Ok(())
    } else {
        Err(report["failed"]["error"]
            .as_str()
            .or(report["error"].as_str())
            .unwrap_or("Batch failed")
            .to_owned())
    }
}

fn execute() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();

    if args.iter().any(|a| a == "--detect-agents") {
        println!("{}", hissabeth_lib::agents::detect());
        Ok(())
    } else if args.iter().any(|a| a == "--configure-agents") {
        println!(
            "{}",
            hissabeth_lib::agents::configure(&[
                hissabeth_lib::preferences::Agent::Codex,
                hissabeth_lib::preferences::Agent::Claude
            ])
        );
        Ok(())
    } else if args.iter().any(|a| a == "--mcp") {
        hissabeth_lib::service::relay(Scope::from_args(&args)?)
    } else if args.iter().any(|a| a == "--engine") {
        let scope = Scope::from_args(&args)?;
        if let Some(index) = args.iter().position(|arg| arg == "--run") {
            return run_script(args.get(index + 1), &scope);
        }
        let engine = open_workspace()?;
        hissabeth_lib::service::serve(std::sync::Arc::new(std::sync::RwLock::new(engine)), scope)?;
        loop {
            std::thread::park()
        }
    } else {
        #[cfg(feature = "desktop")]
        {
            hissabeth_lib::run();
            Ok(())
        }
        #[cfg(not(feature = "desktop"))]
        {
            Err("Use --engine or --mcp".into())
        }
    }
}

fn main() {
    if let Err(e) = execute() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
