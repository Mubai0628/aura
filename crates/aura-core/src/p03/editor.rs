//! Main editor and lifecycle saves publish native receipts into the existing authority store.
use super::*;

impl P03Services {
    pub(super) fn save_editor_buffer(
        &self,
        request: BufferSaveCommand,
        correlation: CorrelationId,
    ) -> Result<Value, IpcErrorEnvelope> {
        let outcome = self.workspace.save_buffer_with_publications(request);
        self.settle_editor_publications(outcome, correlation)
    }

    pub(super) fn resolve_editor_lifecycle(
        &self,
        request: ResolveWorkspaceLifecycleInput,
        correlation: CorrelationId,
    ) -> Result<Value, IpcErrorEnvelope> {
        let outcome = self.workspace.resolve_lifecycle_with_publications(request);
        self.settle_editor_publications(outcome, correlation)
    }

    fn settle_editor_publications<T: serde::Serialize>(
        &self,
        outcome: aura_workspace::WorkspacePublicationOutcome<T>,
        correlation: CorrelationId,
    ) -> Result<Value, IpcErrorEnvelope> {
        let mut approval_follow_up = Vec::new();
        if !outcome.publications.is_empty() {
            let persisted = (|| -> anyhow::Result<()> {
                let mut journal = self.journal.lock()?;
                journal.database.unit_of_work(|tx| {
                    for receipt in &outcome.publications {
                        aura_persistence_v2::file_consents::published_in_workspace(
                            tx,
                            receipt.workspace_id,
                            &aura_security_contract::file_object_key(&receipt.previous_identity),
                            &aura_security_contract::file_object_key(&receipt.published_identity),
                            &aura_security_contract::file_entry_key(
                                &receipt.parent_identity,
                                &receipt.native_name_key,
                            ),
                            aura_domain_common::UtcTimestamp::now(),
                        )?;
                    }
                    Ok::<_, aura_persistence_core::PersistenceCoreError>(())
                })?;
                Ok(())
            })();
            if persisted.is_err() {
                approval_follow_up = outcome
                    .publications
                    .iter()
                    .map(|receipt| aura_workspace_domain::EditorApprovalFollowUp {
                        path: receipt.path.clone(),
                        requires_reapproval: true,
                        message: "文件已保存，下次使用该文件需要重新批准。".into(),
                    })
                    .collect();
                crate::append_core_log(
                    "editor_approval_follow_up_failed",
                    serde_json::json!({"correlation_id":correlation,"saved":true,"requires_reapproval":true}),
                );
            }
        }
        let mut value = encode(
            outcome.result.map_err(|error| workspace_error(error, correlation))?,
            correlation,
        )?;
        if !approval_follow_up.is_empty() {
            value["approval_follow_up"] = encode(approval_follow_up, correlation)?;
        }
        Ok(value)
    }
}
