//! Test MCP server (stdio, newline-delimited JSON-RPC). Tools:
//! - `echo {text}`: answers the text;
//! - `slow {ms}`: answers after `ms`, or never when cancelled first;
//! - `fail`: answers with `isError`;
//! - `crash`: the process exits without answering;
//! - `grow`: adds the `extra` tool and says the list changed.
//!
//! Arguments: `--exit-at-start` (no handshake), `--stderr <line>` (written at start),
//! `--no-tools` (no tools capability), `--version <protocol>` (answered version),
//! `--marker <file>` (appends one line per call, so a test can see what really ran),
//! `--env-tool` (adds `env {name}`: whether the variable is set and its length, never its value),
//! `--allocate-tool` (adds `allocate {mb}`: holds that many MiB, one at a time, and exits with
//! code 7 the moment one cannot be had, as a server out of memory does).
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    io::{self, BufRead, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

type Out = Arc<Mutex<io::Stdout>>;

fn send(out: &Out, value: &Value) {
    if let Ok(mut out) = out.lock() {
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    }
}

fn tools(grown: bool, env_tool: bool, allocate_tool: bool) -> Value {
    let object = |properties: Value| json!({"type": "object", "properties": properties});
    let mut list = vec![
        json!({"name": "echo", "description": "Echo the text back", "inputSchema": object(json!({"text": {"type": "string"}}))}),
        json!({"name": "slow", "description": "Answer after a delay", "inputSchema": object(json!({"ms": {"type": "integer"}}))}),
        json!({"name": "fail", "description": "Always reports an error", "inputSchema": object(json!({}))}),
        json!({"name": "crash", "description": "Exits without answering", "inputSchema": object(json!({}))}),
        json!({"name": "grow", "description": "Adds a tool", "inputSchema": object(json!({}))}),
    ];
    if grown {
        list.push(json!({"name": "extra", "description": "Added later", "inputSchema": object(json!({}))}));
    }
    if env_tool {
        list.push(json!({"name": "env", "description": "Whether a variable is set", "inputSchema": object(json!({"name": {"type": "string"}}))}));
    }
    if allocate_tool {
        list.push(json!({"name": "allocate", "description": "Holds memory", "inputSchema": object(json!({"mb": {"type": "integer"}}))}));
    }
    Value::Array(list)
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| arguments.iter().any(|a| a == name);
    let value = |name: &str| {
        arguments.iter().position(|a| a == name).and_then(|i| arguments.get(i + 1)).cloned()
    };
    if flag("--exit-at-start") {
        eprintln!("fixture: exiting at start");
        std::process::exit(3);
    }
    if let Some(line) = value("--stderr") {
        eprintln!("{line}");
    }
    let version = value("--version").unwrap_or_else(|| "2025-11-25".into());
    let marker = value("--marker");
    let out: Out = Arc::new(Mutex::new(io::stdout()));
    let cancelled: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let mut grown = false;
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (method.as_str(), id) {
            ("initialize", Some(id)) => {
                // A server that is slow to start, so a test can act while it starts.
                if let Some(ms) = value("--initialize-delay-ms").and_then(|ms| ms.parse().ok()) {
                    std::thread::sleep(Duration::from_millis(ms));
                }
                let capabilities = if flag("--no-tools") {
                    json!({})
                } else {
                    json!({"tools": {"listChanged": true}})
                };
                send(
                    &out,
                    &json!({"jsonrpc": "2.0", "id": id, "result": {
                        "protocolVersion": version,
                        "capabilities": capabilities,
                        "serverInfo": {"name": "aura-mcp-fixture", "version": "1.0.0"},
                        "instructions": "Fixture server for Aura tests."
                    }}),
                );
            }
            ("ping", Some(id)) => send(&out, &json!({"jsonrpc": "2.0", "id": id, "result": {}})),
            ("tools/list", Some(id)) => {
                let listed = tools(grown, flag("--env-tool"), flag("--allocate-tool"));
                send(&out, &json!({"jsonrpc": "2.0", "id": id, "result": {"tools": listed}}));
            }
            ("notifications/cancelled", None) => {
                if let Ok(mut set) = cancelled.lock() {
                    set.insert(params["requestId"].to_string());
                }
            }
            ("tools/call", Some(id)) => {
                let name = params["name"].as_str().unwrap_or_default().to_owned();
                let arguments = params["arguments"].clone();
                if let Some(marker) = &marker {
                    let _ = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(marker)
                        .and_then(|mut file| writeln!(file, "{name} {arguments}"));
                }
                match name.as_str() {
                    "echo" => send(
                        &out,
                        &json!({"jsonrpc": "2.0", "id": id, "result": {
                            "content": [{"type": "text", "text": arguments["text"].as_str().unwrap_or_default()}]
                        }}),
                    ),
                    "fail" => send(
                        &out,
                        &json!({"jsonrpc": "2.0", "id": id, "result": {
                            "content": [{"type": "text", "text": "fixture failure"}], "isError": true
                        }}),
                    ),
                    "crash" => std::process::exit(9),
                    "allocate" => {
                        let mb = arguments["mb"].as_u64().unwrap_or(1);
                        let mut held: Vec<Vec<u8>> = Vec::new();
                        for _ in 0..mb {
                            let mut chunk = Vec::new();
                            if chunk.try_reserve_exact(1 << 20).is_err() {
                                std::process::exit(7);
                            }
                            chunk.resize(1 << 20, 1_u8);
                            held.push(chunk);
                        }
                        send(
                            &out,
                            &json!({"jsonrpc": "2.0", "id": id, "result": {
                                "content": [{"type": "text", "text": format!("held {} MiB", held.len())}]
                            }}),
                        );
                    }
                    "env" => {
                        let name = arguments["name"].as_str().unwrap_or_default();
                        let text = match std::env::var(name) {
                            Ok(value) => {
                                format!("{name}: set, {} characters", value.chars().count())
                            }
                            Err(_) => format!("{name}: not set"),
                        };
                        send(
                            &out,
                            &json!({"jsonrpc": "2.0", "id": id, "result": {
                                "content": [{"type": "text", "text": text}]
                            }}),
                        );
                    }
                    "grow" => {
                        grown = true;
                        send(
                            &out,
                            &json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}),
                        );
                        send(
                            &out,
                            &json!({"jsonrpc": "2.0", "id": id, "result": {
                                "content": [{"type": "text", "text": "grown"}]
                            }}),
                        );
                    }
                    "slow" => {
                        let ms = arguments["ms"].as_u64().unwrap_or(1_000);
                        let (out, cancelled) = (out.clone(), cancelled.clone());
                        std::thread::spawn(move || {
                            let deadline = Instant::now() + Duration::from_millis(ms);
                            while Instant::now() < deadline {
                                if cancelled.lock().is_ok_and(|set| set.contains(&id.to_string())) {
                                    return;
                                }
                                std::thread::sleep(Duration::from_millis(10));
                            }
                            send(
                                &out,
                                &json!({"jsonrpc": "2.0", "id": id, "result": {
                                    "content": [{"type": "text", "text": format!("slept {ms}")}]
                                }}),
                            );
                        });
                    }
                    _ => send(
                        &out,
                        &json!({"jsonrpc": "2.0", "id": id, "error": {
                            "code": -32602, "message": format!("unknown tool {name}")
                        }}),
                    ),
                }
            }
            (_, Some(id)) => send(
                &out,
                &json!({"jsonrpc": "2.0", "id": id, "error": {
                    "code": -32601, "message": "method not found"
                }}),
            ),
            _ => {}
        }
    }
}
