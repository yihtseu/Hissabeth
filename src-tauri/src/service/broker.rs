// SPDX-License-Identifier: GPL-3.0-or-later
// Multi-threaded loopback JSON-RPC broker and MCP stdio relay.
use super::{ChangeNotifier, SharedWorkspace, ToolCaller, data_dir, dispatch, tools};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

const MAX_CONCURRENT_CONNECTIONS: usize = 32;

pub fn rpc(request: Value, engine: &mut impl ToolCaller) -> Option<Value> {
    respond(request, |name, args| engine.call_tool(name, args))
}

pub fn rpc_shared(
    request: Value,
    workspace: &SharedWorkspace,
    on_change: Option<&ChangeNotifier>,
) -> Option<Value> {
    respond(request, |name, args| {
        dispatch(workspace, name, args, on_change)
    })
}

fn respond(
    request: Value,
    mut call_tool: impl FnMut(&str, Value) -> Result<Value, String>,
) -> Option<Value> {
    let id = request.get("id")?.clone();
    let result = match request["method"].as_str().unwrap_or("") {
        "initialize" => Ok(
            json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"hissabeth","version":env!("CARGO_PKG_VERSION")},"instructions":"Read workspace_status first; if recovery is active, use workspace_retry or workspace_recover before optical tools. Read system_get before editing. All dimensions are mm and wavelengths nm. GUI reflects changes immediately. Use field_index for stored fields. Every mutating operation requires the current revision."}),
        ),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools":tools()})),
        "tools/call" => {
            let p = &request["params"];
            let r = call_tool(
                p["name"].as_str().unwrap_or(""),
                p.get("arguments").cloned().unwrap_or(json!({})),
            );
            Ok(match r {
                Ok(v) => {
                    json!({"content":[{"type":"text","text":v.to_string()}],"isError":false})
                }
                Err(e) => json!({"content":[{"type":"text","text":e}],"isError":true}),
            })
        }
        _ => Err(json!({"code":-32601,"message":"Method not found"})),
    };
    Some(match result {
        Ok(v) => json!({"jsonrpc":"2.0","id":id,"result":v}),
        Err(e) => json!({"jsonrpc":"2.0","id":id,"error":e}),
    })
}

pub fn serve(engine: SharedWorkspace) -> Result<(), String> {
    serve_with_notifier(engine, None)
}

pub fn serve_with_notifier(
    engine: SharedWorkspace,
    on_change: Option<ChangeNotifier>,
) -> Result<(), String> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let token: Arc<str> = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .into();
    let endpoint = json!({"address":listener.local_addr().unwrap().to_string(),"token":&*token});
    let path = dir.join("endpoint.json");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(endpoint.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            if active.fetch_add(1, Ordering::AcqRel) >= MAX_CONCURRENT_CONNECTIONS {
                active.fetch_sub(1, Ordering::AcqRel);
                continue;
            }
            let engine = engine.clone();
            let token = token.clone();
            let on_change = on_change.clone();
            let active = active.clone();
            std::thread::spawn(move || {
                struct Guard(Arc<AtomicUsize>);
                impl Drop for Guard {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, Ordering::AcqRel);
                    }
                }
                let _guard = Guard(active);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(120)));
                let mut line = String::new();
                if BufReader::new((&mut stream).take(16_000_000))
                    .read_line(&mut line)
                    .is_err()
                {
                    return;
                }
                let Ok(req) = serde_json::from_str::<Value>(&line) else {
                    return;
                };
                if req["token"].as_str() != Some(&*token) {
                    return;
                }
                if let Some(r) = rpc_shared(req["request"].clone(), &engine, on_change.as_ref()) {
                    let _ = writeln!(stream, "{r}");
                }
            });
        }
    });
    Ok(())
}

pub fn relay() -> Result<(), String> {
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}})
                );
                continue;
            }
        };
        if request.get("id").is_none() {
            continue;
        }
        let response = (|| -> Result<Value, String> {
            let endpoint: Value = serde_json::from_slice(
                &std::fs::read(data_dir().join("endpoint.json"))
                    .map_err(|_| "Open Hissabeth before connecting MCP")?,
            )
            .map_err(|e| e.to_string())?;
            let address = endpoint["address"].as_str().ok_or("Invalid endpoint")?;
            let addr: std::net::SocketAddr = address.parse().map_err(|_| "Invalid endpoint")?;
            if !addr.ip().is_loopback() {
                return Err("Endpoint must be loopback".into());
            }
            let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))
                .map_err(|e| format!("Open Hissabeth: {e}"))?;
            stream
                .set_read_timeout(Some(Duration::from_secs(120)))
                .map_err(|e| e.to_string())?;
            writeln!(
                stream,
                "{}",
                json!({"token":endpoint["token"],"request":request})
            )
            .map_err(|e| e.to_string())?;
            let mut result = String::new();
            BufReader::new(stream)
                .read_line(&mut result)
                .map_err(|e| e.to_string())?;
            serde_json::from_str(&result).map_err(|e| e.to_string())
        })();
        println!(
            "{}",
            response.unwrap_or_else(
                |e| json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32603,"message":e}})
            )
        );
    }
    Ok(())
}
