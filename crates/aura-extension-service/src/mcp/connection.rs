//! One running server: a JSON-RPC client over a stdio session or a Streamable HTTP session.
//! Requests are answered by id; the server's pings are answered; its other requests are
//! refused (Aura declares no client capability). Standard error (for HTTP: the server's log
//! messages and HTTP errors) is kept as a short tail for the settings page.
use super::http::{HttpPlan, HttpSession};
use super::launch::StdioPlan;
use aura_backend_windows_user::program::{ProgramSession, SessionEvent, SessionSpec};
use aura_mcp::{
    InitializeResult, LineFramer, McpTool, Message, RequestId, RpcError, cancelled_params,
    encode_error, encode_notification, encode_request, encode_response, initialize_params,
    parse_initialize_result, parse_messages, parse_tools_page,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const STDERR_LINES: usize = 200;
const STDERR_LINE_CHARS: usize = 500;
const MAX_TOOL_PAGES: usize = 20;
/// How long a cancelled request may still be answered before it is settled as cancelled.
const CANCEL_GRACE: Duration = Duration::from_secs(2);

/// Why a request did not produce a result.
#[derive(Clone, Debug, PartialEq)]
pub enum RequestFailure {
    /// It was never written: the server did nothing for it.
    NotSent(String),
    /// It was written and then the server ended or the connection broke: unknown.
    Lost(String),
    TimedOut,
    Cancelled,
    /// The server answered with a JSON-RPC error.
    Rpc(RpcError),
}

impl RequestFailure {
    pub fn describe(&self) -> String {
        match self {
            Self::NotSent(reason) => format!("请求没有发出：{reason}"),
            Self::Lost(reason) => format!("服务器在回应前结束：{reason}"),
            Self::TimedOut => "服务器没有在时限内回应".into(),
            Self::Cancelled => "已取消".into(),
            Self::Rpc(error) => format!("服务器返回错误 {}：{}", error.code, error.message),
        }
    }
}

enum Reply {
    Result(Value),
    Error(RpcError),
}

struct Shared {
    session: Mutex<Option<ProgramSession>>,
    pending: Mutex<HashMap<RequestId, mpsc::SyncSender<Reply>>>,
    stderr: Mutex<VecDeque<String>>,
    ended: Mutex<Option<String>>,
    tools_changed: AtomicBool,
    next_id: AtomicI64,
    last_used: Mutex<Instant>,
    /// Set when the reader has seen the end of the session's output.
    drained: AtomicBool,
}

impl Shared {
    fn send(&self, frame: Vec<u8>) -> Result<(), String> {
        let session = self.session.lock().map_err(|_| "连接状态损坏".to_owned())?;
        session
            .as_ref()
            .ok_or_else(|| "服务器已停止".to_owned())?
            .send(frame)
            .map_err(|error| error.to_string())
    }

    fn end(&self, reason: String) {
        if let Ok(mut ended) = self.ended.lock()
            && ended.is_none()
        {
            *ended = Some(reason);
        }
        // Waiters see their channel close and settle as lost.
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
    }

    fn stderr_line(&self, line: &str) {
        if let Ok(mut tail) = self.stderr.lock() {
            if tail.len() >= STDERR_LINES {
                tail.pop_front();
            }
            tail.push_back(line.chars().take(STDERR_LINE_CHARS).collect());
        }
    }

    fn ended(&self) -> Option<String> {
        self.ended.lock().ok().and_then(|ended| ended.clone())
    }

    fn request(
        &self,
        method: &str,
        params: Option<Value>,
        timeout: Duration,
        cancel: Option<&AtomicBool>,
    ) -> Result<Value, RequestFailure> {
        if let Some(reason) = self.ended() {
            return Err(RequestFailure::NotSent(reason));
        }
        if let Ok(mut used) = self.last_used.lock() {
            *used = Instant::now();
        }
        let id = RequestId::Number(self.next_id.fetch_add(1, Ordering::AcqRel));
        let (waiter, answer) = mpsc::sync_channel(1);
        self.pending
            .lock()
            .map_err(|_| RequestFailure::NotSent("连接状态损坏".into()))?
            .insert(id.clone(), waiter);
        if let Err(reason) = self.send(encode_request(&id, method, params)) {
            self.forget(&id);
            return Err(RequestFailure::NotSent(reason));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let stop = cancel.is_some_and(|flag| flag.load(Ordering::Acquire));
            let expired = Instant::now() >= deadline;
            if stop || expired {
                let reason = if stop { "用户停止" } else { "超过时限" };
                let _ = self.send(encode_notification(
                    "notifications/cancelled",
                    Some(cancelled_params(&id, reason)),
                ));
                // A late answer within the grace is still the real outcome.
                return match answer.recv_timeout(CANCEL_GRACE) {
                    Ok(Reply::Result(value)) => Ok(value),
                    Ok(Reply::Error(error)) => Err(RequestFailure::Rpc(error)),
                    Err(_) if stop => Err(RequestFailure::Cancelled),
                    Err(_) => Err(RequestFailure::TimedOut),
                };
            }
            match answer.recv_timeout(Duration::from_millis(50).min(deadline - Instant::now())) {
                Ok(Reply::Result(value)) => return Ok(value),
                Ok(Reply::Error(error)) => return Err(RequestFailure::Rpc(error)),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(RequestFailure::Lost(
                        self.ended().unwrap_or_else(|| "连接已断开".into()),
                    ));
                }
            }
        }
    }

    fn forget(&self, id: &RequestId) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(id);
        }
    }

    /// Stops the server's whole process tree.
    fn close(&self) {
        self.end("连接已关闭".into());
        if let Ok(mut session) = self.session.lock()
            && let Some(mut running) = session.take()
        {
            running.stop();
        }
    }
}

enum Transport {
    Stdio(Arc<Shared>),
    Http(Arc<HttpSession>),
}

pub struct Connection {
    transport: Transport,
    pub(crate) initialized: InitializeResult,
}

fn route(shared: &Shared, message: Message) {
    match message {
        Message::Response { id, result } => {
            if let Some(waiter) = shared.pending.lock().ok().and_then(|mut p| p.remove(&id)) {
                let _ = waiter.try_send(Reply::Result(result));
            }
        }
        Message::Error { id: Some(id), error } => {
            if let Some(waiter) = shared.pending.lock().ok().and_then(|mut p| p.remove(&id)) {
                let _ = waiter.try_send(Reply::Error(error));
            }
        }
        Message::Error { id: None, error } => {
            shared.stderr_line(&format!("[服务器错误 {}] {}", error.code, error.message));
        }
        Message::Request { id, method, .. } => {
            let answer = if method == "ping" {
                encode_response(&id, json!({}))
            } else {
                encode_error(&id, -32601, "Aura does not offer this capability")
            };
            let _ = shared.send(answer);
        }
        Message::Notification { method, .. } => {
            if method == "notifications/tools/list_changed" {
                shared.tools_changed.store(true, Ordering::Release);
            }
        }
    }
}

fn read(shared: &Shared, events: &mpsc::Receiver<SessionEvent>) {
    let mut framer = LineFramer::new();
    let mut errors = LineFramer::new();
    for event in events {
        match event {
            SessionEvent::Stdout(bytes) => match framer.push(&bytes) {
                Err(error) => {
                    shared.end(error.to_string());
                    if let Ok(mut session) = shared.session.lock()
                        && let Some(mut running) = session.take()
                    {
                        running.stop();
                    }
                }
                Ok(lines) => {
                    for line in lines {
                        match parse_messages(&line) {
                            Ok(messages) => messages.into_iter().for_each(|m| route(shared, m)),
                            Err(error) => shared.stderr_line(&format!("[无法解析的输出] {error}")),
                        }
                    }
                }
            },
            SessionEvent::Stderr(bytes) => {
                if let Ok(lines) = errors.push(&bytes) {
                    for line in lines {
                        shared.stderr_line(&String::from_utf8_lossy(&line));
                    }
                }
            }
            SessionEvent::Ended { exit_code, memory_limit_reached, .. } => {
                let ended = match exit_code {
                    Some(code) => format!("服务器进程已退出（退出码 {code}）"),
                    None => "服务器进程已结束".into(),
                };
                shared.end(if memory_limit_reached {
                    format!("{ended}；它用到了插件设定的内存上限，可能因此退出")
                } else {
                    ended
                });
                shared.drained.store(true, Ordering::Release);
                return;
            }
        }
    }
    shared.end("服务器进程已结束".into());
    shared.drained.store(true, Ordering::Release);
}

/// Why a server did not start, with the end of what it wrote to standard error.
#[derive(Debug)]
pub(crate) struct StartFailure {
    pub reason: String,
    pub stderr: Vec<String>,
}

impl Connection {
    /// Starts the server and completes the MCP handshake within `timeout`.
    pub(crate) fn start(
        plan: StdioPlan,
        timeout: Duration,
        client_version: &str,
    ) -> Result<Self, StartFailure> {
        let refused = |reason: String| StartFailure { reason, stderr: Vec::new() };
        let (events, received) = mpsc::channel();
        let session = ProgramSession::start(
            SessionSpec {
                image: &plan.image,
                cmd_script: plan.cmd_script.as_deref(),
                arguments: &plan.arguments,
                working_directory: &plan.working_directory,
                environment: plan.environment.entries(),
                // A cargo-built server keeps its second link in `deps`.
                other_links_allowed: true,
                limits: plan.limits,
            },
            events,
        )
        .map_err(|error| refused(format!("服务器没有启动：{error}")))?;
        drop(plan);
        let shared = Arc::new(Shared {
            session: Mutex::new(Some(session)),
            pending: Mutex::new(HashMap::new()),
            stderr: Mutex::new(VecDeque::new()),
            ended: Mutex::new(None),
            tools_changed: AtomicBool::new(false),
            next_id: AtomicI64::new(1),
            last_used: Mutex::new(Instant::now()),
            drained: AtomicBool::new(false),
        });
        let reader = shared.clone();
        std::thread::Builder::new()
            .name("aura-mcp-reader".into())
            .spawn(move || read(&reader, &received))
            .map_err(|error| refused(format!("读取线程无法启动：{error}")))?;
        let mut connection = Self {
            transport: Transport::Stdio(Arc::clone(&shared)),
            initialized: InitializeResult {
                protocol_version: String::new(),
                server_info: aura_mcp::ServerInfo::default(),
                tools_supported: false,
                tools_list_changed: false,
                instructions: None,
            },
        };
        let answer = connection.request(
            "initialize",
            Some(initialize_params(client_version)),
            timeout,
            None,
        );
        let initialized = answer
            .map_err(|failure| format!("握手失败：{}", failure.describe()))
            .and_then(|answer| parse_initialize_result(&answer).map_err(|error| error.to_string()))
            .and_then(|initialized| {
                shared
                    .send(encode_notification("notifications/initialized", None))
                    .map(|()| initialized)
            });
        match initialized {
            Ok(initialized) => {
                connection.initialized = initialized;
                Ok(connection)
            }
            Err(reason) => Err(StartFailure { reason, stderr: connection.close_and_drain() }),
        }
    }

    /// Greets a Streamable HTTP server within `timeout` through Core's network gateway. Runs
    /// on a blocking thread, never inside an async task.
    pub(crate) fn start_http(
        plan: HttpPlan,
        gateway: Arc<aura_network_control::CoreHttpClientGateway>,
        runtime: tokio::runtime::Handle,
        timeout: Duration,
        client_version: &str,
    ) -> Result<Self, StartFailure> {
        let (session, initialized) =
            HttpSession::open(plan, gateway, runtime, timeout, client_version)?;
        Ok(Self { transport: Transport::Http(session), initialized })
    }

    /// Stops the server and returns the end of its error output once the reader saw it all.
    pub(crate) fn close_and_drain(&self) -> Vec<String> {
        self.close();
        if let Transport::Stdio(shared) = &self.transport {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !shared.drained.load(Ordering::Acquire) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        self.stderr_tail()
    }

    /// Sends one request and waits for its answer, the deadline, or `cancel`.
    pub fn request(
        &self,
        method: &str,
        params: Option<Value>,
        timeout: Duration,
        cancel: Option<&AtomicBool>,
    ) -> Result<Value, RequestFailure> {
        match &self.transport {
            Transport::Stdio(shared) => shared.request(method, params, timeout, cancel),
            Transport::Http(session) => session.request(method, params, timeout, cancel),
        }
    }

    /// Every page of the server's tools, in its order.
    pub(crate) fn list_tools(&self, timeout: Duration) -> Result<Vec<McpTool>, String> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_TOOL_PAGES {
            let params = cursor.as_ref().map(|cursor| json!({"cursor": cursor}));
            let page = self
                .request("tools/list", params, timeout, None)
                .map_err(|failure| format!("列出工具失败：{}", failure.describe()))?;
            let page = parse_tools_page(&page).map_err(|error| error.to_string())?;
            tools.extend(page.tools);
            match page.next_cursor {
                Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
                _ => return Ok(tools),
            }
        }
        Err(format!("工具列表超过 {MAX_TOOL_PAGES} 页"))
    }

    pub(crate) fn ended(&self) -> Option<String> {
        match &self.transport {
            Transport::Stdio(shared) => shared.ended(),
            Transport::Http(session) => session.ended(),
        }
    }

    pub fn alive(&self) -> bool {
        self.ended().is_none()
    }

    pub(crate) fn idle_for(&self) -> Duration {
        match &self.transport {
            Transport::Stdio(shared) => {
                shared.last_used.lock().map(|used| used.elapsed()).unwrap_or_default()
            }
            Transport::Http(session) => session.idle_for(),
        }
    }

    /// Whether the server said its tools changed since the last call.
    pub fn take_tools_changed(&self) -> bool {
        match &self.transport {
            Transport::Stdio(shared) => shared.tools_changed.swap(false, Ordering::AcqRel),
            Transport::Http(session) => session.take_tools_changed(),
        }
    }

    pub fn stderr_tail(&self) -> Vec<String> {
        match &self.transport {
            Transport::Stdio(shared) => {
                shared.stderr.lock().map(|tail| tail.iter().cloned().collect()).unwrap_or_default()
            }
            Transport::Http(session) => session.notes(),
        }
    }

    /// Stops the server's whole process tree, or ends the HTTP session.
    pub(crate) fn close(&self) {
        match &self.transport {
            Transport::Stdio(shared) => shared.close(),
            Transport::Http(session) => session.close(),
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close();
    }
}
