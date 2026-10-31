use super::*;
use aura_core_broker::agent_store;
use aura_windows_user_contract::*;
impl CommandExecution {
    pub async fn process_list(
        self: &Arc<Self>,
        session: SessionId,
    ) -> anyhow::Result<ManagedProcessListV1> {
        let owner = self.clone();
        self.agent_terminals
            .store
            .run_critical(
                agent_store::StoreStep::operation("process.list"),
                agent_store::StoreAccess::Read,
                move || {
                    let mut processes = aura_persistence_v2::execution::process::running_shells(
                        owner.journal.lock()?.database.connection(),
                        session,
                        &owner.compat.managed_operations(session)?,
                    )?;
                    for record in owner.agent_terminals.list(session)? {
                        if record.view.end.is_none()
                            && owner
                                .compat
                                .terminals
                                .get(session, &record.view.terminal_id)?
                                .end()?
                                .is_none()
                        {
                            processes.push(ManagedProcessViewV1 {
                                kind: ManagedProcessKindV1::Terminal,
                                command: format!(
                                    "{}{} · {}",
                                    if record.view.shell == ManagedShell::Cmd {
                                        "cmd.exe"
                                    } else {
                                        "pwsh.exe"
                                    },
                                    if record.view.title.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" ({})", record.view.title)
                                    },
                                    record.working_directory
                                ),
                                started_at: record.view.started_at,
                                tool_call_id: record
                                    .view
                                    .tool_call_id
                                    .ok_or_else(|| anyhow::anyhow!("terminal.call_missing"))?,
                                terminal_id: Some(record.view.terminal_id),
                            });
                        }
                    }
                    processes.sort_by_key(|v| v.started_at);
                    Ok(ManagedProcessListV1 { processes })
                },
            )
            .await
    }
    pub async fn process_stop(
        self: &Arc<Self>,
        input: ManagedProcessStopRequestV1,
    ) -> anyhow::Result<ManagedProcessStopResultV1> {
        let owner = self.clone();
        let session = input.session_id;
        let call = input.tool_call_id;
        let terminal = self
            .agent_terminals
            .store
            .run_critical(
                agent_store::StoreStep::operation("process.stop.owner"),
                agent_store::StoreAccess::Read,
                move || {
                    Ok(owner
                        .agent_terminals
                        .list(session)?
                        .into_iter()
                        .find(|v| v.view.tool_call_id == Some(call)))
                },
            )
            .await?;
        if let Some(record) = terminal {
            let view = self
                .agent_terminals
                .close(session, record.view.terminal_id, TerminalEndReason::UserClosed)
                .await?;
            return Ok(ManagedProcessStopResultV1 {
                stopped: view.end.is_some_and(|v| v.stop_confirmed),
            });
        }
        self.stop_call(session, call).await?;
        let owner = self.clone();
        let stopped = self
            .agent_terminals
            .store
            .run_critical(
                agent_store::StoreStep::operation("process.stop.confirm"),
                agent_store::StoreAccess::Read,
                move || {
                    let view = aura_persistence_v2::execution::shell::query(
                        owner.journal.lock()?.database.connection(),
                        session,
                        &ShellQueryInput {
                            tool_call_id: call,
                            stdout_offset: 0,
                            stderr_offset: 0,
                            max_bytes: 1,
                        },
                    )?;
                    Ok(view.background == Some(BackgroundState::Interrupted)
                        || view.background == Some(BackgroundState::Exited))
                },
            )
            .await?;
        Ok(ManagedProcessStopResultV1 { stopped })
    }
}
