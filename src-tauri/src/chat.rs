use crate::{agents, service};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;
use tauri::ipc::Channel;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Codex,
    Claude,
    Chatgpt,
}
impl Provider {
    fn name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Chatgpt => "chatgpt",
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Conversation {
    id: String,
    provider: Provider,
    model: String,
    title: String,
    updated_at: u64,
    messages: Vec<Value>,
    session: Option<String>,
    outcome: String,
    error: String,
}
pub struct Chat {
    active: Option<Active>,
    account_changing: bool,
    conversations: Vec<Conversation>,
    path: PathBuf,
    load_error: Option<String>,
}
impl Chat {
    pub fn load() -> Self {
        let path = service::data_dir().join("chat-history.json");
        let loaded = match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice::<Vec<Conversation>>(&bytes).map_err(|e| e.to_string())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.to_string()),
        };
        let (conversations, load_error) = match loaded {
            Ok(records) => (records, None),
            Err(error) => (
                Vec::new(),
                Some(format!("Read conversation history: {error}")),
            ),
        };
        Self {
            active: None,
            account_changing: false,
            conversations,
            path,
            load_error,
        }
    }
    fn save(&self) -> Result<(), String> {
        if let Some(error) = &self.load_error {
            return Err(error.clone());
        }
        let mut file = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())
            .map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(file.as_file_mut(), &self.conversations)
            .map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(&self.path)
            .map_err(|e| format!("Save conversation history: {e}"))?;
        Ok(())
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn view(record: &Conversation) -> Value {
    let mut value = serde_json::to_value(record).unwrap();
    value.as_object_mut().unwrap().remove("session");
    value
}
pub fn history(state: &State) -> Result<Value, String> {
    let chat = state.lock().map_err(|e| e.to_string())?;
    if let Some(error) = &chat.load_error {
        return Err(error.clone());
    }
    Ok(
        json!({"conversations":chat.conversations.iter().map(view).collect::<Vec<_>>(),"activeId":chat.active.as_ref().map(|active| &active.id),"activeConversationId":chat.active.as_ref().map(|active| &active.conversation_id)}),
    )
}
struct Active {
    id: String,
    conversation_id: String,
    child: Arc<Mutex<Child>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}
pub type State = Arc<Mutex<Chat>>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    id: String,
    provider: Provider,
    conversation_id: Option<String>,
    prompt: String,
    model: String,
    /// Reference images attached to the message.
    #[serde(default)]
    images: Vec<Image>,
}

/// An image attached to a chat message, as base64.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    mime: String,
    filename: String,
    data: String,
}
/// Images per message and decoded bytes per image (the Claude API limit).
const IMAGES: usize = 6;
const IMAGE_BYTES: usize = 5 * 1024 * 1024;

/// Checks the attached images and stores each under its content hash in
/// `dir`, the directory `figure` reads. Returns the history record and the
/// bridge input of each image.
fn store_images(dir: &std::path::Path, images: &[Image]) -> Result<Vec<(Value, Value)>, String> {
    use base64::Engine;
    use sha2::Digest;
    if images.len() > IMAGES {
        return Err(format!("Attach at most {IMAGES} images to one message"));
    }
    images
        .iter()
        .map(|image| {
            let ext = match image.mime.as_str() {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/webp" => "webp",
                "image/gif" => "gif",
                _ => {
                    return Err(format!(
                        "Attach PNG, JPEG, WebP or GIF images, not {:?}",
                        image.mime
                    ));
                }
            };
            let name: String = image
                .filename
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect();
            if image.data.len() > IMAGE_BYTES.div_ceil(3) * 4 {
                return Err(format!(
                    "Image {name} is larger than {} MB",
                    IMAGE_BYTES >> 20
                ));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&image.data)
                .map_err(|_| format!("Image {name} is not valid base64"))?;
            // The content must be the type it claims, so the model and the
            // panel never receive anything else under an image name.
            let signed = match ext {
                "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
                "jpg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
                "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
                _ => bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
            };
            if bytes.is_empty() || bytes.len() > IMAGE_BYTES || !signed {
                return Err(format!(
                    "Image {name} is not a {} image of at most {} MB",
                    ext.to_uppercase(),
                    IMAGE_BYTES >> 20
                ));
            }
            let hash = sha2::Sha256::digest(&bytes);
            let file = format!(
                "{}.{ext}",
                hash[..16]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            );
            std::fs::create_dir_all(dir).map_err(|e| format!("Store image {name}: {e}"))?;
            let path = dir.join(&file);
            std::fs::write(&path, &bytes).map_err(|e| format!("Store image {name}: {e}"))?;
            Ok((
                json!({"mime":image.mime, "filename":name, "bytes":bytes.len(), "file":file}),
                json!({"mime":image.mime, "path":path}),
            ))
        })
        .collect()
}

/// A drawing the agent bridge stored for the conversation (see
/// scripts/chat-figures.ts), returned as base64.
pub fn figure(file: &str) -> Result<String, String> {
    read_figure(&service::data_dir().join("chat-figures"), file)
}
fn read_figure(dir: &std::path::Path, file: &str) -> Result<String, String> {
    // Only names the bridge writes: 32 lowercase hex digits of the content
    // hash and an image extension, so no path can leave the directory.
    let valid = file.split_once('.').is_some_and(|(hash, ext)| {
        hash.len() == 32
            && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            && matches!(ext, "svg" | "png" | "jpg" | "webp" | "gif")
    });
    if !valid {
        return Err(format!("Invalid chat figure name {file:?}"));
    }
    let bytes =
        std::fs::read(dir.join(file)).map_err(|e| format!("Read chat figure {file}: {e}"))?;
    Ok(crate::optics::blueprint_render::base64(&bytes))
}
pub fn runtime() -> Value {
    json!({"node": agents::executable("node"), "agents": agents::detect()})
}
fn bridge_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist/agent-bridge.mjs");
    #[cfg(not(debug_assertions))]
    let path = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("agent-bridge.mjs");
    let _ = app;
    if path.is_file() {
        Ok(path)
    } else {
        Err("Agent bridge is missing. Run bun run build:agent.".into())
    }
}

fn chatgpt_paths(app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let config = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    let runtime = std::env::var_os("HISSABETH_SIWC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| config.join("siwc"));
    let runtime = std::path::absolute(runtime).map_err(|error| error.to_string())?;
    Ok((runtime, config.join("chatgpt")))
}

fn chatgpt_command(app: &tauri::AppHandle, action: &str) -> Result<Value, String> {
    let (runtime, storage) = chatgpt_paths(app)?;
    let node = agents::executable("node").ok_or("Install Node.js 22 or later to use ChatGPT")?;
    let output = Command::new(node)
        .arg(bridge_path(app)?)
        .args(["--chatgpt", action])
        .arg(runtime)
        .arg(storage)
        .output()
        .map_err(|error| format!("Start ChatGPT bridge: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("Read ChatGPT bridge: {error}"))
}

pub fn account(app: &tauri::AppHandle, state: &State, action: &str) -> Result<Value, String> {
    if action == "status" {
        return chatgpt_command(app, action);
    }
    if !matches!(action, "sign-in" | "enable-plan" | "disconnect") {
        return Err("Unknown ChatGPT account action".into());
    }
    {
        let mut chat = state.lock().map_err(|error| error.to_string())?;
        if chat.active.is_some() || chat.account_changing {
            return Err("Finish the current conversation or account change first".into());
        }
        chat.account_changing = true;
    }
    let result = chatgpt_command(app, action);
    state
        .lock()
        .map_err(|error| error.to_string())?
        .account_changing = false;
    result
}

pub fn models(app: tauri::AppHandle, provider: Provider) -> Result<Value, String> {
    if provider == Provider::Chatgpt {
        return chatgpt_command(&app, "models");
    }
    let executable =
        agents::executable(provider.name()).ok_or("Install the selected agent CLI first")?;
    let node = agents::executable("node").ok_or("Install Node.js to read agent models")?;
    let output = agents::run(
        Command::new(node)
            .arg(bridge_path(&app)?)
            .arg("--models")
            .arg(provider.name())
            .arg(executable)
            .arg(service::data_dir())
            .arg(env!("CARGO_PKG_VERSION")),
    )?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("Read agent models: {e}"))
}

pub fn start(
    app: tauri::AppHandle,
    state: State,
    request: Request,
    channel: Channel<Value>,
) -> Result<(), String> {
    if (request.prompt.trim().is_empty() && request.images.is_empty())
        || request.prompt.len() > 65536
    {
        return Err("Enter a message of at most 65536 bytes".into());
    }
    if request.provider == Provider::Chatgpt && !request.images.is_empty() {
        return Err(
            "ChatGPT chat takes text only; use Codex or Claude to read a reference image".into(),
        );
    }
    if request.id.is_empty() || request.id.len() > 128 || request.model.len() > 256 {
        return Err("Invalid chat request".into());
    }
    let provider = request.provider.name();
    if request.provider == Provider::Chatgpt && request.model.trim().is_empty() {
        return Err("Select a ChatGPT model first".into());
    }
    let node =
        agents::executable("node").ok_or("Install Node.js 22 or later to use built-in chat")?;
    let bridge = bridge_path(&app)?;
    let workspace = service::data_dir();
    let mut chat = state.lock().map_err(|e| e.to_string())?;
    if chat.active.is_some() {
        return Err("A conversation is already running".into());
    }
    if let Some(error) = &chat.load_error {
        return Err(error.clone());
    }
    let conversation_id = request
        .conversation_id
        .clone()
        .unwrap_or_else(|| request.id.clone());
    let previous = chat
        .conversations
        .iter()
        .find(|record| record.id == conversation_id)
        .cloned();
    if request.conversation_id.is_some() && previous.is_none() {
        return Err("Conversation no longer exists".into());
    }
    if previous
        .as_ref()
        .is_some_and(|record| record.provider != request.provider || record.model != request.model)
    {
        return Err("Start a new conversation to change agent or model".into());
    }
    let session = previous.as_ref().and_then(|record| record.session.clone());
    let (images, inputs): (Vec<_>, Vec<_>) =
        store_images(&workspace.join("chat-figures"), &request.images)?
            .into_iter()
            .unzip();
    let payload = if request.provider == Provider::Chatgpt {
        if chat.account_changing {
            return Err("Finish the ChatGPT account change first".into());
        }
        let (runtime, storage) = chatgpt_paths(&app)?;
        let mut input = Vec::new();
        if session.is_some()
            && let Some(previous) = &previous
        {
            input.extend(previous.messages.iter().filter_map(|message| {
                let role = match message["kind"].as_str()? {
                    "user" => "user",
                    "text" => "assistant",
                    _ => return None,
                };
                Some(json!({"role":role, "content":message["text"].as_str()?}))
            }));
        }
        input.push(json!({"role":"user", "content":request.prompt}));
        json!({"provider":provider, "model":request.model, "session":session,
            "runtime":runtime, "storage":storage, "input":input})
    } else {
        let executable = agents::executable(provider)
            .ok_or_else(|| format!("Install the {provider} CLI first"))?;
        let app_executable = std::env::current_exe().map_err(|e| e.to_string())?;
        json!({"provider":provider,"model":request.model,"prompt":request.prompt,"images":inputs,"session":session,
            "app":app_executable,"workspace":workspace,"executable":executable})
    };
    let mut command = Command::new(node);
    command
        .arg(bridge)
        .current_dir(&workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Start agent bridge: {e}"))?;
    let mut record = previous.clone().unwrap_or_else(|| Conversation {
        id: conversation_id.clone(),
        provider: request.provider,
        model: request.model.clone(),
        title: request
            .prompt
            .trim()
            .lines()
            .next()
            .or_else(|| images.first().and_then(|image| image["filename"].as_str()))
            .unwrap_or("")
            .chars()
            .take(80)
            .collect(),
        updated_at: now(),
        messages: Vec::new(),
        session: None,
        outcome: String::new(),
        error: String::new(),
    });
    let mut message = json!({"id":request.id,"kind":"user","text":request.prompt});
    if !images.is_empty() {
        message["images"] = images.into();
    }
    record.messages.push(message);
    record.updated_at = now();
    record.outcome.clear();
    record.error.clear();
    chat.conversations
        .retain(|record| record.id != conversation_id);
    chat.conversations.push(record);
    if let Err(error) = chat.save() {
        chat.conversations
            .retain(|record| record.id != conversation_id);
        if let Some(previous) = previous {
            chat.conversations.push(previous);
        }
        kill_tree(&mut child);
        let _ = child.wait();
        return Err(error);
    }
    if let Err(error) = writeln!(child.stdin.as_mut().unwrap(), "{payload}") {
        kill_tree(&mut child);
        let _ = child.wait();
        chat.conversations
            .retain(|record| record.id != conversation_id);
        if let Some(previous) = previous {
            chat.conversations.push(previous);
        }
        return match chat.save() {
            Ok(()) => Err(error.to_string()),
            Err(storage) => Err(format!("{error}; {storage}")),
        };
    }
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let process = Arc::new(Mutex::new(child));
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    chat.active = Some(Active {
        id: request.id.clone(),
        conversation_id: conversation_id.clone(),
        child: process.clone(),
        cancelled: cancelled.clone(),
    });
    drop(chat);
    // Drain stderr concurrently so SDK diagnostics cannot fill a subprocess pipe.
    let errors = std::thread::spawn(move || {
        let mut saved = Vec::new();
        let mut reader = BufReader::new(stderr);
        let mut buffer = [0; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let remaining = 8192usize.saturating_sub(saved.len());
            saved.extend_from_slice(&buffer[..count.min(remaining)]);
        }
        String::from_utf8_lossy(&saved).into_owned()
    });
    std::thread::spawn(move || {
        let mut terminal = None;
        let mut delivery_failed = false;
        for line in BufReader::new(stdout).lines() {
            let event = match line
                .map_err(|e| e.to_string())
                .and_then(|line| serde_json::from_str::<Value>(&line).map_err(|e| e.to_string()))
            {
                Ok(event) => event,
                Err(error) => {
                    terminal = Some(
                        json!({"kind":"error","text":format!("Agent bridge protocol: {error}")}),
                    );
                    break;
                }
            };
            let kind = event["kind"].as_str().unwrap_or("");
            if kind == "session" {
                if let Some(session) = event["session"].as_str()
                    && let Ok(mut chat) = state.lock()
                {
                    if let Some(record) = chat
                        .conversations
                        .iter_mut()
                        .find(|record| record.id == conversation_id)
                    {
                        record.session = Some(session.to_string());
                    }
                    if let Err(error) = chat.save() {
                        let _ = channel.send(json!({"kind":"storage_error","text":error}));
                    }
                }
            } else if matches!(kind, "done" | "cancelled" | "error") {
                terminal = Some(event);
            } else {
                if matches!(kind, "text" | "tool")
                    && let Ok(mut chat) = state.lock()
                    && let Some(record) = chat
                        .conversations
                        .iter_mut()
                        .find(|record| record.id == conversation_id)
                {
                    let key = format!("{}:{}", request.id, event["id"].as_str().unwrap_or(""));
                    if let Some(existing) =
                        record.messages.iter_mut().find(|item| item["id"] == key)
                    {
                        for (key, value) in event.as_object().unwrap() {
                            existing[key] = value.clone();
                        }
                        existing["id"] = json!(key);
                    } else {
                        let mut item = event.clone();
                        item["id"] = json!(key);
                        record.messages.push(item);
                    }
                }
                if channel.send(event).is_err() {
                    delivery_failed = true;
                    break;
                }
            }
        }
        if (terminal.is_none() || delivery_failed)
            && let Ok(mut child) = process.lock()
        {
            kill_tree(&mut child);
        }
        let status = process.lock().ok().and_then(|mut child| child.wait().ok());
        let diagnostic = errors.join().unwrap_or_default();
        let mut event = terminal.unwrap_or_else(|| json!({"kind":"error","text":format!("Agent bridge exited without a result. {}", diagnostic.trim())}));
        if status.is_some_and(|status| !status.success()) && event["kind"] == "done" {
            event =
                json!({"kind":"error","text":format!("Agent bridge failed. {}",diagnostic.trim())});
        }
        if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            event = json!({"kind":"cancelled"});
        }
        if let Ok(mut chat) = state.lock() {
            if chat
                .active
                .as_ref()
                .is_some_and(|active| active.id == request.id)
            {
                chat.active = None;
            }
            if let Some(record) = chat
                .conversations
                .iter_mut()
                .find(|record| record.id == conversation_id)
            {
                for item in &mut record.messages {
                    if item["kind"] == "tool" && item["status"] == "in_progress" {
                        item["status"] = json!("interrupted");
                    }
                }
                record.outcome = event["kind"].as_str().unwrap_or("error").to_string();
                record.error = event["text"].as_str().unwrap_or("").to_string();
                record.updated_at = now();
            }
            if let Err(error) = chat.save() {
                let _ = channel.send(json!({"kind":"storage_error","text":error}));
            }
        }
        let _ = channel.send(event);
    });
    Ok(())
}

pub fn cancel(state: &State, id: &str) -> Result<(), String> {
    let chat = state.lock().map_err(|e| e.to_string())?;
    let Some(active) = &chat.active else {
        return Ok(());
    };
    if active.id != id {
        return Err("This turn is no longer active".into());
    }
    active
        .cancelled
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let process = active.child.clone();
    {
        let mut child = process.lock().map_err(|e| e.to_string())?;
        if let Some(input) = child.stdin.as_mut() {
            let _ = writeln!(input, "{{\"kind\":\"cancel\"}}");
        }
    }
    // SDK cancellation should reap its own children; stop a stuck bridge after a grace period.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(5));
        if let Ok(mut child) = process.lock()
            && matches!(child.try_wait(), Ok(None))
        {
            kill_tree(&mut child);
        }
    });
    Ok(())
}
pub fn stop(state: &State) {
    let id = state
        .lock()
        .ok()
        .and_then(|chat| chat.active.as_ref().map(|active| active.id.clone()));
    if let Some(id) = id {
        let _ = cancel(state, &id);
    }
}

fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .status();
    }
    let _ = child.kill();
}
pub fn shutdown(state: &State) {
    if let Ok(chat) = state.lock()
        && let Some(active) = &chat.active
        && let Ok(mut child) = active.child.lock()
    {
        kill_tree(&mut child);
    }
}

#[cfg(test)]
mod tests {
    use super::{Image, read_figure, store_images};
    use serde_json::json;

    #[test]
    fn attached_images_are_checked_and_stored_under_their_hash() {
        let dir = tempfile::tempdir().unwrap();
        let png = b"\x89PNG\r\n\x1a\nbody";
        let image = |mime: &str, data: &[u8]| Image {
            mime: mime.into(),
            filename: "ref\u{7}.png".into(),
            data: crate::optics::blueprint_render::base64(data),
        };
        let stored = store_images(dir.path(), &[image("image/png", png)]).unwrap();
        let (record, input) = &stored[0];
        let file = record["file"].as_str().unwrap();
        assert_eq!(
            record,
            &json!({"mime":"image/png","filename":"ref.png","bytes":png.len(),"file":file})
        );
        assert_eq!(input["path"], json!(dir.path().join(file)));
        // The stored copy is what the chat panel reads back.
        assert_eq!(
            read_figure(dir.path(), file).unwrap(),
            crate::optics::blueprint_render::base64(png)
        );
        let webp = b"RIFF\0\0\0\0WEBPVP8 ";
        assert!(
            store_images(dir.path(), &[image("image/webp", webp)]).unwrap()[0].0["file"]
                .as_str()
                .unwrap()
                .ends_with(".webp")
        );
        let gif = b"GIF89a\x01\0\x01\0";
        assert!(
            store_images(dir.path(), &[image("image/gif", gif)]).unwrap()[0].0["file"]
                .as_str()
                .unwrap()
                .ends_with(".gif")
        );
        for (bad, why) in [
            (image("image/svg+xml", b"<svg/>"), "Attach PNG"),
            (image("image/jpeg", png), "not a JPG"),
            (image("image/gif", b"GIF90a"), "not a GIF"),
            (image("image/png", b""), "not a PNG"),
            (
                Image {
                    data: "@@".into(),
                    ..image("image/png", png)
                },
                "not valid base64",
            ),
            (
                image("image/png", &[png.as_slice(), &vec![0; 5 << 20]].concat()),
                "larger than 5 MB",
            ),
        ] {
            assert!(
                store_images(dir.path(), &[bad]).unwrap_err().contains(why),
                "{why}"
            );
        }
        let many: Vec<_> = (0..7).map(|_| image("image/png", png)).collect();
        assert!(
            store_images(dir.path(), &many)
                .unwrap_err()
                .contains("at most 6")
        );
    }

    #[test]
    fn figures_are_read_only_by_their_stored_name() {
        let dir = tempfile::tempdir().unwrap();
        let name = format!("{}.svg", "0123456789abcdef".repeat(2));
        std::fs::write(dir.path().join(&name), b"<svg/>").unwrap();
        assert_eq!(read_figure(dir.path(), &name).unwrap(), "PHN2Zy8+");
        std::fs::write(dir.path().join("secret.txt"), b"x").unwrap();
        for bad in [
            "../secret.txt",
            "secret.txt",
            "0123456789ABCDEF0123456789ABCDEF.svg",
            "0123456789abcdef0123456789abcdef.svg/../../x",
            "0123456789abcdef0123456789abcdef.html",
        ] {
            assert!(
                read_figure(dir.path(), bad)
                    .unwrap_err()
                    .starts_with("Invalid"),
                "{bad}"
            );
        }
        assert!(read_figure(dir.path(), &name.replace(".svg", ".png")).is_err());
    }
}
