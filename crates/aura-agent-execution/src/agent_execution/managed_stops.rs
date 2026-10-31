//! Selective cancellation never sets the owning run's cancellation bit.
use super::*;
use std::collections::BTreeSet;

impl AgentExecution {
    pub async fn stop_authority_dependencies(
        self: &Arc<Self>,
        session: SessionId,
        before: Option<PermissionMode>,
        commands: Option<Arc<crate::command_execution::CommandExecution>>,
        deadline: std::time::Instant,
    ) -> anyhow::Result<()> {
        tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.stop_authority_dependencies_inner(session, before, commands, deadline),
        ).await.map_err(|_| anyhow::anyhow!(
            "agent.stop_pending: authority lookup or managed execution did not finish before the deadline"
        ))?
    }

    async fn stop_authority_dependencies_inner(
        self: &Arc<Self>,
        session: SessionId,
        before: Option<PermissionMode>,
        commands: Option<Arc<crate::command_execution::CommandExecution>>,
        deadline: std::time::Instant,
    ) -> anyhow::Result<()> {
        let state = self.persistence.session_permission_state(session)?;
        if before.is_some_and(|old| {
            super::permission_changes::rank(state.mode) >= super::permission_changes::rank(old)
        }) {
            return Ok(());
        }
        let mut affected = Vec::new();
        let mut operations = BTreeSet::new();
        for entry in self.active_calls.session(session)? {
            if before.is_some_and(|old| {
                !super::permission_changes::lost_default(&entry.call.operation, old, state.mode)
            }) {
                continue;
            }
            let service = self.clone();
            let call = entry.call.clone();
            let mode = state.mode;
            let revision = state.revision;
            let covered = self
                .section(
                    StoreStep::operation("agent.permission_change.authority"),
                    StoreAccess::Read,
                    move || {
                        service.operation_permitted_now(
                            &call.operation,
                            &call.snapshot,
                            mode,
                            revision,
                        )
                    },
                )
                .await?;
            if !covered {
                // File and network workers confirm their own lifetime below.
                // Only built-in Shell operations belong to the Job controller.
                let content = &entry.call.operation.content;
                if aura_security_contract::built_in_shell(&content.source, &content.action_kind) {
                    operations.insert(entry.call.operation.operation_id);
                }
                entry.cancel.send_replace(true);
                affected.push(entry);
            }
        }
        if let Some(commands) = &commands {
            for id in commands.managed_operations(session)? {
                let service = self.clone();
                let mode = state.mode;
                let revision = state.revision;
                let covered = self
                    .section(
                        StoreStep::operation("agent.managed_permission.authority"),
                        StoreAccess::Read,
                        move || {
                            let journal = service.journal.lock()?;
                            let (operation, snapshot) =
                                crate::command_execution::managed_operations::context(
                                    journal.database.connection(),
                                    id,
                                    session,
                                )?;
                            drop(journal);
                            if before.is_some_and(|old| {
                                !super::permission_changes::lost_default(&operation, old, mode)
                            }) {
                                return Ok(true);
                            }
                            service.operation_permitted_now(&operation, &snapshot, mode, revision)
                        },
                    )
                    .await?;
                if !covered {
                    operations.insert(id);
                }
            }
        }
        let stop_result = match commands {
            Some(commands) => {
                commands.stop_operations_until(operations.into_iter().collect(), deadline).await
            }
            None => Ok(()),
        };
        let mut unresolved = Vec::new();
        for mut entry in affected {
            while !*entry.ended.borrow() {
                match tokio::time::timeout_at(
                    tokio::time::Instant::from_std(deadline),
                    entry.ended.changed(),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    _ => {
                        unresolved.push(entry.call.validated.call.tool_call_id.to_string());
                        break;
                    }
                }
            }
        }
        stop_result?;
        anyhow::ensure!(
            unresolved.is_empty(),
            "agent.stop_pending: tools {} did not confirm their end",
            unresolved.join(", ")
        );
        Ok(())
    }
}
