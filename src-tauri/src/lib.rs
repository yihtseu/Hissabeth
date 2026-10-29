pub mod agents;
pub mod compute;
pub mod optics;
pub mod preferences;
pub mod service;
#[cfg(feature = "desktop")]
mod desktop {
    use super::*;
    use serde_json::Value;
    use std::sync::{Arc, Mutex};
    use tauri::{Emitter, Manager};

    struct Settings {
        snapshot: Mutex<preferences::Snapshot>,
        store: Arc<tauri_plugin_store::Store<tauri::Wry>>,
    }
    fn apply_window_preferences(app: &tauri::AppHandle, preferences: &preferences::Preferences) {
        let chinese = preferences.locale == "zh-CN";
        let theme = match preferences.appearance.theme.as_str() {
            "light" => Some(tauri::Theme::Light),
            "dark" | "high-contrast" => Some(tauri::Theme::Dark),
            _ => None,
        };
        for (label, title) in [
            (
                "main",
                if chinese {
                    "Hissabeth — 光学工作台"
                } else {
                    "Hissabeth — Optical Workbench"
                },
            ),
            (
                "settings",
                if chinese {
                    "Hissabeth — 设置"
                } else {
                    "Hissabeth — Settings"
                },
            ),
        ] {
            if let Some(window) = app.get_webview_window(label) {
                if let Err(error) = window.set_title(title) {
                    eprintln!("Window title: {error}");
                }
                if let Err(error) = window.set_theme(theme.clone()) {
                    eprintln!("Window theme: {error}");
                }
            }
        }
    }
    #[tauri::command]
    fn optical_call(
        state: tauri::State<'_, Arc<Mutex<service::Workspace>>>,
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
    async fn configure_agents(agents: Vec<preferences::Agent>) -> Result<Value, String> {
        if agents
            .iter()
            .enumerate()
            .any(|(i, agent)| agents[..i].contains(agent))
        {
            return Err("Agent targets must be unique".into());
        }
        tauri::async_runtime::spawn_blocking(move || agents::configure(&agents))
            .await
            .map_err(|e| e.to_string())
    }
    #[tauri::command]
    fn get_preferences(
        state: tauri::State<'_, Settings>,
    ) -> Result<preferences::Snapshot, preferences::Error> {
        state
            .snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .map_err(preferences::Error::storage)
    }
    #[tauri::command]
    fn set_preferences(
        app: tauri::AppHandle,
        state: tauri::State<'_, Settings>,
        preferences: preferences::Preferences,
        expected_revision: u64,
    ) -> Result<preferences::Snapshot, preferences::Error> {
        let mut snapshot = state.snapshot.lock().map_err(preferences::Error::storage)?;
        let saved = snapshot.commit(preferences, expected_revision, |next| {
            let previous = state.store.get("snapshot");
            state.store.set(
                "snapshot",
                serde_json::to_value(next).map_err(|e| e.to_string())?,
            );
            if let Err(error) = state.store.save() {
                match previous {
                    Some(value) => state.store.set("snapshot", value),
                    None => {
                        state.store.delete("snapshot");
                    }
                }
                return Err(error.to_string());
            }
            Ok(())
        })?;
        apply_window_preferences(&app, &saved.preferences);
        // A completed disk write remains confirmed even if a window closes before delivery.
        if let Err(error) = app.emit("preferences-changed", &saved) {
            eprintln!("Settings notification: {error}");
        }
        Ok(saved)
    }
    #[tauri::command]
    async fn open_settings(app: tauri::AppHandle, section: String) -> Result<(), String> {
        if !matches!(section.as_str(), "general" | "agents") {
            return Err("Unknown settings section".into());
        }
        let state = app.state::<Settings>();
        let snapshot = state.snapshot.lock().map_err(|e| e.to_string())?;
        if let Some(window) = app.get_webview_window("settings") {
            app.emit_to("settings", "settings-section", &section)
                .map_err(|e| e.to_string())?;
            window.unminimize().map_err(|e| e.to_string())?;
            window.show().map_err(|e| e.to_string())?;
            return window.set_focus().map_err(|e| e.to_string());
        }
        tauri::WebviewWindowBuilder::new(
            &app,
            "settings",
            tauri::WebviewUrl::App(format!("index.html?window=settings&section={section}").into()),
        )
        .title("Hissabeth — Settings")
        .inner_size(900.0, 700.0)
        .min_inner_size(480.0, 420.0)
        .build()
        .map_err(|e| e.to_string())?;
        apply_window_preferences(&app, &snapshot.preferences);
        Ok(())
    }
    pub fn run() {
        let dir = service::data_dir();
        std::fs::create_dir_all(&dir).expect("Create application data directory");
        let engine = Arc::new(Mutex::new(
            service::Workspace::open(dir.join("system.json")).expect("Open application workspace"),
        ));
        service::serve(engine.clone()).expect("Start optical service");
        let preferences_path = dir.join("preferences.json");
        let snapshot = preferences::load(&preferences_path);
        tauri::Builder::default()
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_store::Builder::default().build())
            .manage(engine)
            .setup(move |app| {
                let store = tauri_plugin_store::StoreBuilder::new(app, &preferences_path)
                    .create_new()
                    .disable_auto_save()
                    .build()?;
                store.set("snapshot", serde_json::to_value(&snapshot)?);
                let automatic_targets = if snapshot.preferences.agents.auto_register {
                    snapshot.preferences.agents.targets.clone()
                } else {
                    Vec::new()
                };
                apply_window_preferences(app.handle(), &snapshot.preferences);
                app.manage(Settings {
                    snapshot: Mutex::new(snapshot),
                    store,
                });
                if !automatic_targets.is_empty()
                    && std::env::var_os("HISSABETH_SKIP_AGENT_REGISTRATION").is_none()
                {
                    std::thread::spawn(move || {
                        eprintln!(
                            "Agent registration: {}",
                            agents::configure(&automatic_targets)
                        );
                    });
                }
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                optical_call,
                detect_agents,
                configure_agents,
                get_preferences,
                set_preferences,
                open_settings
            ])
            .run(tauri::generate_context!())
            .expect("Run Hissabeth");
    }
}
#[cfg(feature = "desktop")]
pub use desktop::run;
