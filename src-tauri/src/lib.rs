pub mod agents;
pub mod compute;
pub mod optics;
pub mod service;
#[cfg(feature = "desktop")]
mod desktop {
    use super::*;
    use serde_json::Value;
    use std::sync::{Arc, Mutex};
    #[tauri::command]
    fn optical_call(
        state: tauri::State<'_, Arc<Mutex<service::Engine>>>,
        name: String,
        args: Value,
    ) -> Result<Value, String> {
        state.lock().map_err(|e| e.to_string())?.call(&name, args)
    }
    #[tauri::command]
    fn detect_agents() -> Value {
        agents::detect()
    }
    #[tauri::command]
    fn configure_agents() -> Value {
        agents::configure()
    }
    pub fn run() {
        let dir = service::data_dir();
        std::fs::create_dir_all(&dir).expect("Create application data directory");
        let engine = Arc::new(Mutex::new(
            service::Engine::open(dir.join("system.json")).expect("Load optical system"),
        ));
        service::serve(engine.clone()).expect("Start optical service");
        tauri::Builder::default()
            .plugin(tauri_plugin_opener::init())
            .manage(engine)
            .setup(|_| {
                if std::env::var_os("HISSABETH_SKIP_AGENT_REGISTRATION").is_none() {
                    std::thread::spawn(|| {
                        eprintln!("Agent registration: {}", agents::configure());
                    });
                }
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                optical_call,
                detect_agents,
                configure_agents
            ])
            .run(tauri::generate_context!())
            .expect("Run Hissabeth");
    }
}
#[cfg(feature = "desktop")]
pub use desktop::run;
