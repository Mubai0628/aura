use super::*;
mod memory;
impl P04Services {
    pub(super) fn native_consent_command(
        &self,
        method_name: &str,
        params: &serde_json::Value,
        correlation_id: CorrelationId,
    ) -> Result<serde_json::Value, IpcErrorEnvelope> {
        match method_name {
            method::NATIVE_SESSION_CONSENT_LIST => {
                let request: aura_permission_domain::ListNativeSessionConsentsInput =
                    decode(params, correlation_id)?;
                let journal = self.external_grants.bound_journal().ok_or_else(|| {
                    consent_error(
                        crate::native_session_grants::NativeSessionConsentError::JournalUnbound,
                        correlation_id,
                    )
                })?;
                encode(
                    crate::native_session_grants::list(
                        &journal,
                        &self.persistence,
                        self.runtime_instance_id,
                        &request,
                    )
                    .map_err(|error| consent_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::NATIVE_SESSION_CONSENT_REVOKE => {
                let request: aura_permission_domain::RevokeNativeSessionConsentInput =
                    decode(params, correlation_id)?;
                let journal = self.external_grants.bound_journal().ok_or_else(|| {
                    consent_error(
                        crate::native_session_grants::NativeSessionConsentError::JournalUnbound,
                        correlation_id,
                    )
                })?;
                encode(
                    crate::native_session_grants::revoke(
                        &journal,
                        &self.persistence,
                        self.runtime_instance_id,
                        &request,
                    )
                    .map_err(|error| consent_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            _ => unreachable!("registered Native consent command"),
        }
    }
}
