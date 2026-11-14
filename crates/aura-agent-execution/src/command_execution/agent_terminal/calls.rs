//! One tool invocation has a finite wait; the terminal owns its longer Job lifetime.
use super::handback;
use super::wait::{Typed, wait};
use super::*;
use aura_backend_windows_user::compat::{TerminalKind, TerminalOptions};
use aura_execution_contract::{DispatchRequest, OperationNetwork, SideEffectStateV1};

impl CommandExecution {
    pub(crate) async fn agent_terminal_call(
        &self,
        call: &crate::agent_execution::CallContext,
        request: &DispatchRequest,
        side_effect: &mut SideEffectStateV1,
        start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
        sensitive: Option<sensitive::SensitiveWait>,
    ) -> Result<serde_json::Value> {
        let session = call.snapshot.session_id;
        let owner = self.agent_terminals.clone();
        let action = &request.operation.content.action;
        match request.operation.content.action_kind.as_str() {
            "terminal.request_sensitive_input" => {
                drop(start_lease);
                let result = owner
                    .request_sensitive(
                        call,
                        serde_json::from_value(action.clone())?,
                        sensitive.context("terminal.sensitive.policy_missing")?,
                    )
                    .await?;
                *side_effect = match result.status {
                    TerminalSensitiveStatusV1::Completed => SideEffectStateV1::FullyApplied,
                    TerminalSensitiveStatusV1::Cancelled => SideEffectStateV1::NoEffect,
                    TerminalSensitiveStatusV1::Failed => SideEffectStateV1::Unknown,
                };
                Ok(serde_json::to_value(result)?)
            }
            "terminal.open" => {
                let input: AgentTerminalOpenInput = serde_json::from_value(action.clone())?;
                *side_effect = SideEffectStateV1::Unknown;
                let entry = self
                    .compat
                    .open_terminal(
                        session,
                        TerminalOptions {
                            shell: input.shell.into(),
                            kind: TerminalKind::Agent {
                                requires_network: request.operation.content.network
                                    != OperationNetwork::None,
                            },
                            working_directory: input.working_directory.clone().into(),
                            size: aura_windows_platform::TerminalSize { columns: 80, rows: 24 },
                            output_limit: 16 * 1024 * 1024,
                            secret_environment: input.secret_environment.clone(),
                        },
                    )
                    .await?;
                *side_effect = SideEffectStateV1::FullyApplied;
                let terminal = self.compat.terminals.get(session, &entry.id)?;
                let record = AgentTerminalRecord {
                    view: TerminalViewV1 {
                        session_id: session,
                        terminal_id: entry.id.clone(),
                        shell: entry.shell,
                        process_id: entry.process_id,
                        started_at: entry.started_at,
                        end: None,
                        owner: TerminalOwnerV1::Agent,
                        controller: TerminalControllerV1::Agent,
                        title: input.title,
                        tool_call_id: Some(call.validated.call.tool_call_id),
                        revision: Revision::INITIAL,
                        handed_over: false,
                    },
                    operation_id: request.operation.operation_id,
                    runtime_instance_id: owner.runtime,
                    working_directory: input.working_directory,
                    requires_network: request.operation.content.network != OperationNetwork::None,
                    tab_closed: false,
                };
                let registered = record.clone();
                let registrar = owner.clone();
                let result = owner
                    .store
                    .run_critical(
                        StoreStep::operation("terminal.register"),
                        StoreAccess::Write,
                        move || registrar.register(registered, terminal, 0),
                    )
                    .await;
                if result.is_err() {
                    let registry = self.compat.terminals.clone();
                    let id = entry.id.clone();
                    if tokio::task::spawn_blocking(move || {
                        registry.close_and_remove(session, &id, TerminalEndReason::OutputFailed)
                    })
                    .await?
                    .is_ok()
                    {
                        *side_effect = SideEffectStateV1::Compensated;
                    }
                    result?;
                }
                drop(start_lease);
                owner.publish(&record.view).await?;
                let output =
                    wait(owner.clone(), session, entry.id.clone(), 0, 2000, None, Typed::Nothing)
                        .await?;
                Ok(serde_json::to_value(owner.on_screen(&entry.id, Some(0), output)?)?)
            }
            "terminal.write" => {
                let input: AgentTerminalWriteInput = serde_json::from_value(action.clone())?;
                ensure!(
                    input.input.len() <= 65536 && input.wait_ms <= 30000,
                    "terminal.write: input is limited to 65536 bytes and wait_ms to 30000 (this call: {} bytes, wait_ms {}); nothing was typed. Split the input into several writes or lower wait_ms.",
                    input.input.len(),
                    input.wait_ms
                );
                // An empty write to a terminal the user took over waits for its handback.
                if input.input.is_empty() && !input.interrupt {
                    let checker = owner.clone();
                    let id = input.terminal_id.clone();
                    let held = owner
                        .store
                        .run_critical(
                            StoreStep::operation("terminal.write"),
                            StoreAccess::Read,
                            move || checker.held_by_user(session, &id),
                        )
                        .await?;
                    if held {
                        drop(start_lease);
                        let latest =
                            handback::handed_back(owner, session, input.terminal_id, input.wait_ms)
                                .await?;
                        return Ok(serde_json::to_value(latest)?);
                    }
                }
                let writer = owner.clone();
                let id = input.terminal_id.clone();
                let text = input.input.replace("\r\n", "\n").replace('\n', "\r");
                let interrupt = input.interrupt;
                let typed = Typed::of(&text, interrupt);
                let started=owner.store.run_critical(StoreStep::operation("terminal.write"),StoreAccess::Write,move || {
                    let before=writer.model_read(session,&id,0,1)?;
                    ensure!(before.running,"terminal.ended: 终端已结束（可能是 Core 已停止），这次没有输入任何内容。请调用 terminal.open 新建终端，不要自动重放输入。");
                    let record=writer.record(session,&id)?;
                    let end=records::read(writer.journal.lock()?.database.connection(),&record,u64::MAX,1)?.next_offset;
                    let live=writer.entry(&id)?;
                    let live=live.lock().map_err(|_|anyhow::anyhow!("terminal.owner_lock"))?;
                    ensure!(live.controller==TerminalControllerV1::Agent,handback::TAKEN_OVER);
                    let terminal=live.terminal.as_ref().context("terminal.ended: 终端已结束，这次没有输入任何内容。请调用 terminal.open 新建终端。")?;
                    // The answer shows the screen from the line the typed text goes to.
                    let from=live.lines.last_line(&live.lines.shown(&live.privacy));
                    if interrupt {terminal.interrupt()?;}
                    if !text.is_empty() {terminal.write(text.as_bytes())?;}
                    Ok((end,from))
                }).await;
                // A refusal has no input effect. Any pipe failure may have
                // written a prefix, so it must keep Unknown rather than replay.
                let (offset, from) = match started {
                    Ok(started) => {
                        *side_effect = SideEffectStateV1::FullyApplied;
                        started
                    }
                    Err(error) => {
                        // A terminal that had ended, or that the user had taken over, was given
                        // nothing; its refusal says so by its code or in words.
                        let said = format!("{error:#}");
                        let refused = ["terminal.ended", "terminal.taken_over", "接管", "已结束"]
                            .iter()
                            .any(|mark| said.contains(mark));
                        if !refused {
                            *side_effect = SideEffectStateV1::Unknown;
                        }
                        return Err(error);
                    }
                };
                drop(start_lease);
                // The common scheduler also enforces the configured tool limit.
                let limit = input.wait_ms;
                let id = input.terminal_id;
                let output = wait(
                    owner.clone(),
                    session,
                    id.clone(),
                    offset,
                    limit,
                    input.until_text,
                    typed,
                )
                .await?;
                Ok(serde_json::to_value(owner.on_screen(&id, Some(from), output)?)?)
            }
            "terminal.read" => {
                drop(start_lease);
                let input: AgentTerminalReadInput = serde_json::from_value(action.clone())?;
                let reader = owner.clone();
                Ok(serde_json::to_value(
                    owner
                        .store
                        .run_critical(
                            StoreStep::operation("terminal.read"),
                            StoreAccess::Read,
                            move || {
                                reader.model_lines(
                                    session,
                                    &input.terminal_id,
                                    input.line,
                                    input.max_lines as usize,
                                )
                            },
                        )
                        .await?,
                )?)
            }
            "terminal.close" => {
                let input: AgentTerminalCloseInput = serde_json::from_value(action.clone())?;
                let reader = owner.clone();
                let target = input.terminal_id.clone();
                let users = owner
                    .store
                    .run_critical(
                        StoreStep::operation("terminal.close"),
                        StoreAccess::Read,
                        move || reader.users(session, &target),
                    )
                    .await?;
                ensure!(!users, super::handover::USER_OWNED);
                *side_effect = SideEffectStateV1::Unknown;
                let view = owner
                    .close(session, input.terminal_id.clone(), TerminalEndReason::AgentClosed)
                    .await?;
                *side_effect = SideEffectStateV1::FullyApplied;
                drop(start_lease);
                if view.controller == TerminalControllerV1::User {
                    return Ok(serde_json::to_value(AgentTerminalResultV1 {
                        terminal_id: input.terminal_id,
                        output: String::new(),
                        start_offset: 0,
                        next_offset: 0,
                        truncated: false,
                        running: false,
                        end: view.end,
                    })?);
                }
                let reader = owner.clone();
                let id = input.terminal_id.clone();
                let output = owner
                    .store
                    .run_critical(
                        StoreStep::operation("terminal.close.output"),
                        StoreAccess::Read,
                        move || reader.model_read(session, &id, 0, 8192),
                    )
                    .await?;
                // What the screen showed last.
                Ok(serde_json::to_value(owner.on_screen(&input.terminal_id, None, output)?)?)
            }
            _ => anyhow::bail!(
                "terminal.tool_unknown: Aura 不认识这个终端工具，没有执行。请改用 terminal.open、terminal.write、terminal.read 或 terminal.close。"
            ),
        }
    }
}
