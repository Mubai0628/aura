use super::*;
use aura_permission_domain::{
    ListNetworkSessionConsentsInput, NetworkSessionConsentAvailability as A,
    NetworkSessionConsentView, RevokeNetworkSessionConsentInput,
};

impl P04Services {
    pub(super) fn network_consent_command(
        &self,
        method_name: &str,
        params: &Value,
        correlation: CorrelationId,
    ) -> Result<Value, IpcErrorEnvelope> {
        let work = || -> Result<Value> {
            let journal = self
                .external_grants
                .bound_journal()
                .ok_or_else(|| anyhow!("permission.journal_unbound"))?;
            let mut store = journal.lock()?;
            match method_name {
                method::NETWORK_SESSION_CONSENT_LIST => {
                    let input: ListNetworkSessionConsentsInput =
                        serde_json::from_value(params.clone())?;
                    let rows = aura_persistence_v2::network_consents::list(
                        store.database.connection(),
                        input.session_id,
                    )?;
                    Ok(serde_json::to_value(
                        rows.into_iter()
                            .filter_map(view)
                            .filter(|v| input.include_inactive || v.availability == A::InForce)
                            .collect::<Vec<_>>(),
                    )?)
                }
                method::NETWORK_SESSION_CONSENT_REVOKE => {
                    let input: RevokeNetworkSessionConsentInput =
                        serde_json::from_value(params.clone())?;
                    store.database.unit_of_work(|tx| {
                        aura_persistence_v2::network_consents::revoke(
                            tx,
                            input.session_id,
                            input.grant_id,
                            input.expected_revision,
                            UtcTimestamp::now(),
                        )
                    })?;
                    let grant = aura_persistence_v2::security::grant(
                        store.database.connection(),
                        input.grant_id,
                    )?
                    .ok_or_else(|| anyhow!("permission.grant_state_changed"))?;
                    Ok(serde_json::to_value(
                        view(grant).ok_or_else(|| anyhow!("permission.grant_state_changed"))?,
                    )?)
                }
                _ => Err(anyhow!("permission.unknown_consent_command")),
            }
        };
        work().map_err(|e| crate::broker::error_envelope(correlation, e))
    }
}
fn view(grant: aura_security_contract::AuthorizationGrant) -> Option<NetworkSessionConsentView> {
    let scope = grant.scope.network_consent?;
    let now = UtcTimestamp::now();
    let availability = if grant.revoked_at.is_some() {
        A::Revoked
    } else if grant.expires_at.is_some_and(|t| t <= now) {
        A::Expired
    } else if grant.issued_at > now {
        A::NotYetValid
    } else {
        A::InForce
    };
    Some(NetworkSessionConsentView {
        grant_id: grant.grant_id,
        grant_revision: grant.revision,
        scope,
        availability,
        issued_at: grant.issued_at,
        expires_at: grant.expires_at,
        revoked_at: grant.revoked_at,
    })
}
