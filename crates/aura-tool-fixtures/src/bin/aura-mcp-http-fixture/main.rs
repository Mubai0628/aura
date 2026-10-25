//! Test MCP server over Streamable HTTP, on 127.0.0.1 only. The first line of standard
//! output is its address (`http://127.0.0.1:<port>/mcp`; `--port <n>` fixes the port, otherwise
//! the system picks one); it exits when standard input closes. One request per connection
//! (`Connection: close`).
//!
//! `POST /mcp` speaks MCP. A session starts at `initialize` (`Mcp-Session-Id`); every later
//! request must carry it and the negotiated `Mcp-Protocol-Version` (missing → 400; unknown or
//! expired session → 404). `DELETE /mcp` ends the session. `POST /moved` answers 307. Tools:
//! - `echo {text}`: answers as JSON;
//! - `stream {text}`: an event stream that first asks the client for a ping and logs a line,
//!   then answers whether the ping came back;
//! - `slow {ms}`: an event stream that answers after `ms`, or ends without an answer when
//!   cancelled first;
//! - `fail`: answers with `isError`;
//! - `drop`: starts an event stream and ends it without an answer;
//! - `expire`: forgets the session, then answers (the next request gets 404);
//! - `grow`: adds the `extra` tool and says the list changed, in its event stream;
//! - `header`: answers the `X-Team` header it received;
//! - `error500`: HTTP 500.
//!
//! `--oauth` puts a fake OAuth authorization server in front of `/mcp` (see `oauth.rs`);
//! `--token-lifetime <seconds>` shortens its access tokens.
//!
//! `--require-auth <name>:<value>` answers 401 to an MCP request without exactly that header
//! and adds the tool `reflect`, which answers the header's value back (a server that leaks it).
//!
//! `--marker <file>` appends one line per event (`initialize`, `call <name> <arguments>`,
//! `cancelled <id>`, `pong` or `refused <id>` for the client's answer to a ping,
//! `deleted <session>`), so a test can see what really happened.
mod oauth;

use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
struct State {
    /// Session → the protocol version agreed for it.
    sessions: HashMap<String, String>,
    next_session: u32,
    next_ask: u32,
    cancelled: HashSet<String>,
    pongs: HashSet<String>,
    grown: bool,
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn read_chunked(reader: &mut impl BufRead) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut size = String::new();
        reader.read_line(&mut size).ok()?;
        let size = usize::from_str_radix(size.trim().split(';').next()?, 16).ok()?;
        let mut chunk = vec![0; size + 2];
        reader.read_exact(&mut chunk).ok()?;
        if size == 0 {
            return Some(body);
        }
        body.extend_from_slice(&chunk[..size]);
    }
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next()?.to_owned(), parts.next()?.to_owned());
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':')?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let mut request = Request { method, path, headers, body: Vec::new() };
    if request.header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        request.body = read_chunked(&mut reader)?;
    } else {
        let length = request.header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
        request.body = vec![0; length];
        reader.read_exact(&mut request.body).ok()?;
    }
    Some(request)
}

fn respond(stream: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    let mut head =
        format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn answer_json(stream: &mut TcpStream, session: Option<&str>, value: &Value) {
    let mut headers = vec![("Content-Type", "application/json")];
    if let Some(session) = session {
        headers.push(("Mcp-Session-Id", session));
    }
    respond(stream, "200 OK", &headers, value.to_string().as_bytes());
}

/// An event-stream response: its body lasts until the connection closes.
struct Events<'a>(&'a mut TcpStream);

impl<'a> Events<'a> {
    fn start(stream: &'a mut TcpStream) -> Self {
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(b": stream open\n\n");
        let _ = stream.flush();
        Self(stream)
    }

    fn send(&mut self, value: &Value) {
        let _ = write!(self.0, "event: message\ndata: {value}\n\n");
        let _ = self.0.flush();
    }
}

fn text(id: &Value, text: String) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": text}]}})
}

fn tools(grown: bool) -> Value {
    let object = |properties: Value| json!({"type": "object", "properties": properties});
    let text_input = || object(json!({"text": {"type": "string"}}));
    let mut list = vec![
        json!({"name": "echo", "description": "Echo the text back", "inputSchema": text_input()}),
        json!({"name": "stream", "description": "Echo through an event stream", "inputSchema": text_input()}),
        json!({"name": "slow", "description": "Answer after a delay", "inputSchema": object(json!({"ms": {"type": "integer"}}))}),
    ];
    for (name, description) in [
        ("fail", "Always reports an error"),
        ("drop", "Ends without answering"),
        ("expire", "Forgets the session"),
        ("grow", "Adds a tool"),
        ("header", "Answers the X-Team header"),
        ("error500", "Answers HTTP 500"),
    ] {
        list.push(
            json!({"name": name, "description": description, "inputSchema": object(json!({}))}),
        );
    }
    if grown {
        list.push(json!({"name": "extra", "description": "Added later", "inputSchema": object(json!({}))}));
    }
    Value::Array(list)
}

struct Server {
    state: Mutex<State>,
    marker: Option<String>,
    /// The header (lowercase name, value) every MCP request must carry.
    auth: Option<(String, String)>,
    /// `--oauth`: the authorization server and the tokens it issued.
    oauth: Option<Mutex<oauth::OAuth>>,
}

impl Server {
    fn note(&self, line: &str) {
        if let Some(marker) = &self.marker {
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(marker)
                .and_then(|mut file| writeln!(file, "{line}"));
        }
    }

    fn with<T>(&self, change: impl FnOnce(&mut State) -> T) -> T {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        change(&mut state)
    }

    fn serve(&self, mut stream: TcpStream) {
        let Some(request) = read_request(&stream) else { return };
        if let Some(oauth) = &self.oauth {
            let mut oauth = oauth.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match oauth.serve(&mut stream, &request) {
                None => {}
                Some(oauth::Event::Metadata) => return self.note("metadata"),
                Some(oauth::Event::Registered(client)) => {
                    return self.note(&format!("registered {client}"));
                }
                Some(oauth::Event::Authorized) => return self.note("authorized"),
                Some(oauth::Event::Issued) => return self.note("token issued"),
                Some(oauth::Event::Refreshed) => return self.note("token refreshed"),
                Some(oauth::Event::Refused(step)) => return self.note(&format!("refused {step}")),
                Some(oauth::Event::Revoked) => return self.note("revoked"),
            }
        }
        match (request.method.as_str(), request.path.as_str()) {
            ("POST", "/moved") => {
                respond(&mut stream, "307 Temporary Redirect", &[("Location", "/mcp")], b"");
            }
            ("DELETE", "/mcp") => {
                let session = request.header("mcp-session-id").unwrap_or_default().to_owned();
                self.with(|state| state.sessions.remove(&session));
                self.note(&format!("deleted {session}"));
                respond(&mut stream, "200 OK", &[], b"");
            }
            ("POST", "/mcp") => self.mcp(&mut stream, &request),
            _ => respond(&mut stream, "404 Not Found", &[], b""),
        }
    }

    fn mcp(&self, stream: &mut TcpStream, request: &Request) {
        if let Some(oauth) = &self.oauth {
            let oauth = oauth.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if !oauth.allows(request.header("authorization")) {
                let challenge = oauth.challenge();
                drop(oauth);
                self.note("unauthorized");
                return respond(
                    stream,
                    "401 Unauthorized",
                    &[("WWW-Authenticate", &challenge)],
                    b"",
                );
            }
        }
        if let Some((name, value)) = &self.auth
            && request.header(name) != Some(value.as_str())
        {
            self.note("unauthorized");
            return respond(
                stream,
                "401 Unauthorized",
                &[("WWW-Authenticate", "Bearer realm=\"fixture\"")],
                b"",
            );
        }
        let Ok(message) = serde_json::from_slice::<Value>(&request.body) else {
            return respond(stream, "400 Bad Request", &[], b"not JSON");
        };
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        if method == "initialize" {
            let version = params["protocolVersion"].as_str().unwrap_or("2025-11-25").to_owned();
            let session = self.with(|state| {
                state.next_session += 1;
                let session = format!("s-{}", state.next_session);
                state.sessions.insert(session.clone(), version.clone());
                session
            });
            self.note("initialize");
            let result = json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": true}, "logging": {}},
                "serverInfo": {"name": "aura-mcp-http-fixture", "version": "1.0.0"},
                "instructions": "HTTP fixture server for Aura tests."
            });
            let answer = json!({"jsonrpc": "2.0", "id": id, "result": result});
            return answer_json(stream, Some(&session), &answer);
        }
        let Some(session) = request.header("mcp-session-id").map(str::to_owned) else {
            return respond(stream, "400 Bad Request", &[], b"missing session");
        };
        let Some(version) = self.with(|state| state.sessions.get(&session).cloned()) else {
            return respond(stream, "404 Not Found", &[], b"unknown session");
        };
        if request.header("mcp-protocol-version") != Some(version.as_str()) {
            return respond(stream, "400 Bad Request", &[], b"wrong protocol version");
        }
        match (method.as_str(), id) {
            // The client's answer to the server's own ping: a result is a pong, an error is not.
            ("", Some(answered)) => {
                if message.get("result").is_some() {
                    self.with(|state| state.pongs.insert(answered.to_string()));
                    self.note("pong");
                } else {
                    self.note(&format!("refused {answered}"));
                }
                respond(stream, "202 Accepted", &[], b"");
            }
            ("notifications/cancelled", None) => {
                let cancelled = params["requestId"].to_string();
                self.note(&format!("cancelled {cancelled}"));
                self.with(|state| state.cancelled.insert(cancelled));
                respond(stream, "202 Accepted", &[], b"");
            }
            (_, None) => respond(stream, "202 Accepted", &[], b""),
            ("ping", Some(id)) => {
                answer_json(stream, None, &json!({"jsonrpc": "2.0", "id": id, "result": {}}));
            }
            ("tools/list", Some(id)) => {
                let grown = self.with(|state| state.grown);
                let mut listed = tools(grown);
                if self.auth.is_some()
                    && let Some(list) = listed.as_array_mut()
                {
                    list.push(json!({"name": "reflect", "description": "Answers the sign-in header back", "inputSchema": {"type": "object", "properties": {}}}));
                }
                let answer = json!({"jsonrpc": "2.0", "id": id, "result": {"tools": listed}});
                answer_json(stream, None, &answer);
            }
            ("tools/call", Some(id)) => self.call(stream, request, &session, &id, &params),
            (_, Some(id)) => answer_json(
                stream,
                None,
                &json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}}),
            ),
        }
    }

    fn call(
        &self,
        stream: &mut TcpStream,
        request: &Request,
        session: &str,
        id: &Value,
        params: &Value,
    ) {
        let name = params["name"].as_str().unwrap_or_default().to_owned();
        let arguments = params["arguments"].clone();
        self.note(&format!("call {name} {arguments}"));
        match name.as_str() {
            "echo" => {
                answer_json(
                    stream,
                    None,
                    &text(id, arguments["text"].as_str().unwrap_or_default().into()),
                );
            }
            "reflect" => {
                let name =
                    self.auth.as_ref().map(|(name, _)| name.as_str()).unwrap_or("authorization");
                let leaked = request.header(name).unwrap_or("none").to_owned();
                answer_json(stream, None, &text(id, leaked));
            }
            "header" => {
                answer_json(
                    stream,
                    None,
                    &text(id, request.header("x-team").unwrap_or("none").into()),
                );
            }
            "fail" => {
                let mut answer = text(id, "fixture failure".into());
                answer["result"]["isError"] = Value::Bool(true);
                answer_json(stream, None, &answer);
            }
            "error500" => respond(stream, "500 Internal Server Error", &[], b"fixture error"),
            "expire" => {
                self.with(|state| state.sessions.remove(session));
                answer_json(stream, None, &text(id, "expired".into()));
            }
            "drop" => drop(Events::start(stream)),
            "stream" => {
                let ask = self.with(|state| {
                    state.next_ask += 1;
                    json!(format!("srv-{}", state.next_ask))
                });
                let mut events = Events::start(stream);
                events.send(&json!({"jsonrpc": "2.0", "id": ask, "method": "ping"}));
                events.send(&json!({"jsonrpc": "2.0", "method": "notifications/message",
                    "params": {"level": "info", "data": "fixture log line"}}));
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut answered = false;
                while !answered && Instant::now() < deadline {
                    answered = self.with(|state| state.pongs.contains(&ask.to_string()));
                    std::thread::sleep(Duration::from_millis(10));
                }
                let echoed = arguments["text"].as_str().unwrap_or_default();
                events.send(&text(id, format!("streamed {echoed} (ping answered: {answered})")));
            }
            "slow" => {
                let ms = arguments["ms"].as_u64().unwrap_or(1_000);
                let mut events = Events::start(stream);
                let deadline = Instant::now() + Duration::from_millis(ms);
                while Instant::now() < deadline {
                    if self.with(|state| state.cancelled.contains(&id.to_string())) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                events.send(&text(id, format!("slept {ms}")));
            }
            "grow" => {
                self.with(|state| state.grown = true);
                let mut events = Events::start(stream);
                events
                    .send(&json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}));
                events.send(&text(id, "grown".into()));
            }
            _ => answer_json(
                stream,
                None,
                &json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": format!("unknown tool {name}")}}),
            ),
        }
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        arguments.iter().position(|a| a == name).and_then(|i| arguments.get(i + 1)).cloned()
    };
    let marker = value("--marker");
    // A fixed port keeps the trusted address across restarts (for a person trying it by hand).
    let port = value("--port").and_then(|port| port.parse::<u16>().ok()).unwrap_or(0);
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("listen on 127.0.0.1");
    let address = listener.local_addr().expect("local address");
    println!("http://{address}/mcp");
    let _ = std::io::stdout().flush();
    // The test (or E2E) that started the server holds its standard input.
    std::thread::spawn(|| {
        let mut sink = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut sink);
        std::process::exit(0);
    });
    let auth = value("--require-auth").and_then(|pair| {
        pair.split_once(':')
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
    });
    let lifetime =
        value("--token-lifetime").and_then(|seconds| seconds.parse().ok()).unwrap_or(3600);
    let oauth = arguments.iter().any(|a| a == "--oauth").then(|| {
        let quirks = oauth::Quirks {
            no_pkce: arguments.iter().any(|a| a == "--no-pkce"),
            metadata_elsewhere: arguments.iter().any(|a| a == "--metadata-elsewhere"),
        };
        Mutex::new(oauth::OAuth::new(
            &format!("http://{address}"),
            std::time::Duration::from_secs(lifetime),
            quirks,
        ))
    });
    let server = Arc::new(Server { state: Mutex::new(State::default()), marker, auth, oauth });
    for stream in listener.incoming().flatten() {
        let server = Arc::clone(&server);
        std::thread::spawn(move || server.serve(stream));
    }
}
