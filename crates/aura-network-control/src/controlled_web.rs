//! Core-issued, one-request public retrieval authority. This type is never a
//! Wire DTO: only the Core Web application constructs it after its mode guard.
use crate::{GatewayRequest, NetworkControlError, normalize_and_resolve};
use aura_domain_common::{UtcTimestamp, jcs::canonical_sha256};
use aura_permission_domain::{
    AuthorizedNetworkRequest, CoreRequestGrant, NetworkDestinationPolicy, NetworkProtocolClass,
    NetworkRequestPlan, RedirectPolicy,
};
use aura_security_contract::{
    InvocationActor, NetworkCallerKind, NetworkDataClass, NetworkDestinationClass, PermissionMode,
};
#[cfg(test)]
mod tests;

pub struct ControlledWebRequestGrant {
    request_hash: aura_domain_common::jcs::CanonicalSha256HashV1,
}

impl ControlledWebRequestGrant {
    /// `mode`/revision must be read from the Core's current authority under the
    /// same barrier used to register cancellation. No stored network Grant or
    /// caller-provided channel can create this authority.
    pub fn issue(
        plan: &NetworkRequestPlan,
        mode: PermissionMode,
        revision: u64,
        expires_at: UtcTimestamp,
    ) -> Result<CoreRequestGrant<Self>, NetworkControlError> {
        if mode == PermissionMode::ReadOnly || revision == 0 || expires_at <= UtcTimestamp::now() {
            return Err(NetworkControlError::GrantNotApplicable);
        }
        validate_shape(plan)?;
        let url =
            url::Url::parse(&plan.requested_url).map_err(|_| NetworkControlError::InvalidUrl)?;
        let host = match url.host().ok_or(NetworkControlError::InvalidUrl)? {
            url::Host::Domain(v) => v.trim_end_matches('.').to_ascii_lowercase(),
            url::Host::Ipv4(v) => v.to_string(),
            url::Host::Ipv6(v) => v.to_string(),
        };
        Ok(CoreRequestGrant {
            logical_request_id: plan.request_id,
            purpose: Self {
                request_hash: canonical_sha256(plan)
                    .map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?,
            },
            caller: NetworkCallerKind::WebRuntime,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host,
                port: url.port_or_known_default().ok_or(NetworkControlError::InvalidUrl)?,
                allowed_destination_classes: vec![NetworkDestinationClass::Public],
            },
            methods: vec!["GET".into()],
            redirect_policy: RedirectPolicy::Deny,
            data_classes: plan.data_classes.clone(),
            byte_limits: plan.byte_limits.clone(),
            expires_at,
        })
    }
    pub(crate) fn validate(&self, request: &GatewayRequest) -> Result<(), NetworkControlError> {
        validate_shape(&request.plan)?;
        if !request.headers.is_empty()
            || !request.body.is_empty()
            || canonical_sha256(&request.plan)
                .map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?
                != self.request_hash
        {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        Ok(())
    }
}

fn validate_shape(plan: &NetworkRequestPlan) -> Result<(), NetworkControlError> {
    let id = &plan.identity;
    let url = url::Url::parse(&plan.requested_url).map_err(|_| NetworkControlError::InvalidUrl)?;
    if plan.caller != NetworkCallerKind::WebRuntime
        || !matches!(id.actor, InvocationActor::ParentAgent | InvocationActor::ChildAgent)
        || id.agent_run_id.is_none()
        || id.turn_task_id.is_none()
        || id.workspace_id.is_none()
        || id.session_id.is_none()
        || id.permission_snapshot_id.is_none()
        || plan.method != "GET"
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || plan.requested_url.len() > 32768
        || url.host().is_none()
        || plan.redirect_policy != RedirectPolicy::Deny
        || plan.destination.is_some()
        || !plan.credential_refs.is_empty()
        || !plan.network_grant_ids.is_empty()
        || !plan.request_data.is_empty()
        || plan.byte_limits.max_upload_bytes != 0
        || !(1..=8 * 1024 * 1024).contains(&plan.byte_limits.max_download_bytes)
        || !(1..=8 * 1024 * 1024).contains(&plan.byte_limits.max_decompressed_bytes)
        || !(1..=30000).contains(&plan.timeout_ms)
        || plan.data_classes.iter().any(|class| {
            matches!(
                class,
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
    Ok(())
}

pub(crate) async fn authorize(
    mut plan: NetworkRequestPlan,
    dns_timeout: std::time::Duration,
) -> Result<AuthorizedNetworkRequest, NetworkControlError> {
    validate_shape(&plan)?;
    let (_, _, destination) = normalize_and_resolve(&plan.requested_url, dns_timeout).await?;
    if destination.classes != [NetworkDestinationClass::Public] {
        return Err(NetworkControlError::DestinationBlocked);
    }
    plan.destination = Some(destination.clone());
    Ok(AuthorizedNetworkRequest { plan, destination, matched_grant_ids: vec![] })
}
