use crate::{GatewayRequest, NetworkControlError};
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_permission_domain::ToolCredentialUseV1;
use serde::Serialize;

pub fn ordinary_request_hash(
    request: &GatewayRequest,
    credential: Option<&ToolCredentialUseV1>,
) -> Result<CanonicalSha256HashV1, NetworkControlError> {
    #[derive(Serialize)]
    struct Material<'a> {
        method: &'a str,
        url: &'a str,
        headers: &'a [(String, String)],
        body: CanonicalSha256HashV1,
        credential: Option<&'a ToolCredentialUseV1>,
        limits: &'a aura_permission_domain::NetworkByteLimits,
        redirect: &'a aura_permission_domain::RedirectPolicy,
        timeout_ms: u64,
        caller: aura_security_contract::NetworkCallerKind,
        data: &'a [aura_security_contract::NetworkDataClass],
    }
    canonical_sha256(&Material {
        method: &request.plan.method,
        url: &request.plan.requested_url,
        headers: &request.headers,
        body: CanonicalSha256HashV1::sha256(&request.body),
        credential,
        limits: &request.plan.byte_limits,
        redirect: &request.plan.redirect_policy,
        timeout_ms: request.plan.timeout_ms,
        caller: request.plan.caller,
        data: &request.plan.data_classes,
    })
    .map_err(|_| NetworkControlError::CoreGrantBindingMismatch)
}

/// Result and diagnostic URLs never reproduce userinfo, fragments or query values.
pub fn safe_url(raw: &str) -> String {
    let Ok(mut url) = url::Url::parse(raw) else { return "[invalid URL]".into() };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_fragment(None);
    if url.query().is_some() {
        url.set_query(Some("redacted"));
    }
    url.into()
}
