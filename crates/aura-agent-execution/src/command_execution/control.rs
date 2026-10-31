use super::*;

impl CommandExecution {
    pub async fn stop_call(
        &self,
        session: SessionId,
        call: aura_domain_common::ToolCallId,
    ) -> anyhow::Result<()> {
        let operation = self.persistence.with_session_permission_authority(
            session,
            |_| -> anyhow::Result<_> {
                Ok(self.journal.lock()?.database.unit_of_work(|tx| {
                    aura_persistence_v2::execution::shell::request_stop(
                        tx,
                        session,
                        call,
                        aura_domain_common::UtcTimestamp::now(),
                    )
                })?)
            },
        )??;
        if let Some(operation) = operation {
            self.stop_operation(operation).await?;
        }
        Ok(())
    }
    pub async fn stop_operation(
        &self,
        id: aura_domain_common::SecurityOperationId,
    ) -> anyhow::Result<()> {
        let records =
            aura_persistence_v2::terminal::list(self.journal.lock()?.database.connection(), None)?;
        if let Some(record) = records.into_iter().find(|record| record.operation_id == id) {
            self.agent_terminals
                .close(
                    record.view.session_id,
                    record.view.terminal_id,
                    aura_windows_user_contract::TerminalEndReason::UserClosed,
                )
                .await?;
        } else {
            self.compat.stop_operation(id).await?;
        }
        Ok(())
    }
    pub fn shutdown(&self) {
        if let Err(error) = self.compat.shutdown() {
            eprintln!("compat execution cleanup requires recovery: {error}");
        }
        if let Err(error) = self.agent_terminals.flush_shutdown() {
            eprintln!("terminal final output requires recovery: {error}");
        }
    }
}
