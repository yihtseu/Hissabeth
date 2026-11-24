pub mod agents;
#[cfg(feature = "desktop")]
mod chat;
pub mod compute;
pub mod optics;
pub mod preferences;
pub mod service;
#[cfg(feature = "desktop")]
mod desktop {
    use super::*;
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex, RwLock};
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
    async fn optical_call(
        state: tauri::State<'_, service::SharedWorkspace>,
        notifier: tauri::State<'_, service::ChangeNotifier>,
        name: String,
        args: Value,
    ) -> Result<Value, String> {
        let workspace = state.inner().clone();
        let notifier = notifier.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            service::dispatch(&workspace, &name, args, Some(&notifier))
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[tauri::command]
    fn detect_agents() -> Value {
        agents::detect()
    }
    #[tauri::command]
    async fn configure_agents(agents: Vec<preferences::Agent>) -> Result<Value, String> {
        tauri::async_runtime::spawn_blocking(move || agents::configure(&agents))
            .await
            .map_err(|e| e.to_string())
    }
    #[tauri::command]
    fn chat_runtime() -> Value {
        chat::runtime()
    }
    #[tauri::command]
    async fn chat_models(app: tauri::AppHandle, provider: chat::Provider) -> Result<Value, String> {
        tauri::async_runtime::spawn_blocking(move || chat::models(app, provider))
            .await
            .map_err(|e| e.to_string())?
    }
    #[tauri::command]
    async fn chatgpt_account(
        app: tauri::AppHandle,
        state: tauri::State<'_, chat::State>,
        action: String,
    ) -> Result<Value, String> {
        let state = state.inner().clone();
        tauri::async_runtime::spawn_blocking(move || chat::account(&app, &state, &action))
            .await
            .map_err(|error| error.to_string())?
    }
    #[tauri::command]
    fn chat_start(
        app: tauri::AppHandle,
        state: tauri::State<'_, chat::State>,
        request: chat::Request,
        channel: tauri::ipc::Channel<Value>,
    ) -> Result<(), String> {
        chat::start(app, state.inner().clone(), request, channel)
    }
    #[tauri::command]
    fn chat_cancel(state: tauri::State<'_, chat::State>, id: String) -> Result<(), String> {
        chat::cancel(state.inner(), &id)
    }
    #[tauri::command]
    fn chat_history(state: tauri::State<'_, chat::State>) -> Result<Value, String> {
        chat::history(state.inner())
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
        let engine = Arc::new(RwLock::new(
            service::Workspace::open(dir.join("system.json")).expect("Open application workspace"),
        ));
        let preferences_path = dir.join("preferences.json");
        let snapshot = preferences::load(&preferences_path);
        tauri::Builder::default()
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_store::Builder::default().build())
            .manage(engine.clone())
            .manage(Arc::new(Mutex::new(chat::Chat::load())))
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
                let handle = app.handle().clone();
                let notifier: service::ChangeNotifier = Arc::new(move |revision| {
                    if let Err(error) =
                        handle.emit("workspace-changed", json!({"revision": revision}))
                    {
                        eprintln!("Workspace notification: {error}");
                    }
                });
                app.manage(notifier.clone());
                service::serve_with_notifier(engine, Some(notifier))
                    .map_err(std::io::Error::other)?;
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
                chat_runtime,
                chat_models,
                chatgpt_account,
                chat_start,
                chat_cancel,
                chat_history,
                optical_call,
                detect_agents,
                configure_agents,
                get_preferences,
                set_preferences,
                open_settings
            ])
            .build(tauri::generate_context!())
            .expect("Build Hissabeth")
            .run(|app, event| match event {
                tauri::RunEvent::ExitRequested { .. } => {
                    chat::stop(app.state::<chat::State>().inner())
                }
                tauri::RunEvent::Exit => chat::shutdown(app.state::<chat::State>().inner()),
                _ => {}
            });
    }
}
#[cfg(feature = "desktop")]
pub use desktop::run;
