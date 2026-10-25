//! A ReadOnly session's memory answers ("本会话允许" for one Vault): listing and withdrawal.
use super::*;
use aura_permission_domain::{
    ListMemorySessionConsentsInput, MemorySessionConsentView, RevokeMemorySessionConsentInput,
};
use aura_persistence_v2::memory_consents;
use aura_security_contract::AuthorizationGrant;

impl P04Services {
    pub(in crate::p04) fn memory_consent_command(
        &self,
        method_name: &str,
        params: &serde_json::Value,
        correlation: CorrelationId,
    ) -> Result<serde_json::Value, IpcErrorEnvelope> {
        let failed = |error: aura_persistence_core::PersistenceCoreError| {
            crate::broker::error_envelope(correlation, error)
        };
        let journal = self.external_grants.bound_journal().ok_or_else(|| {
            envelope(
                "security.journal_unbound",
                ErrorCategory::Unavailable,
                "The authority store is not available yet.",
                true,
                correlation,
            )
        })?;
        let mut store = journal.lock().map_err(failed)?;
        match method_name {
            method::MEMORY_SESSION_CONSENT_LIST => {
                let input: ListMemorySessionConsentsInput = decode(params, correlation)?;
                let views = memory_consents::list(store.database.connection(), input.session_id)
                    .map_err(failed)?
                    .into_iter()
                    .filter_map(view)
                    .filter(|v| input.include_inactive || v.in_force)
                    .collect::<Vec<_>>();
                encode(views, correlation)
            }
            method::MEMORY_SESSION_CONSENT_REVOKE => {
                let input: RevokeMemorySessionConsentInput = decode(params, correlation)?;
                store
                    .database
                    .unit_of_work(|tx| {
                        memory_consents::revoke(
                            tx,
                            input.session_id,
                            input.grant_id,
                            input.expected_revision,
                            UtcTimestamp::now(),
                        )
                    })
                    .map_err(|error| match error {
                        // Another revision, another session, or already withdrawn: the
                        // caller rereads instead of retrying the same withdrawal.
                        aura_persistence_core::PersistenceCoreError::Conflict => envelope(
                            "permission.grant_state_changed",
                            ErrorCategory::Conflict,
                            "The memory approval changed or is not this session's; reload it.",
                            false,
                            correlation,
                        ),
                        other => failed(other),
                    })?;
                let grant = aura_persistence_v2::security::grant(
                    store.database.connection(),
                    input.grant_id,
                )
                .map_err(failed)?
                .and_then(view)
                .ok_or_else(|| {
                    envelope(
                        "permission.grant_state_changed",
                        ErrorCategory::Conflict,
                        "The withdrawn memory approval could not be reread.",
                        false,
                        correlation,
                    )
                })?;
                encode(grant, correlation)
            }
            _ => unreachable!("registered memory consent command"),
        }
    }
}

fn view(grant: AuthorizationGrant) -> Option<MemorySessionConsentView> {
    let scope = grant.scope.memory_consent?;
    if !scope.accepts_grant_revision(grant.revision) {
        return None;
    }
    Some(MemorySessionConsentView {
        grant_id: grant.grant_id,
        grant_revision: grant.revision,
        in_force: grant.revoked_at.is_none(),
        scope,
        issued_at: grant.issued_at,
        revoked_at: grant.revoked_at,
    })
}
