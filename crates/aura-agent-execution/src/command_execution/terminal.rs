//! Desktop identity is not model authority. User terminal data never enters Agent history.
use super::*;
use anyhow::{Context, Result, ensure};
use aura_backend_windows_user::compat::{TerminalKind, TerminalSession, TerminalSummary};
use aura_core_base::outbound_json::send_notification;
use aura_ipc::{AsyncPriorityQueue, JsonRpcRequest, QueuePriority};
use aura_windows_platform::TerminalSize;
use aura_windows_user_contract::*;
use base64::Engine;

pub(super) fn view(entry: &TerminalSummary, terminal: &TerminalSession) -> Result<TerminalViewV1> {
    Ok(TerminalViewV1 {
        session_id: entry.session,
        terminal_id: entry.id.clone(),
        shell: entry.shell,
        process_id: entry.process_id,
        started_at: entry.started_at,
        end: terminal.end()?,
        owner: TerminalOwnerV1::User,
        controller: TerminalControllerV1::User,
        title: String::new(),
        tool_call_id: None,
        revision: aura_domain_common::Revision::INITIAL,
    })
}
fn batch(
    runtime: RuntimeInstanceId,
    session: SessionId,
    id: &str,
    terminal: &TerminalSession,
    offset: u64,
) -> Result<TerminalOutputBatchV1> {
    let page = terminal.read(offset, 65_536)?;
    // End is published only when the final retained output has been delivered.
    let end =
        if terminal.read(page.next_offset, 1)?.bytes.is_empty() { terminal.end()? } else { None };
    Ok(TerminalOutputBatchV1 {
        runtime_instance_id: runtime,
        session_id: session,
        terminal_id: id.into(),
        start_offset: page.start_offset,
        next_offset: page.next_offset,
        data_base64: base64::engine::general_purpose::STANDARD.encode(&page.bytes),
        truncated: page.truncated,
        end,
    })
}
impl CommandExecution {
    pub async fn terminal_request(
        &self,
        runtime: RuntimeInstanceId,
        queue: AsyncPriorityQueue,
        request: &JsonRpcRequest,
    ) -> Result<serde_json::Value> {
        if request.method == "terminal.user.write" {
            let input: UserTerminalWriteRequestV1 = serde_json::from_value(request.params.clone())?;
            ensure!(input.input.len() <= 65_536, "terminal.input_too_large");
        }
        let session: SessionId = serde_json::from_value(
            request.params.get("session_id").cloned().context("terminal.session_required")?,
        )?;
        let projection = self.persistence.workspace_session_projection()?;
        projection
            .sessions
            .iter()
            .find(|v| v.session_id == session)
            .context("terminal.session_unknown")?;
        let registry = Arc::clone(&self.compat.terminals);
        if request.method == "terminal.user.sensitive.pending" {
            let _: ManagedProcessRequestV1 = serde_json::from_value(request.params.clone())?;
            return Ok(serde_json::to_value(self.agent_terminals.sensitive_pending(session)?)?);
        }
        if request.method == "terminal.user.sensitive.cancel" {
            return Ok(serde_json::to_value(
                self.agent_terminals
                    .sensitive_cancel(serde_json::from_value(request.params.clone())?)
                    .await?,
            )?);
        }
        if request.method == "terminal.user.list" {
            let input: TerminalSessionRequestV1 = serde_json::from_value(request.params.clone())?;
            let mut terminals = registry
                .list(session)?
                .into_iter()
                .filter(|v| v.kind == TerminalKind::User)
                .map(|v| view(&v, registry.get(session, &v.id)?.as_ref()))
                .collect::<Result<Vec<_>>>()?;
            terminals.extend(
                self.agent_terminals
                    .list(session)?
                    .into_iter()
                    .filter(|record| !record.tab_closed)
                    .map(|record| record.view),
            );
            terminals.sort_by(|a, b| a.terminal_id.cmp(&b.terminal_id));
            terminals
                .retain(|v| input.cursor.as_ref().is_none_or(|cursor| v.terminal_id > *cursor));
            let has_more = terminals.len() > 16;
            terminals.truncate(16);
            let next_cursor = if has_more {
                terminals.last().map(|entry| entry.terminal_id.clone())
            } else {
                None
            };
            return Ok(serde_json::to_value(TerminalListV1 { terminals, next_cursor })?);
        }
        let id = request.params["terminal_id"]
            .as_str()
            .context("terminal.identity_required")?
            .to_owned();
        if request.method == "terminal.user.control" {
            let input: TerminalControlRequestV1 = serde_json::from_value(request.params.clone())?;
            return Ok(serde_json::to_value(self.agent_terminals.control(input).await?)?);
        }
        // A key for an Agent terminal of this Core goes straight to it: reading the record here
        // waited for every output save, and a person typing fell behind.
        if request.method == "terminal.user.write" && self.agent_terminals.is_live(&id)? {
            let input: UserTerminalWriteRequestV1 = serde_json::from_value(request.params.clone())?;
            let owner = self.agent_terminals.clone();
            let view = tokio::task::spawn_blocking(move || {
                owner.user_write(session, &id, &input.input, input.interrupt)
            })
            .await??;
            return Ok(serde_json::to_value(view)?);
        }
        if let Ok(record) = self.agent_terminals.record(session, &id) {
            if request.method == "terminal.user.close" {
                let mut closed = self
                    .agent_terminals
                    .close(session, id.clone(), TerminalEndReason::UserClosed)
                    .await?;
                let mut record = self.agent_terminals.record(session, &id)?;
                record.tab_closed = true;
                self.journal
                    .lock()?
                    .database
                    .unit_of_work(|tx| aura_persistence_v2::terminal::save(tx, &record))?;
                closed.end = record.view.end;
                return Ok(serde_json::to_value(closed)?);
            }
            if request.method == "terminal.user.write" {
                let input: UserTerminalWriteRequestV1 =
                    serde_json::from_value(request.params.clone())?;
                // Only a terminal of an earlier Core reaches here; it ended with that Core.
                let owner = self.agent_terminals.clone();
                let target = id.clone();
                let view = tokio::task::spawn_blocking(move || {
                    owner.user_write(session, &target, &input.input, input.interrupt)
                })
                .await??;
                return Ok(serde_json::to_value(view)?);
            }
            if request.method == "terminal.user.subscribe" && record.view.end.is_some() {
                let input: TerminalSubscribeRequestV1 =
                    serde_json::from_value(request.params.clone())?;
                let ack = TerminalOutputBatchV1 {
                    runtime_instance_id: runtime,
                    session_id: session,
                    terminal_id: id.clone(),
                    start_offset: input.offset,
                    next_offset: input.offset,
                    data_base64: String::new(),
                    truncated: false,
                    end: None,
                };
                let owner = self.agent_terminals.clone();
                let worker_id = id.clone();
                let worker = tokio::spawn(async move {
                    let mut offset = input.offset;
                    loop {
                        let record = record.clone();
                        let result = owner.output_page(record, offset).await;
                        let Ok(output) = result else {
                            break;
                        };
                        let payload = TerminalOutputBatchV1 {
                            runtime_instance_id: runtime,
                            session_id: session,
                            terminal_id: worker_id.clone(),
                            start_offset: output.start_offset,
                            next_offset: output.next_offset,
                            data_base64: base64::engine::general_purpose::STANDARD
                                .encode(output.output.as_bytes()),
                            truncated: offset < output.start_offset,
                            end: if output.truncated { None } else { output.end },
                        };
                        offset = payload.next_offset;
                        let ended = payload.end.is_some();
                        if send_notification(
                            &queue,
                            QueuePriority::P2,
                            "terminal.output.v1",
                            &payload,
                        )
                        .await
                        .is_err()
                            || ended
                        {
                            break;
                        }
                    }
                });
                let mut tasks = self
                    .terminal_outputs
                    .lock()
                    .map_err(|_| anyhow::anyhow!("terminal.subscription_lock"))?;
                if let Some(previous) = tasks.insert(id, worker) {
                    previous.abort();
                }
                return Ok(serde_json::to_value(ack)?);
            }
            ensure!(record.view.end.is_none(), "终端已结束，请新建终端。");
        }
        let entry = registry
            .list(session)?
            .into_iter()
            .find(|v| v.id == id)
            .context("terminal.not_found_in_session")?;
        let terminal = registry.get(session, &id)?;
        match request.method.as_str() {
            "terminal.user.write" => {
                let input: UserTerminalWriteRequestV1 =
                    serde_json::from_value(request.params.clone())?;
                tokio::task::spawn_blocking(move || -> Result<()> {
                    if input.interrupt {
                        terminal.interrupt()?;
                    }
                    if !input.input.is_empty() {
                        terminal.write(input.input.as_bytes())?;
                    }
                    Ok(())
                })
                .await??;
            }
            "terminal.user.resize" => {
                let input: TerminalResizeRequestV1 =
                    serde_json::from_value(request.params.clone())?;
                tokio::task::spawn_blocking(move || {
                    terminal.resize(TerminalSize { columns: input.columns, rows: input.rows })
                })
                .await??;
            }
            "terminal.user.close" => {
                let _: TerminalRequestV1 = serde_json::from_value(request.params.clone())?;
                if let Some(task) = self
                    .terminal_outputs
                    .lock()
                    .map_err(|_| anyhow::anyhow!("terminal.subscription_lock"))?
                    .remove(&id)
                {
                    task.abort();
                }
                let end = tokio::task::spawn_blocking(move || {
                    registry.close_and_remove(session, &id, TerminalEndReason::UserClosed)
                })
                .await??;
                let mut closed = view(&entry, &terminal)?;
                closed.end = Some(end);
                return Ok(serde_json::to_value(closed)?);
            }
            "terminal.user.subscribe" => {
                let input: TerminalSubscribeRequestV1 =
                    serde_json::from_value(request.params.clone())?;
                let mut changed = terminal.subscribe_output();
                let mut first = batch(runtime, session, &id, &terminal, input.offset)?;
                // P0 replies have a 32 KiB emergency bound. All output travels on P2.
                first.data_base64.clear();
                first.start_offset = input.offset;
                first.next_offset = input.offset;
                first.truncated = false;
                first.end = None;
                // A new connection/subscription replaces its predecessor, preventing duplicate streams.
                let mut tasks = self
                    .terminal_outputs
                    .lock()
                    .map_err(|_| anyhow::anyhow!("terminal.subscription_lock"))?;
                if let Some(previous) = tasks.remove(&id) {
                    previous.abort();
                }
                let worker_id = id.clone();
                let mut offset = input.offset;
                tasks.insert(
                    id,
                    tokio::spawn(async move {
                        loop {
                            let Ok(output) = batch(runtime, session, &worker_id, &terminal, offset)
                            else {
                                break;
                            };
                            let ended = output.end.is_some();
                            if !output.data_base64.is_empty() || ended {
                                offset = output.next_offset;
                                if send_notification(
                                    &queue,
                                    QueuePriority::P2,
                                    "terminal.output.v1",
                                    &output,
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                                if ended {
                                    break;
                                }
                            } else if changed.changed().await.is_err() {
                                break;
                            }
                        }
                    }),
                );
                return Ok(serde_json::to_value(first)?);
            }
            _ => anyhow::bail!("terminal.route_unknown"),
        }
        Ok(serde_json::to_value(if entry.kind == TerminalKind::User {
            view(&entry, registry.get(session, &entry.id)?.as_ref())?
        } else {
            self.agent_terminals.record(session, &entry.id)?.view
        })?)
    }
    pub async fn close_removed_terminal_sessions(
        &self,
        live: &[aura_workspace_domain::SessionSummary],
    ) {
        if let Err(error) = self.agent_terminals.remove_absent_sessions(live).await {
            eprintln!("terminal session record cleanup unconfirmed: {error}");
        }
        if let Ok(sessions) = self.compat.terminals.sessions() {
            for session in sessions {
                if !live.iter().any(|v| v.session_id == session) {
                    if let Ok(entries) = self.compat.terminals.list(session) {
                        for entry in entries {
                            if let Ok(mut outputs) = self.terminal_outputs.lock()
                                && let Some(task) = outputs.remove(&entry.id)
                            {
                                task.abort();
                            }
                        }
                    }
                    let registry = Arc::clone(&self.compat.terminals);
                    match tokio::task::spawn_blocking(move || registry.close_session(session)).await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            eprintln!("terminal session cleanup unconfirmed: {error}")
                        }
                        Err(error) => eprintln!("terminal session cleanup worker failed: {error}"),
                    }
                }
            }
        }
    }
}
