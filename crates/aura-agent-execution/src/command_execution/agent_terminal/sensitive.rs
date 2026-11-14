//! One-shot user input belongs to a live model call, never the credential store. It is typed only
//! while the terminal still shows the prompt the request was opened at.
use super::prompt::Shown;
use super::*;
use aura_core_base::outbound_json::send_notification;
use aura_ipc::QueuePriority;
use aura_runtime_contract::IpcErrorEnvelope;
use aura_windows_platform::SecretBytes;
use tokio::sync::{oneshot, watch};
use zeroize::Zeroizing;
/// The output stays quiet this long before the card opens, so it opens at a prompt the terminal
/// has finished printing; it opens anyway after `SETTLE_FOR`.
const QUIET: Duration = Duration::from_millis(300);
const SETTLE_FOR: Duration = Duration::from_secs(2);
pub(crate) const PROMPT_GONE: &str = "terminal.sensitive.prompt_gone";
pub(crate) struct SensitiveWait {
    pub timeout_ms: u64,
    pub cancellation: watch::Receiver<bool>,
}
pub(super) struct Pending {
    request: TerminalSensitiveRequestV1,
    deadline: tokio::time::Instant,
    sender: Option<oneshot::Sender<Answer>>,
    cancellation: watch::Receiver<bool>,
    /// What the terminal showed when the card opened.
    shown: Shown,
}
enum Answer {
    Status(TerminalSensitiveStatusV1),
    /// The terminal had moved on from the prompt when the user answered; nothing was typed.
    PromptGone,
}
/// The user answered after the terminal had moved on from the prompt; nothing was typed.
#[derive(Debug, thiserror::Error)]
#[error("{}: the terminal no longer shows the prompt; nothing was typed", PROMPT_GONE)]
pub struct PromptGone;
struct Cleanup {
    owner: Arc<AgentTerminals>,
    call: ToolCallId,
    session: SessionId,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.owner.sensitive.lock() {
            pending.remove(&self.call);
        }
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let owner = self.owner.clone();
            let session = self.session;
            handle.spawn(async move {
                let _ = owner.publish_sensitive(session).await;
            });
        }
    }
}
impl AgentTerminals {
    pub fn sensitive_pending(&self, session: SessionId) -> Result<TerminalSensitivePendingV1> {
        let pending =
            self.sensitive.lock().map_err(|_| anyhow::anyhow!("terminal.sensitive_lock"))?;
        Ok(TerminalSensitivePendingV1 {
            session_id: session,
            requests: pending
                .values()
                .filter(|v| {
                    v.request.session_id == session
                        && v.sender.is_some()
                        && v.deadline > tokio::time::Instant::now()
                })
                .map(|v| v.request.clone())
                .collect(),
        })
    }
    /// The conversations with a secure input waiting for the user.
    pub fn sensitive_sessions(&self) -> Vec<SessionId> {
        let now = tokio::time::Instant::now();
        let Ok(pending) = self.sensitive.lock() else { return Vec::new() };
        let mut sessions = pending
            .values()
            .filter(|v| v.sender.is_some() && v.deadline > now)
            .map(|v| v.request.session_id)
            .collect::<Vec<_>>();
        sessions.sort_unstable();
        sessions.dedup();
        sessions
    }
    async fn publish_sensitive(&self, session: SessionId) -> Result<()> {
        let queue =
            self.publisher.lock().map_err(|_| anyhow::anyhow!("terminal.publisher_lock"))?.clone();
        if let Some(queue) = queue {
            send_notification(
                &queue,
                QueuePriority::P2,
                "terminal.sensitive.v1",
                &TerminalSensitiveChangedV1 {
                    runtime_instance_id: self.runtime,
                    pending: self.sensitive_pending(session)?,
                },
            )
            .await?;
        }
        Ok(())
    }
    pub(super) fn cancel_sensitive_terminal(&self, id: &str) {
        if let Ok(mut pending) = self.sensitive.lock() {
            for entry in pending.values_mut().filter(|entry| entry.request.terminal_id == id) {
                if let Some(sender) = entry.sender.take() {
                    let _ = sender.send(Answer::Status(TerminalSensitiveStatusV1::Cancelled));
                }
            }
        }
    }
    /// The user resized the terminal to `size` when its output had reached `at`: ConPTY prints
    /// its whole screen again, which is output too.
    pub fn resized(&self, id: &str, size: aura_windows_platform::TerminalSize, at: u64) {
        if let Ok(entry) = self.entry(id)
            && let Ok(mut live) = entry.lock()
        {
            live.screen.resized();
            let live = &mut *live;
            live.lines.resize_at(at, size, &live.privacy);
        }
    }
    /// How far the terminal's output has come and the line it shows, once that output is saved.
    async fn shown(self: &Arc<Self>, session: SessionId, id: &str) -> Result<Shown> {
        let owner = self.clone();
        let id = id.to_owned();
        self.store
            .run_critical(
                StoreStep::operation("terminal.sensitive.shown"),
                StoreAccess::Write,
                move || {
                    owner.capture(session, &id)?;
                    let live = owner.entry(&id)?;
                    let live = live.lock()?;
                    Ok(live.screen.shown(live.offset))
                },
            )
            .await
    }
    /// What the terminal shows once its output has been quiet for a moment.
    async fn settle(self: &Arc<Self>, session: SessionId, id: &str) -> Result<Shown> {
        let until = tokio::time::Instant::now() + SETTLE_FOR;
        let mut shown = self.shown(session, id).await?;
        loop {
            tokio::time::sleep(QUIET).await;
            let now = self.shown(session, id).await?;
            if now.offset == shown.offset || tokio::time::Instant::now() >= until {
                return Ok(now);
            }
            shown = now;
        }
    }
    /// Whether a new secure input may be asked for at `id`: the model's own live terminal of
    /// this session, with no other request open there. The requests stay locked for the caller.
    fn sensitive_allowed(
        &self,
        session: SessionId,
        id: &str,
    ) -> Result<MutexGuard<'_, BTreeMap<ToolCallId, Pending>>> {
        let record = self.record(session, id)?;
        ensure!(record.view.controller == TerminalControllerV1::Agent, super::handback::TAKEN_OVER);
        ensure!(
            record.view.end.is_none(),
            "terminal.ended: 终端已结束，没有请求输入。请调用 terminal.open 新建终端。"
        );
        let pending =
            self.sensitive.lock().map_err(|_| anyhow::anyhow!("terminal.sensitive_lock"))?;
        ensure!(
            !pending.values().any(|v| v.request.terminal_id == id && v.sender.is_some()),
            "terminal.sensitive.pending: 请先完成当前安全输入。"
        );
        Ok(pending)
    }
    pub async fn request_sensitive(
        self: &Arc<Self>,
        call: &crate::agent_execution::CallContext,
        input: AgentTerminalSensitiveInput,
        mut wait: SensitiveWait,
    ) -> Result<TerminalSensitiveResultV1> {
        ensure!(
            call.operation.content.identity.actor
                == aura_security_contract::InvocationActor::ParentAgent,
            "terminal.sensitive.parent_only: 子 Agent 不支持安全输入，请由主 Agent 请求。"
        );
        ensure!(
            !input.prompt.trim().is_empty() && input.prompt.chars().count() <= 2000,
            "terminal.sensitive.prompt_invalid: 请给出不超过 2000 字的输入说明。"
        );
        let owner = self.clone();
        let session = call.snapshot.session_id;
        let id = call.validated.call.tool_call_id;
        let timeout = std::time::Duration::from_millis(wait.timeout_ms);
        let deadline = tokio::time::Instant::now() + timeout;
        let request = TerminalSensitiveRequestV1 {
            session_id: session,
            terminal_id: input.terminal_id,
            tool_call_id: id,
            prompt: input.prompt,
            expires_at: UtcTimestamp::from_value(
                UtcTimestamp::now().value()
                    + time::Duration::milliseconds(i64::try_from(wait.timeout_ms)?),
            ),
        };
        let checker = self.clone();
        let terminal = request.terminal_id.clone();
        self.store
            .run_critical(
                StoreStep::operation("terminal.sensitive.request"),
                StoreAccess::Read,
                move || checker.sensitive_allowed(session, &terminal).map(drop),
            )
            .await?;
        let shown = self.settle(session, &request.terminal_id).await?;
        let (sender, receiver) = oneshot::channel();
        let cancellation = wait.cancellation.clone();
        self.store
            .run_critical(
                StoreStep::operation("terminal.sensitive.request"),
                StoreAccess::Read,
                move || {
                    let mut pending = owner.sensitive_allowed(session, &request.terminal_id)?;
                    let sender = Some(sender);
                    pending.insert(id, Pending { request, deadline, sender, cancellation, shown });
                    Ok(())
                },
            )
            .await?;
        let cleanup = Cleanup { owner: self.clone(), call: id, session };
        self.publish_sensitive(session).await?;
        let answer = tokio::select! {
            result = receiver => result.unwrap_or(Answer::Status(TerminalSensitiveStatusV1::Cancelled)),
            _ = tokio::time::sleep_until(deadline) => Answer::Status(TerminalSensitiveStatusV1::Cancelled),
            _ = wait.cancellation.wait_for(|v| *v) => Answer::Status(TerminalSensitiveStatusV1::Cancelled),
        };
        drop(cleanup);
        self.publish_sensitive(session).await?;
        match answer {
            Answer::Status(status) => Ok(TerminalSensitiveResultV1 { status }),
            Answer::PromptGone => anyhow::bail!(
                "{PROMPT_GONE}: 没有输入：用户提交时，终端已不在显示请求时的提示，请求之后终端有了新的输出。可以先用 terminal.read 看终端现在的状态，需要时等提示出现后再请求一次。"
            ),
        }
    }
    pub async fn sensitive_cancel(
        self: &Arc<Self>,
        input: TerminalSensitiveReplyV1,
    ) -> Result<TerminalSensitiveResultV1> {
        {
            let mut pending =
                self.sensitive.lock().map_err(|_| anyhow::anyhow!("terminal.sensitive_lock"))?;
            let entry =
                pending.get_mut(&input.tool_call_id).context("terminal.sensitive.not_pending")?;
            ensure!(
                entry.request.session_id == input.session_id,
                "terminal.sensitive.wrong_session"
            );
            if let Some(sender) = entry.sender.take() {
                let _ = sender.send(Answer::Status(TerminalSensitiveStatusV1::Cancelled));
            }
        }
        self.publish_sensitive(input.session_id).await?;
        Ok(TerminalSensitiveResultV1 { status: TerminalSensitiveStatusV1::Cancelled })
    }
    pub async fn sensitive_submit(
        self: &Arc<Self>,
        input: TerminalSensitiveReplyV1,
        secret: SecretBytes,
    ) -> Result<TerminalSensitiveResultV1> {
        let owner = self.clone();
        let session = input.session_id;
        let result = self
            .store
            .run_critical(
                StoreStep::operation("terminal.sensitive.submit"),
                StoreAccess::Write,
                move || {
                    let text = std::str::from_utf8(secret.expose())
                        .context("terminal.sensitive.invalid_utf8")?;
                    ensure!(
                        !text.is_empty()
                            && text.len() <= aura_ipc::MAX_SECRET_INPUT_BYTES
                            && !text.chars().any(|ch| matches!(
                                ch,
                                '\u{0}'..='\u{1f}' | '\u{7f}' | '\u{2028}' | '\u{2029}'
                            )),
                        "terminal.sensitive.invalid_input"
                    );
                    let id = {
                        let pending = owner
                            .sensitive
                            .lock()
                            .map_err(|_| anyhow::anyhow!("terminal.sensitive_lock"))?;
                        let entry = pending
                            .get(&input.tool_call_id)
                            .context("terminal.sensitive.not_pending")?;
                        ensure!(
                            entry.request.session_id == session
                                && entry.deadline > tokio::time::Instant::now()
                                && entry.sender.is_some(),
                            "terminal.sensitive.expired_or_consumed"
                        );
                        entry.request.terminal_id.clone()
                    };
                    owner.capture(session, &id)?;
                    let live = owner.entry(&id)?;
                    let mut live =
                        live.lock().map_err(|_| anyhow::anyhow!("terminal.owner_lock"))?;
                    let mut pending = owner
                        .sensitive
                        .lock()
                        .map_err(|_| anyhow::anyhow!("terminal.sensitive_lock"))?;
                    let entry = pending
                        .get_mut(&input.tool_call_id)
                        .context("terminal.sensitive.not_pending")?;
                    ensure!(
                        entry.deadline > tokio::time::Instant::now()
                            && entry.sender.is_some()
                            && !*entry.cancellation.borrow(),
                        "terminal.sensitive.expired_or_consumed"
                    );
                    ensure!(
                        live.controller == TerminalControllerV1::Agent,
                        "terminal.taken_over: 用户已接管该终端，没有输入。等用户交还后再试。"
                    );
                    let terminal = live
                        .terminal
                        .clone()
                        .context("terminal.ended: 终端已结束，没有输入。请新建终端。")?;
                    if !entry.shown.still(&live.screen.shown(live.offset)) {
                        drop(secret);
                        let sender = entry.sender.take().context("terminal.sensitive.consumed")?;
                        let _ = sender.send(Answer::PromptGone);
                        return Err(PromptGone.into());
                    }
                    live.privacy.add(text)?;
                    let mut bytes = Zeroizing::new(secret.expose().to_vec());
                    bytes.push(b'\r');
                    let status = if terminal.write(&bytes).is_ok() {
                        TerminalSensitiveStatusV1::Completed
                    } else {
                        TerminalSensitiveStatusV1::Failed
                    };
                    drop(bytes);
                    drop(secret);
                    let sender = entry.sender.take().context("terminal.sensitive.consumed")?;
                    let _ = sender.send(Answer::Status(status));
                    Ok(TerminalSensitiveResultV1 { status })
                },
            )
            .await;
        self.publish_sensitive(session).await?;
        result
    }
}
/// The submit route's refusal. It says why nothing was typed and never anything about the input.
pub fn terminal_sensitive_refusal(
    correlation: CorrelationId,
    failure: Option<&anyhow::Error>,
) -> IpcErrorEnvelope {
    let (code, category, message) = if failure
        .is_some_and(|f| f.downcast_ref::<PromptGone>().is_some())
    {
        (
            PROMPT_GONE,
            ErrorCategory::Conflict,
            "终端已不在等这次输入，没有输入。请查看终端，需要时让模型再请求一次。",
        )
    } else {
        (
            "terminal.sensitive.unavailable",
            ErrorCategory::Validation,
            "安全输入未提交：请求已结束、会话不匹配或终端已被接管。请检查当前输入请求；不要自动重发输入。",
        )
    };
    IpcErrorEnvelope::failed(
        ErrorCode::new(code).expect("static code"),
        category,
        message,
        correlation,
    )
}
impl CommandExecution {
    pub async fn terminal_sensitive_submit(
        &self,
        input: TerminalSensitiveReplyV1,
        secret: SecretBytes,
    ) -> Result<TerminalSensitiveResultV1> {
        self.agent_terminals.sensitive_submit(input, secret).await
    }
    /// The conversations with a secure input waiting for the user, which waits as an approval does.
    pub fn sensitive_input_sessions(&self) -> Vec<SessionId> {
        self.agent_terminals.sensitive_sessions()
    }
}
