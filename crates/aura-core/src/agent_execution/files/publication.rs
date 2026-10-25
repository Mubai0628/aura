use super::*;

pub(in crate::agent_execution) fn invoke(
    service: &AgentExecution,
    call: &CallContext,
) -> Result<Value> {
    let scope = super::traversal::scope(service, call)?;
    let _read_scope = scope.as_ref().map(aura_fs_tools::FileReadScope::install);
    let (outcome, receipts) = aura_fs_tools::with_file_publications(|| {
        // `/init`'s write takes the same publication receipts as an ordinary file write.
        let correlation = call.operation.content.identity.correlation_id;
        if call.operation.content.action_kind == super::super::workspace_init::ACTION {
            super::super::workspace_init::write(service, call, correlation)
        } else if super::super::command_tools::is_command_tool(&call.operation.content.action_kind)
        {
            super::super::command_tools::run(service, call, correlation)
        } else {
            super::invoke_inner(service, call)
        }
    });
    if outcome.is_ok() && !receipts.is_empty() {
        follow_publications(service, call, &receipts)?;
    }
    super::traversal::finish(call, outcome?, scope.as_ref())
}

pub(crate) fn follow_publications(
    service: &AgentExecution,
    call: &CallContext,
    receipts: &[aura_fs_tools::FilePublicationReceipt],
) -> Result<()> {
    let targets = call
        .operation
        .content
        .resources
        .iter()
        .filter_map(aura_security_contract::file_consent_target_of)
        .collect::<Vec<_>>();
    let mut journal = service.journal.lock()?;
    let identity = &call.operation.content.identity;
    let session =
        identity.session_id.ok_or_else(|| anyhow::anyhow!("agent.file_session_missing"))?;
    let result: Result<(), aura_persistence_core::PersistenceCoreError> =
        journal.database.unit_of_work(|tx| {
            for receipt in receipts {
                for before in &targets {
                    let expected = receipt
                        .previous_identity
                        .as_ref()
                        .map(aura_security_contract::file_object_key)
                        .unwrap_or_else(|| receipt.entry_key.clone());
                    if before.identity_key == expected && before.entry_key == receipt.entry_key {
                        aura_persistence_v2::file_consents::published(
                            tx,
                            session,
                            identity.principal_id,
                            before,
                            &aura_security_contract::file_object_key(&receipt.published_identity),
                            &receipt.entry_key,
                            UtcTimestamp::now(),
                        )?;
                    }
                }
            }
            Ok(())
        });
    if let Err(error) = result {
        return Err(aura_fs_tools::FsToolError::ReconciliationRequired(format!(
            "file was published but consent receipt could not be committed: {error}"
        ))
        .into());
    }

    Ok(())
}
