//! Requests to a Streamable HTTP MCP server the user trusted in Settings. Core issues
//! one grant per request after checking that the server is still trusted, switched on and
//! unchanged. The grant reaches exactly the trusted endpoint (scheme, host, port and path),
//! with POST or DELETE and the MCP headers only; it never follows a redirect; and it reaches a
//! private or loopback address only when the trusted address itself names one: an IP address
//! keeps its own class, `localhost` is the loopback address, any other domain name may only
//! resolve to the public internet (so a public name cannot be rebound to a local machine).
use crate::{GatewayRequest, NetworkControlError, classify_ip};
use aura_domain_common::UtcTimestamp;
use aura_permission_domain::{
    CoreRequestGrant, NetworkDestinationPolicy, NetworkProtocolClass, RedirectPolicy,
    ToolCredentialPlacementV1, ToolCredentialUseV1,
};
use aura_security_contract::{NetworkCallerKind, NetworkDestinationClass};
use url::{Host, Url};

/// The headers Core sets on every MCP request; any other header must be one the user
/// configured for this server.
const MANAGED_HEADERS: [&str; 4] =
    ["accept", "content-type", "mcp-session-id", "mcp-protocol-version"];

pub struct McpHttpRequestGrant {
    endpoint: String,
    configured_headers: Vec<String>,
    network_proxy: Option<aura_tool_contract::ToolNetworkProxyV1>,
    /// The secret or sign-in the server's configuration names; the gateway adds it after the
    /// secret scan and scans the answer for it.
    credential: Option<ToolCredentialUseV1>,
}

impl McpHttpRequestGrant {
    /// The destination classes a trusted MCP address may reach.
    pub fn destination_classes(
        endpoint: &Url,
    ) -> Result<Vec<NetworkDestinationClass>, NetworkControlError> {
        Ok(match endpoint.host().ok_or(NetworkControlError::InvalidUrl)? {
            Host::Ipv4(address) => classify_ip(&address.into()),
            Host::Ipv6(address) => classify_ip(&address.into()),
            Host::Domain(name) if name.trim_end_matches('.').eq_ignore_ascii_case("localhost") => {
                vec![NetworkDestinationClass::Loopback]
            }
            Host::Domain(_) => vec![NetworkDestinationClass::Public],
        })
    }

    /// The grant for one request to the trusted endpoint. `configured_headers` are the header
    /// names the user configured for the server; `network_proxy` is the user's proxy setting,
    /// which only an https request to the public internet uses.
    pub fn issue(
        endpoint: &str,
        configured_headers: &[String],
        network_proxy: Option<aura_tool_contract::ToolNetworkProxyV1>,
        request: &GatewayRequest,
        expires_at: UtcTimestamp,
    ) -> Result<CoreRequestGrant<Self>, NetworkControlError> {
        Self::issue_with_credential(
            endpoint,
            configured_headers,
            network_proxy,
            None,
            request,
            expires_at,
        )
    }

    /// [`Self::issue`] for a server that signs in: `credential` is placed in a header (bearer
    /// or one named header that is neither Aura's nor a configured one), never elsewhere.
    pub fn issue_with_credential(
        endpoint: &str,
        configured_headers: &[String],
        network_proxy: Option<aura_tool_contract::ToolNetworkProxyV1>,
        credential: Option<ToolCredentialUseV1>,
        request: &GatewayRequest,
        expires_at: UtcTimestamp,
    ) -> Result<CoreRequestGrant<Self>, NetworkControlError> {
        let placed = match credential.as_ref().map(|credential| &credential.placement) {
            None => None,
            Some(ToolCredentialPlacementV1::BearerAuthorization) => {
                Some("authorization".to_owned())
            }
            Some(ToolCredentialPlacementV1::FixedHeader { header_name }) => {
                Some(header_name.as_str().to_ascii_lowercase())
            }
            Some(ToolCredentialPlacementV1::Environment { .. }) => {
                return Err(NetworkControlError::CoreGrantBindingMismatch);
            }
        };
        if placed.as_ref().is_some_and(|name| {
            MANAGED_HEADERS.contains(&name.as_str())
                || configured_headers.iter().any(|header| header.eq_ignore_ascii_case(name))
        }) {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        let url = Url::parse(endpoint).map_err(|_| NetworkControlError::InvalidUrl)?;
        let classes = Self::destination_classes(&url)?;
        let local = classes.iter().all(|class| *class == NetworkDestinationClass::Loopback);
        let plan = &request.plan;
        if plan.caller != NetworkCallerKind::McpHttp
            || plan.requested_url != endpoint
            || !matches!(plan.method.as_str(), "POST" | "DELETE")
            || plan.redirect_policy != RedirectPolicy::Deny
            || !(url.scheme() == "https" || url.scheme() == "http" && local)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || !plan.credential_refs.is_empty()
            || !plan.network_grant_ids.is_empty()
            || !plan.request_data.is_empty()
            || plan.destination.is_some()
            || classes.contains(&NetworkDestinationClass::Metadata)
            || expires_at <= UtcTimestamp::now()
        {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        let host = match url.host().ok_or(NetworkControlError::InvalidUrl)? {
            Host::Domain(name) => name.trim_end_matches('.').to_ascii_lowercase(),
            Host::Ipv4(address) => address.to_string(),
            Host::Ipv6(address) => address.to_string(),
        };
        let port = url.port_or_known_default().ok_or(NetworkControlError::InvalidUrl)?;
        let grant = Self {
            endpoint: endpoint.to_owned(),
            configured_headers: configured_headers.iter().map(|h| h.to_ascii_lowercase()).collect(),
            network_proxy,
            credential,
        };
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
        let unknown_header = request.headers.iter().any(|(name, _)| {
            let name = name.to_ascii_lowercase();
            !MANAGED_HEADERS.contains(&name.as_str()) && !self.configured_headers.contains(&name)
        });
        if request.plan.requested_url != self.endpoint
            || request.plan.caller != NetworkCallerKind::McpHttp
            || !matches!(request.plan.method.as_str(), "POST" | "DELETE")
            || unknown_header
        {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        Ok(())
    }

    pub(crate) fn network_proxy(&self) -> Option<&aura_tool_contract::ToolNetworkProxyV1> {
        self.network_proxy.as_ref()
    }

    pub(crate) fn credential(&self) -> Option<&ToolCredentialUseV1> {
        self.credential.as_ref()
    }
}
