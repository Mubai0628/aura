//! The requests Core makes to sign in to a trusted MCP server — its protected-resource
//! metadata, its authorization server's metadata, client registration and tokens. One grant per
//! request, for exactly the one URL Core derived from the trusted server and the metadata it
//! published; the same address rules as the server itself (a domain only on the public
//! internet, http only on this computer), GET or POST, no redirect. The body carries an
//! authorization code, a verifier or a refresh token Core itself holds, so it is not scanned
//! for secrets; model content never goes into these requests.
use crate::{GatewayRequest, McpHttpRequestGrant, NetworkControlError};
use aura_domain_common::UtcTimestamp;
use aura_permission_domain::{
    CoreRequestGrant, NetworkDestinationPolicy, NetworkProtocolClass, RedirectPolicy,
};
use aura_security_contract::{NetworkCallerKind, NetworkDestinationClass};
use url::{Host, Url};

const HEADERS: [&str; 2] = ["accept", "content-type"];

pub struct McpOAuthRequestGrant {
    url: String,
    network_proxy: Option<aura_tool_contract::ToolNetworkProxyV1>,
}

impl McpOAuthRequestGrant {
    pub fn issue(
        url: &str,
        network_proxy: Option<aura_tool_contract::ToolNetworkProxyV1>,
        request: &GatewayRequest,
        expires_at: UtcTimestamp,
    ) -> Result<CoreRequestGrant<Self>, NetworkControlError> {
        let parsed = Url::parse(url).map_err(|_| NetworkControlError::InvalidUrl)?;
        let classes = McpHttpRequestGrant::destination_classes(&parsed)?;
        let local = classes.iter().all(|class| *class == NetworkDestinationClass::Loopback);
        let plan = &request.plan;
        if plan.caller != NetworkCallerKind::McpHttp
            || plan.requested_url != url
            || !matches!(plan.method.as_str(), "GET" | "POST")
            || plan.redirect_policy != RedirectPolicy::Deny
            || !(parsed.scheme() == "https" || parsed.scheme() == "http" && local)
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
            || !plan.credential_refs.is_empty()
            || !plan.network_grant_ids.is_empty()
            || !plan.request_data.is_empty()
            || plan.destination.is_some()
            || classes.contains(&NetworkDestinationClass::Metadata)
            || expires_at <= UtcTimestamp::now()
        {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        let host = match parsed.host().ok_or(NetworkControlError::InvalidUrl)? {
            Host::Domain(name) => name.trim_end_matches('.').to_ascii_lowercase(),
            Host::Ipv4(address) => address.to_string(),
            Host::Ipv6(address) => address.to_string(),
        };
        let port = parsed.port_or_known_default().ok_or(NetworkControlError::InvalidUrl)?;
        let grant = Self { url: url.to_owned(), network_proxy };
        grant.validate(request)?;
        Ok(CoreRequestGrant {
            logical_request_id: plan.request_id,
            purpose: grant,
            caller: NetworkCallerKind::McpHttp,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host,
                port,
                allowed_destination_classes: classes,
            },
            methods: vec![plan.method.clone()],
            redirect_policy: RedirectPolicy::Deny,
            data_classes: plan.data_classes.clone(),
            byte_limits: plan.byte_limits.clone(),
            expires_at,
        })
    }

    pub(crate) fn validate(&self, request: &GatewayRequest) -> Result<(), NetworkControlError> {
        if request.plan.requested_url != self.url
            || request.plan.caller != NetworkCallerKind::McpHttp
            || !matches!(request.plan.method.as_str(), "GET" | "POST")
            || request
                .headers
                .iter()
                .any(|(name, _)| !HEADERS.contains(&name.to_ascii_lowercase().as_str()))
        {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        Ok(())
    }

    pub(crate) fn network_proxy(&self) -> Option<&aura_tool_contract::ToolNetworkProxyV1> {
        self.network_proxy.as_ref()
    }
}
