//! One-shot user input belongs to a live model call, never the credential store.
use super::*;
use aura_windows_platform::SecretBytes;
use tokio::sync::{oneshot, watch};
use zeroize::Zeroizing;
pub(crate) struct SensitiveWait {
    pub timeout_ms: u64,
    pub cancellation: watch::Receiver<bool>,
}
pub(super) struct Pending {
    request: TerminalSensitiveRequestV1,
    deadline: tokio::time::Instant,
    sender: Option<oneshot::Sender<TerminalSensitiveStatusV1>>,
    cancellation: watch::Receiver<bool>,
}
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
    async fn publish_sensitive(&self, session: SessionId) -> Result<()> {
        let queue =
            self.publisher.lock().map_err(|_| anyhow::anyhow!("terminal.publisher_lock"))?.clone();
        if let Some(queue) = queue {
            crate::send_notification(
                &queue,
                crate::QueuePriority::P2,
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
                    let _ = sender.send(TerminalSensitiveStatusV1::Cancelled);
                }
            }
        }
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
        let (sender, receiver) = oneshot::channel();
        let cancellation = wait.cancellation.clone();
        self.store
            .run_critical(
                StoreStep::operation("terminal.sensitive.request"),
                StoreAccess::Read,
                move || {
                    let record = owner.record(session, &request.terminal_id)?;
                    ensure!(
                        record.view.controller == TerminalControllerV1::Agent,
                        super::handback::TAKEN_OVER
                    );
                    ensure!(record.view.end.is_none(), "终端已结束，请新建终端。");
                    let mut pending = owner
                        .sensitive
                        .lock()
                        .map_err(|_| anyhow::anyhow!("terminal.sensitive_lock"))?;
                    ensure!(
                        !pending
                            .values()
                            .any(|v| v.request.terminal_id == request.terminal_id
                                && v.sender.is_some()),
                        "terminal.sensitive.pending: 请先完成当前安全输入。"
                    );
                    pending.insert(
                        id,
                        Pending { request, deadline, sender: Some(sender), cancellation },
                    );
                    Ok(())
                },
            )
            .await?;
        let cleanup = Cleanup { owner: self.clone(), call: id, session };
        self.publish_sensitive(session).await?;
        let status = tokio::select! {
            result = receiver => result.unwrap_or(TerminalSensitiveStatusV1::Cancelled),
            _ = tokio::time::sleep_until(deadline) => TerminalSensitiveStatusV1::Cancelled,
            _ = wait.cancellation.wait_for(|v| *v) => TerminalSensitiveStatusV1::Cancelled,
        };
        drop(cleanup);
        self.publish_sensitive(session).await?;
        Ok(TerminalSensitiveResultV1 { status })
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
                let _ = sender.send(TerminalSensitiveStatusV1::Cancelled);
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
                        "用户已接管该终端，等用户交还后再试。"
                    );
                    let terminal = live.terminal.clone().context("终端已结束，请新建终端。")?;
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
                    let _ = sender.send(status);
                    Ok(TerminalSensitiveResultV1 { status })
                },
            )
            .await;
        self.publish_sensitive(session).await?;
        result
    }
}
impl CommandExecution {
    pub(crate) async fn terminal_sensitive_submit(
        &self,
        input: TerminalSensitiveReplyV1,
        secret: SecretBytes,
    ) -> Result<TerminalSensitiveResultV1> {
        self.agent_terminals.sensitive_submit(input, secret).await
    }
}
