use super::*;
use aura_domain_common::SecurityOperationId;

impl CommandExecution {
    pub(crate) fn managed_operations(
        &self,
        session: SessionId,
    ) -> anyhow::Result<Vec<SecurityOperationId>> {
        let mut operations = self.compat.managed_operations(session)?;
        operations.extend(
            self.agent_terminals
                .list(session)?
                .into_iter()
                .filter(|record| record.view.end.is_none())
                .map(|record| record.operation_id),
        );
        operations.sort();
        operations.dedup();
        Ok(operations)
    }

    /// All stop requests are delivered within one deadline; an unknown receipt stays unknown.
    pub(crate) async fn stop_operations_until(
        self: &Arc<Self>,
        operations: Vec<SecurityOperationId>,
        deadline: std::time::Instant,
    ) -> anyhow::Result<()> {
        let mut stops = tokio::task::JoinSet::new();
        for id in operations {
            let owner = self.clone();
            stops.spawn(async move {
                match tokio::time::timeout_at(
                    tokio::time::Instant::from_std(deadline),
                    owner.stop_operation(id),
                )
                .await
                {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(format!("{id}: {error}")),
                    Err(_) => Some(format!("{id}: stop deadline elapsed")),
                }
            });
        }
        let mut unresolved = Vec::new();
        while let Some(result) = stops.join_next().await {
            match result {
                Ok(Some(reason)) => unresolved.push(reason),
                Err(error) => unresolved.push(error.to_string()),
                Ok(None) => {}
            }
        }
        unresolved.sort();
        anyhow::ensure!(unresolved.is_empty(), "agent.stop_pending: {}", unresolved.join("; "));
        Ok(())
    }
}

/// Reads immutable operation and snapshot facts, including calls whose foreground already ended.
pub(crate) fn context(
    db: &aura_persistence_core::Connection,
    id: SecurityOperationId,
    session: SessionId,
) -> anyhow::Result<(
    aura_execution_contract::SecuredOperation,
    aura_security_contract::PermissionSnapshot,
)> {
    let (batch, call) = aura_persistence_v2::tool::execution::operation_call(db, id)?
        .ok_or_else(|| anyhow::anyhow!("command.managed_binding_missing"))?;
    anyhow::ensure!(batch.invocation_identity.session_id == session, "command.session_workspace");
    let operation = if aura_persistence_v2::terminal::list(db, Some(session))?
        .iter()
        .any(|record| record.operation_id == id)
    {
        let payload: String =
            db.query_row("SELECT payload FROM operations WHERE id=?1", [id.to_string()], |row| {
                row.get(0)
            })?;
        let operation: aura_execution_contract::SecuredOperation = serde_json::from_str(&payload)?;
        anyhow::ensure!(
            operation.content.action_kind == "terminal.open",
            "terminal.managed_operation_kind"
        );
        operation
    } else {
        aura_persistence_v2::execution::shell::operation_for_call(db, session, call)?
            .ok_or_else(|| anyhow::anyhow!("command.managed_operation_missing"))?
    };
    let snapshot_id = operation
        .content
        .identity
        .permission_snapshot_id
        .ok_or_else(|| anyhow::anyhow!("command.permission_snapshot_missing"))?;
    let snapshot = aura_persistence_v2::security::snapshot(db, snapshot_id)?
        .ok_or_else(|| anyhow::anyhow!("command.permission_snapshot_missing"))?;
    Ok((operation, snapshot))
}
