//! One session with a Streamable HTTP server. Requests are independent POSTs; the server may
//! name a session at the handshake, which then goes with every request. A server that forgot
//! the session (404) is greeted again and the request resent once: it never processed it.
//! Anything else that fails after the request may have reached the server is unknown, never
//! resent.
use super::failure::Failure;
use crate::mcp::connection::{RequestFailure, StartFailure};
use crate::mcp::credentials::RequestCredential;
use aura_mcp::{
    InitializeResult, RequestId, RpcError, cancelled_params, encode_notification, encode_request,
    initialize_params, parse_initialize_result,
};
use aura_network_control::CoreHttpClientGateway;
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::runtime::Handle;

const NOTES: usize = 200;
const NOTE_CHARS: usize = 500;
/// How long a cancelled request may still be answered before it is settled as cancelled.
pub(super) const CANCEL_GRACE: Duration = Duration::from_secs(2);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// The trusted address and what goes with every request to it.
#[derive(Clone)]
pub(crate) struct HttpPlan {
    pub endpoint: String,
    pub headers: Vec<(String, String)>,
    /// The user's proxy, used only for https to the public internet.
    pub network_proxy: Option<aura_tool_contract::ToolNetworkProxyV1>,
    /// What the server signs in with, read again for every request.
    pub credential: Option<Arc<dyn crate::mcp::credentials::CredentialSource>>,
}

pub(super) enum Answer {
    Result(Value),
    Error(RpcError),
}

pub(crate) struct HttpSession {
    pub(super) plan: HttpPlan,
    pub(super) gateway: Arc<CoreHttpClientGateway>,
    runtime: Handle,
    client_version: String,
    pub(super) session_id: Mutex<Option<String>>,
    pub(super) protocol: Mutex<Option<String>>,
    pub(super) tools_changed: AtomicBool,
    notes: Mutex<VecDeque<String>>,
    ended: Mutex<Option<String>>,
    /// Held while greeting the server again, so two expired requests start one new session.
    greeting: Mutex<()>,
    next_id: AtomicI64,
    last_used: Mutex<Instant>,
}

pub(super) fn current<T: Clone + Default>(value: &Mutex<T>) -> T {
    value.lock().map(|value| value.clone()).unwrap_or_default()
}

fn set<T>(value: &Mutex<T>, next: T) {
    if let Ok(mut current) = value.lock() {
        *current = next;
    }
}

impl HttpSession {
    /// Greets the server within `timeout`; the session is ready when this returns. Runs on a
    /// blocking thread, never inside an async task.
    pub(crate) fn open(
        plan: HttpPlan,
        gateway: Arc<CoreHttpClientGateway>,
        runtime: Handle,
        timeout: Duration,
        client_version: &str,
    ) -> Result<(Arc<Self>, InitializeResult), StartFailure> {
        let session = Arc::new(Self {
            plan,
            gateway,
            runtime,
            client_version: client_version.to_owned(),
            session_id: Mutex::new(None),
            protocol: Mutex::new(None),
            tools_changed: AtomicBool::new(false),
            notes: Mutex::new(VecDeque::new()),
            ended: Mutex::new(None),
            greeting: Mutex::new(()),
            next_id: AtomicI64::new(1),
            last_used: Mutex::new(Instant::now()),
        });
        match session.handshake(timeout) {
            Ok(initialized) => Ok((session, initialized)),
            Err(reason) => Err(StartFailure { reason, stderr: session.notes() }),
        }
    }

    fn handshake(&self, timeout: Duration) -> Result<InitializeResult, String> {
        set(&self.session_id, None);
        set(&self.protocol, None);
        let failed =
            |failure: Failure| format!("握手失败：{}", RequestFailure::from(failure).describe());
        let id = self.id();
        let params = Some(initialize_params(&self.client_version));
        let initialized = match self.exchange(
            encode_request(&id, "initialize", params),
            Some(&id),
            timeout,
            None,
        ) {
            Ok(Some(Answer::Result(value))) => {
                parse_initialize_result(&value).map_err(|error| error.to_string())?
            }
            Ok(Some(Answer::Error(error))) => {
                return Err(format!("握手失败：服务器返回错误 {}：{}", error.code, error.message));
            }
            Ok(None) => return Err("握手失败：服务器没有给出结果".into()),
            Err(failure) => return Err(failed(failure)),
        };
        set(&self.protocol, Some(initialized.protocol_version.clone()));
        let initialized_notice = encode_notification("notifications/initialized", None);
        self.exchange(initialized_notice, None, timeout, None).map_err(failed)?;
        Ok(initialized)
    }

    /// Sends one request and waits for its answer, the deadline, or `cancel`.
    pub(crate) fn request(
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
        let mut greeted_again = false;
        loop {
            let id = self.id();
            let session = current(&self.session_id);
            let body = encode_request(&id, method, params.clone());
            match self.exchange(body, Some(&id), timeout, cancel) {
                Ok(Some(Answer::Result(value))) => return Ok(value),
                Ok(Some(Answer::Error(error))) => return Err(RequestFailure::Rpc(error)),
                Ok(None) => return Err(RequestFailure::Lost("服务器没有给出结果".into())),
                Err(Failure::Expired) if !greeted_again => {
                    greeted_again = true;
                    let _greeting = self.greeting.lock();
                    // Another request may already have started the new session.
                    if current(&self.session_id) == session {
                        self.note("[会话已过期] 重新握手后再发一次这个请求");
                        self.handshake(timeout).map_err(|reason| {
                            RequestFailure::NotSent(format!(
                                "服务器的会话已过期，重新握手失败：{reason}"
                            ))
                        })?;
                        // A new session may come with other tools.
                        self.tools_changed.store(true, Ordering::Release);
                    }
                }
                Err(failure) => return Err(failure.into()),
            }
        }
    }

    /// The credential for the next exchange. Read here, outside the runtime: a sign-in may
    /// refresh its token, which is a network request of its own.
    fn credential(&self) -> Result<Option<RequestCredential>, Failure> {
        match &self.plan.credential {
            None => Ok(None),
            Some(source) => source
                .current()
                .map(Some)
                .map_err(|reason| Failure::NotSent(format!("读不到登录凭据：{reason}"))),
        }
    }

    /// One POST. A server that refuses the credential (401) is asked once more when the
    /// credential can be renewed (a sign-in refreshes its token); otherwise the request was
    /// not processed and nothing else happens.
    fn exchange(
        &self,
        body: Vec<u8>,
        id: Option<&RequestId>,
        timeout: Duration,
        cancel: Option<&AtomicBool>,
    ) -> Result<Option<Answer>, Failure> {
        let mut renewed = false;
        loop {
            let credential = self.credential()?;
            match self.exchange_once(body.clone(), id, timeout, cancel, credential.as_ref()) {
                Err(Failure::Unauthorized)
                    if !renewed
                        && self
                            .plan
                            .credential
                            .as_ref()
                            .is_some_and(|source| source.rejected()) =>
                {
                    renewed = true;
                }
                Err(Failure::Unauthorized) => {
                    return Err(Failure::NotSent(if self.plan.credential.is_some() {
                        "服务器拒绝了登录凭据（HTTP 401），请在设置里检查密钥或重新登录".into()
                    } else {
                        "服务器要求登录（HTTP 401），请在服务器设置的“认证”里设置密钥或 OAuth 登录"
                            .into()
                    }));
                }
                outcome => return outcome,
            }
        }
    }

    /// One POST: a request (`id`) waits for its answer; a notification only for acceptance.
    fn exchange_once(
        &self,
        body: Vec<u8>,
        id: Option<&RequestId>,
        timeout: Duration,
        cancel: Option<&AtomicBool>,
        credential: Option<&RequestCredential>,
    ) -> Result<Option<Answer>, Failure> {
        let had_session = current(&self.session_id).is_some();
        self.runtime.block_on(async {
            let run = async {
                let mut response = self.send("POST", body, timeout, credential).await?;
                self.answer(&mut response, id, had_session, credential).await
            };
            tokio::pin!(run);
            let stopped = async {
                while !cancel.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            };
            let outcome = tokio::select! {
                result = &mut run => return result,
                () = tokio::time::sleep(timeout) => Failure::TimedOut,
                () = stopped => Failure::Cancelled,
            };
            if let Some(id) = id {
                let reason = if matches!(outcome, Failure::Cancelled) {
                    "用户停止"
                } else {
                    "超过时限"
                };
                let notice = encode_notification(
                    "notifications/cancelled",
                    Some(cancelled_params(id, reason)),
                );
                let _ = self.send("POST", notice, CANCEL_GRACE, credential).await;
            }
            // A late answer within the grace is still the real outcome.
            match tokio::time::timeout(CANCEL_GRACE, &mut run).await {
                Ok(Ok(Some(answer))) => Ok(Some(answer)),
                _ => Err(outcome),
            }
        })
    }

    fn id(&self) -> RequestId {
        RequestId::Number(self.next_id.fetch_add(1, Ordering::AcqRel))
    }

    pub(super) fn note(&self, line: &str) {
        if let Ok(mut notes) = self.notes.lock() {
            if notes.len() >= NOTES {
                notes.pop_front();
            }
            notes.push_back(line.chars().take(NOTE_CHARS).collect());
        }
    }

    /// What the server logged and what went wrong, for the settings page.
    pub(crate) fn notes(&self) -> Vec<String> {
        self.notes.lock().map(|notes| notes.iter().cloned().collect()).unwrap_or_default()
    }

    pub(crate) fn ended(&self) -> Option<String> {
        current(&self.ended)
    }

    pub(crate) fn idle_for(&self) -> Duration {
        self.last_used.lock().map(|used| used.elapsed()).unwrap_or_default()
    }

    pub(crate) fn take_tools_changed(&self) -> bool {
        self.tools_changed.swap(false, Ordering::AcqRel)
    }

    /// Ends the session. The server is told in the background (best effort), so closing never
    /// waits on the network; a runtime that is shutting down simply drops the notice.
    pub(crate) fn close(self: &Arc<Self>) {
        if let Ok(mut ended) = self.ended.lock() {
            if ended.is_some() {
                return;
            }
            *ended = Some("连接已关闭".into());
        }
        if current(&self.session_id).is_none() {
            return;
        }
        // Best effort and without a credential: a refresh must not run on the runtime, and a
        // server that wants one for DELETE ends the session on its own.
        let session = Arc::clone(self);
        drop(self.runtime.spawn(async move {
            let close = session.send("DELETE", Vec::new(), CLOSE_TIMEOUT, None);
            let _ = tokio::time::timeout(CLOSE_TIMEOUT, close).await;
        }));
    }
}
