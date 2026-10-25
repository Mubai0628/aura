//! General Tool networking is admitted only by the sealed exact-attempt
//! authority built from a FullAccess snapshot. This does not accept a caller
//! channel field, reusable network approval or Provider credential reference.
use crate::{NetworkControlError, NetworkControlOptions, resolver::normalize_with_options};
use aura_permission_domain::{AuthorizedNetworkRequest, NetworkRequestPlan};
use aura_security_contract::{NetworkCallerKind, NetworkDataClass};

pub(crate) async fn authorize(
    plan: NetworkRequestPlan,
    options: &NetworkControlOptions,
) -> Result<AuthorizedNetworkRequest, NetworkControlError> {
    authorize_for_caller(plan, options, NetworkCallerKind::AuraTool).await
}

pub(crate) async fn authorize_for_caller(
    mut plan: NetworkRequestPlan,
    options: &NetworkControlOptions,
    caller: NetworkCallerKind,
) -> Result<AuthorizedNetworkRequest, NetworkControlError> {
    if plan.caller != caller
        || !plan.credential_refs.is_empty()
        || plan.timeout_ms == 0
        || plan.byte_limits.max_download_bytes == 0
        || plan.byte_limits.max_decompressed_bytes == 0
        || plan.method.is_empty()
        || plan.method.len() > 16
        || !plan.method.bytes().all(|v| v.is_ascii_uppercase() || v == b'-')
        || plan.data_classes.iter().any(|v| {
            matches!(
                v,
                NetworkDataClass::CredentialMaterial
                    | NetworkDataClass::SecurityInput
                    | NetworkDataClass::Password
                    | NetworkDataClass::Otp
                    | NetworkDataClass::Cookie
                    | NetworkDataClass::PrivateKey
            )
        })
    {
        return Err(NetworkControlError::CoreGrantBindingMismatch);
    }
    let (_, _, destination) = normalize_with_options(&plan.requested_url, options).await?;
    if plan.destination.as_ref().is_some_and(|expected| expected != &destination) {
        return Err(NetworkControlError::DestinationBlocked);
    }
    // The outer grant still constrains each hop's destination classes, method,
    // timeout, bytes and credential origin. Source text is not relabelled and
    // ordinary FullAccess uploads do not use heuristic content DLP.
    plan.destination = Some(destination.clone());
    Ok(AuthorizedNetworkRequest { plan, destination, matched_grant_ids: vec![] })
}
