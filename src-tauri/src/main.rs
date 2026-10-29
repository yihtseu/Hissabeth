fn execute() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    let result = if args.iter().any(|a| a == "--detect-agents") {
        println!("{}", hissabeth_lib::agents::detect());
        Ok(())
    } else if args.iter().any(|a| a == "--configure-agents") {
        println!("{}", hissabeth_lib::agents::configure());
        Ok(())
    } else if args.iter().any(|a| a == "--mcp") {
        hissabeth_lib::service::relay()
    } else if args.iter().any(|a| a == "--engine") {
        let dir = hissabeth_lib::service::data_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let engine = hissabeth_lib::service::Workspace::open(dir.join("system.json"))?;
        hissabeth_lib::service::serve(std::sync::Arc::new(std::sync::Mutex::new(engine)))?;
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
    };
    result
}

fn main() {
    if let Err(e) = execute() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
