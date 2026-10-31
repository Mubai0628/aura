//! Acceptance owns the legacy grant result and the message transaction even after the caller
//! stops waiting. The per-session lease serializes sends; no journal lock spans a legacy wait.
use super::*;
use aura_core_base::append_core_log;
use aura_persistence_core::{Connection, StoreResult};
use aura_security_service::external_grants;

impl AgentApplication {
    pub(in crate::agent_application) async fn accept_attachments<T, R, F>(
        self: &Arc<Self>,
        step: &'static str,
        session: SessionId,
        grants: Vec<external_grants::AttachmentGrant>,
        replay: R,
        commit: F,
    ) -> anyhow::Result<T>
    where
        T: Send + 'static,
        R: Fn(&Connection) -> StoreResult<Option<T>> + Send + 'static,
        F: FnOnce(&mut aura_persistence_v2::V2Store) -> StoreResult<T> + Send + 'static,
    {
        let boundary = self.execution.persistence.permission_change_lease(session).await;
        let journal = Arc::clone(&self.execution.journal);
        let persistence = Arc::clone(&self.execution.persistence);
        self.store.run_critical(StoreStep::operation(step), StoreAccess::Write, move || {
            let _boundary = boundary;
            if let Some(previous) = { let journal = journal.lock()?; replay(journal.database.connection())? } {
                return Ok(previous);
            }
            let records = grants.iter().map(|grant| {
                debug_assert_eq!(aura_workspace::domain_identity(&grant.file.info().identity), grant.record.root_identity);
                grant.record.clone()
            }).collect::<Vec<_>>();
            let created = if records.is_empty() { Vec::new() } else {
                let result = persistence.prepare_attachment_grants(records, CorrelationId::new())?;
                // This bounded owner keeps listening to the one admitted writer job. Its
                // caller can receive Store Unknown; the job is neither replayed nor abandoned.
                result.blocking_recv().map_err(|_| anyhow::anyhow!(
                    "command.outcome_unknown: 附件授权结果尚未确认，请查询原命令结果，不要自动重发。"))??
            };
            let result = (|| {
                let mut journal = journal.lock()?;
                if let Some(previous) = replay(journal.database.connection())? { return Ok(previous); }
                commit(&mut journal)
            })();
            if let Err(error) = result {
                // A real transaction failure rolled back the message. Revoke only grants this
                // acceptance actually created; an existing grant is never ours to revoke.
                let mut compensation_failed = false;
                for id in created {
                    if let Err(compensation) = persistence.revoke_external_grant(id, session, CorrelationId::new()) {
                        append_core_log("attachment_grant_compensation_unknown", serde_json::json!({
                            "session_id": session, "external_grant_id": id,
                            "message_error": error.to_string(), "compensation_error": compensation.to_string()
                        }));
                        compensation_failed = true;
                    }
                }
                if compensation_failed {
                    return Err(anyhow::anyhow!("command.outcome_unknown: 消息未保存，附件授权撤销结果尚未确认，请查看外部授权并查询原命令结果。"));
                }
                return Err(error.into());
            }
            // `grants` retains the selected native handles throughout both outcomes.
            drop(grants);
            result.map_err(Into::into)
        }).await
    }
}
