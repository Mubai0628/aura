use super::*;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) async fn fixture(mode: PermissionMode, names: &[&str]) -> Fixture {
    let release = aura_tool_registry::agent_release::release().expect("published builtin release");
    for name in names {
        assert!(release.entry(name).is_some(), "S07 requires the registered {name} tool");
    }
    Fixture::with_exposure(mode, names).await
}

pub(super) fn approve(
    f: &Fixture,
    call: &CallContext,
    decision: ApprovalDecisionKind,
) -> AuthorizationGrant {
    let engine = f.service().agent_execution_required();
    let backend = broker::descriptor(engine.runtime);
    let policy = call.policy(UtcTimestamp::now(), engine.runtime);
    let PolicyAuthorization::Ask { proposal } =
        aura_permission_engine::policy::authorize(&policy, &backend, None)
    else {
        panic!("S07 needs a pre-dispatch approval for missing network/file capabilities");
    };
    let now = UtcTimestamp::now();
    let request = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: now,
        expires_at: UtcTimestamp::from_value(now.value() + time::Duration::minutes(10)),
    };
    f.service()
        .broker
        .lock()
        .expect("approval journal")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::security::insert_approval(tx, &request)?;
            aura_persistence_v2::security_approval::resolve(
                tx,
                request.approval_id,
                request.revision,
                &call.operation,
                decision,
                backend.class,
                call.snapshot.session_permission_revision,
                engine.runtime,
                now,
            )
        })
        .expect("persist network approval")
        .expect("an allow decision issues a grant")
}

pub(super) fn no_attempt(f: &Fixture, id: ToolCallId) {
    let store = f.service().broker.lock().expect("inspect durable call");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("call exists");
    assert!(call.attempt_id.is_none(), "permission/argument refusal must precede Attempt");
}

pub(super) struct Server {
    pub url: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    pub async fn start() -> Self {
        Self::start_at("127.0.0.1").await
    }
    pub async fn start_at(address: &str) -> Self {
        let listener = tokio::net::TcpListener::bind(format!("{address}:0"))
            .await
            .expect("isolated local HTTP listener");
        let url = format!("http://{}", listener.local_addr().expect("bound address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            let mut handlers = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    socket = listener.accept() => {
                        let Ok((mut stream, _)) = socket else { break };
                        let observed = Arc::clone(&observed);
                        handlers.spawn(async move {
                            let mut input = Vec::new();
                            let mut block = [0u8; 8192];
                            loop {
                                let Ok(n) = stream.read(&mut block).await else { return };
                                if n == 0 { return }
                                input.extend_from_slice(&block[..n]);
                                if input.windows(4).any(|v| v == b"\r\n\r\n") { break }
                                if input.len() > 1024 * 1024 { return }
                            }
                            let header_end = input.windows(4).position(|v|v==b"\r\n\r\n").expect("header delimiter")+4;
                            let body_length = String::from_utf8_lossy(&input[..header_end]).lines().find_map(|line|line.to_ascii_lowercase().strip_prefix("content-length:").and_then(|value|value.trim().parse::<usize>().ok())).unwrap_or(0);
                            while input.len() < header_end+body_length {
                                let Ok(n)=stream.read(&mut block).await else {return};
                                if n==0 {return;} input.extend_from_slice(&block[..n]);
                            }
                            let text = String::from_utf8_lossy(&input).into_owned();
                            let path = text.split_whitespace().nth(1).unwrap_or("/").to_owned();
                            observed.lock().expect("record local request").push(text.clone());
                            if path == "/disconnect" { return }
                            if path == "/timeout" {
                                tokio::time::sleep(std::time::Duration::from_secs(45)).await;
                            }
                            if path == "/page" || path == "/echo" {
                                let body = if path=="/page" {"<html><body>S07 cached page body for local read.</body></html>"} else {text.as_str()};
                                let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body);
                                let _=stream.write_all(response.as_bytes()).await;return;
                            }
                            if path == "/slow-body" {
                                let _=stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 8\r\nConnection: close\r\n\r\npart").await;
                                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                                let _=stream.write_all(b"done").await;return;
                            }
                            if let Some(target) = path.strip_prefix("/redirect/") {
                                let response = format!("HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                                let _ = stream.write_all(response.as_bytes()).await;
                                return;
                            }
                            let bytes = path.strip_prefix("/binary/").and_then(|n| n.parse::<usize>().ok());
                            if let Some(bytes) = bytes {
                                let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {bytes}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n");
                                if stream.write_all(header.as_bytes()).await.is_err() { return }
                                let block = [b'Z'; 16 * 1024];
                                let mut remaining = bytes;
                                while remaining > 0 {
                                    let n = remaining.min(block.len());
                                    if stream.write_all(&block[..n]).await.is_err() { return }
                                    remaining -= n;
                                }
                            } else {
                                if path == "/slow" { tokio::time::sleep(std::time::Duration::from_secs(20)).await; }
                                let body = b"{\"marker\":\"s07-local\"}";
                                let header = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                                let _ = stream.write_all(header.as_bytes()).await;
                                let _ = stream.write_all(body).await;
                            }
                        });
                    }
                    _ = handlers.join_next(), if !handlers.is_empty() => {}
                }
            }
        });
        Self { url, requests, task }
    }
    pub fn count(&self) -> usize {
        self.requests.lock().expect("local service observations").len()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
