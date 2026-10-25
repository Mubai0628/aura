use super::ordinary_request_hash;
use crate::{GatewayRequest, NetworkControlError};
use aura_domain_common::{UtcTimestamp, jcs::CanonicalSha256HashV1};
use aura_execution_contract::{AuthorizedDispatch, OperationNetwork};
use aura_permission_domain::{
    CoreRequestGrant, NetworkDestinationPolicy, NetworkProtocolClass, ToolCredentialUseV1,
};
use aura_security_contract::{
    NetworkCallerKind, NetworkDestinationClass, ResolvedCapability, ResourceDescriptor,
};

/// Rust-only authority for exactly the immutable request in a reserved Agent dispatch.
pub struct OrdinaryToolRequestGrant {
    request_hash: CanonicalSha256HashV1,
    credential: Option<ToolCredentialUseV1>,
    network_proxy: Option<aura_tool_contract::ToolNetworkProxyV1>,
}

impl OrdinaryToolRequestGrant {
    /// A retrieval renderer may fetch credential-free scripts from its original origin.
    /// The parent request must still prove the reserved operation before this narrower grant exists.
    pub fn subresource(
        dispatch: &AuthorizedDispatch,
        parent: &GatewayRequest,
        request: &GatewayRequest,
        expires_at: UtcTimestamp,
    ) -> Result<CoreRequestGrant<Self>, NetworkControlError> {
        let mut grant = Self::issue(dispatch, parent, None, expires_at)?;
        let parent_url = url::Url::parse(&parent.plan.requested_url)
            .map_err(|_| NetworkControlError::InvalidUrl)?;
        let next = url::Url::parse(&request.plan.requested_url)
            .map_err(|_| NetworkControlError::InvalidUrl)?;
        if parent.plan.caller != NetworkCallerKind::WebRuntime
            || parent.plan.method != "GET"
            || request.plan.method != "GET"
            || !request.body.is_empty()
            || !request.headers.is_empty()
            || request.plan.identity != parent.plan.identity
            || request.plan.caller != parent.plan.caller
            || next.origin() != parent_url.origin()
            || !next.username().is_empty()
            || next.password().is_some()
            || next.fragment().is_some()
            || request.plan.byte_limits != parent.plan.byte_limits
            || request.plan.redirect_policy != parent.plan.redirect_policy
            || request.plan.timeout_ms != parent.plan.timeout_ms
            || !request.plan.request_data.is_empty()
            || !request.plan.data_classes.is_empty()
            || !request.plan.credential_refs.is_empty()
            || !request.plan.network_grant_ids.is_empty()
            || request.plan.destination.is_some()
        {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        grant.logical_request_id = request.plan.request_id;
        grant.purpose.request_hash = ordinary_request_hash(request, None)?;
        Ok(grant)
    }
    pub fn issue(
        dispatch: &AuthorizedDispatch,
        request: &GatewayRequest,
        credential: Option<ToolCredentialUseV1>,
        expires_at: UtcTimestamp,
    ) -> Result<CoreRequestGrant<Self>, NetworkControlError> {
        let op = &dispatch.request().operation;
        let c = &op.content;
        let url = url::Url::parse(&request.plan.requested_url)
            .map_err(|_| NetworkControlError::InvalidUrl)?;
        let host = match url.host().ok_or(NetworkControlError::InvalidUrl)? {
            url::Host::Domain(v) => v.trim_end_matches('.').to_ascii_lowercase(),
            url::Host::Ipv4(v) => v.to_string(),
            url::Host::Ipv6(v) => v.to_string(),
        };
        let port = url.port_or_known_default().ok_or(NetworkControlError::InvalidUrl)?;
        let request_hash = ordinary_request_hash(request, credential.as_ref())?;
        let caller = if c.action_kind.starts_with("web.") {
            NetworkCallerKind::WebRuntime
        } else {
            NetworkCallerKind::AuraTool
        };
        if !aura_security_contract::controlled_tool_network(
            &c.source,
            &c.action_kind,
            &c.capabilities,
            &c.resources,
        ) || !c.capabilities.contains(&ResolvedCapability::OrdinaryNetwork)
            || c.network != OperationNetwork::ExactOrigins
            || request.plan.identity != c.identity
            || request.plan.caller != caller
            || expires_at <= UtcTimestamp::now()
            || c.action.get("network_request_hash")
                != Some(
                    &serde_json::to_value(&request_hash)
                        .map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?,
                )
            || !c.resources.contains(&ResourceDescriptor::NetworkOrigin {
                scheme: url.scheme().into(),
                host: host.clone(),
                port,
            })
            || !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || !request.plan.credential_refs.is_empty()
            || !request.plan.network_grant_ids.is_empty()
            || !request.plan.request_data.is_empty()
            || request.plan.destination.is_some()
            || credential.as_ref().is_some_and(|use_ref| {
                !c.credentials.iter().any(|bound| {
                    bound.credential_id == use_ref.credential_id
                        && bound.credential_revision == use_ref.credential_revision
                })
            })
        {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        Ok(CoreRequestGrant {
            logical_request_id: request.plan.request_id,
            purpose: Self { request_hash, credential, network_proxy: None },
            caller,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host,
                port,
                allowed_destination_classes: vec![
                    NetworkDestinationClass::Public,
                    NetworkDestinationClass::Loopback,
                    NetworkDestinationClass::Private,
                    NetworkDestinationClass::LinkLocal,
                    NetworkDestinationClass::Metadata,
                    NetworkDestinationClass::Reserved,
                ],
            },
            methods: vec![request.plan.method.clone()],
            redirect_policy: request.plan.redirect_policy,
            data_classes: request.plan.data_classes.clone(),
            byte_limits: request.plan.byte_limits.clone(),
            expires_at,
        })
    }
    pub(crate) fn validate(&self, request: &GatewayRequest) -> Result<(), NetworkControlError> {
        if ordinary_request_hash(request, self.credential.as_ref())? != self.request_hash {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        Ok(())
    }
    pub(crate) fn credential(&self) -> Option<&ToolCredentialUseV1> {
        self.credential.as_ref()
    }
    /// Trusted Core supplies the step's immutable settings, never environment or global state.
    pub fn with_frozen_tool_settings(
        mut self,
        settings: &aura_tool_contract::ToolSettingsSnapshotV1,
    ) -> Result<Self, NetworkControlError> {
        settings.validate().map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?;
        self.network_proxy = settings.network_proxy.clone();
        Ok(self)
    }
    pub(crate) fn network_proxy(&self) -> Option<&aura_tool_contract::ToolNetworkProxyV1> {
        self.network_proxy.as_ref()
    }
}
