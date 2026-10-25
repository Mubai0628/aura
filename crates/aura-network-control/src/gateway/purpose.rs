//! What a Core-issued grant was issued for decides how the gateway treats its request: the
//! request check, credentials, the user's proxy, private destinations and failure reporting.
use super::*;

pub trait GatewayGrantPurpose: sealed::Sealed {
    fn validate_request(&self, request: &GatewayRequest) -> Result<(), NetworkControlError>;

    fn provider_credential_binding(&self) -> Option<&ConfigurationRequestCredentialBindingV1> {
        None
    }

    fn allows_ui_configuration_private_destination(&self) -> bool {
        false
    }

    fn tool_credential_binding(&self) -> Option<&aura_permission_domain::ToolCredentialUseV1> {
        None
    }

    fn tool_success_status_ranges(&self) -> Option<&[(u16, u16)]> {
        None
    }

    fn is_controlled_web(&self) -> bool {
        false
    }
    fn is_general_tool(&self) -> bool {
        false
    }
    fn is_ordinary_tool(&self) -> bool {
        false
    }
    fn frozen_user_proxy(&self) -> Option<&aura_tool_contract::ToolNetworkProxyV1> {
        None
    }
    /// The user's proxy applies to this purpose (an https request to the public internet).
    fn uses_user_proxy(&self) -> bool {
        false
    }
    /// The user trusted this exact endpoint to receive what the request carries (tool
    /// arguments for an MCP server), so no separate data-egress approval is asked for.
    fn trusted_endpoint_content(&self) -> bool {
        false
    }
    /// A transport failure says whether the request could have reached the server, so the
    /// caller can settle a call as not started rather than unknown.
    fn reports_unsent_failures(&self) -> bool {
        false
    }
    /// The body is credential material Core itself holds (an authorization code, a verifier or
    /// a refresh token for an MCP sign-in), never model content, so it is not secret-scanned.
    fn carries_own_credentials(&self) -> bool {
        false
    }
    /// The caller reads a `Bearer` challenge (an MCP server that wants a sign-in names its
    /// metadata there); other `WWW-Authenticate` values stay redacted.
    fn reads_bearer_challenge(&self) -> bool {
        false
    }
}

impl sealed::Sealed for crate::McpHttpRequestGrant {}
impl GatewayGrantPurpose for crate::McpHttpRequestGrant {
    fn validate_request(&self, request: &GatewayRequest) -> Result<(), NetworkControlError> {
        self.validate(request)
    }
    /// The trusted address may itself be a local one; the grant's destination classes still
    /// bind it to exactly the class that address names.
    fn allows_ui_configuration_private_destination(&self) -> bool {
        true
    }
    fn frozen_user_proxy(&self) -> Option<&aura_tool_contract::ToolNetworkProxyV1> {
        self.network_proxy()
    }
    fn tool_credential_binding(&self) -> Option<&aura_permission_domain::ToolCredentialUseV1> {
        self.credential()
    }
    fn reads_bearer_challenge(&self) -> bool {
        true
    }
    fn uses_user_proxy(&self) -> bool {
        true
    }
    fn trusted_endpoint_content(&self) -> bool {
        true
    }
    fn reports_unsent_failures(&self) -> bool {
        true
    }
}

impl sealed::Sealed for crate::McpOAuthRequestGrant {}
impl GatewayGrantPurpose for crate::McpOAuthRequestGrant {
    fn validate_request(&self, request: &GatewayRequest) -> Result<(), NetworkControlError> {
        self.validate(request)
    }
    /// A sign-in address may itself be a local one; the grant still binds its own class.
    fn allows_ui_configuration_private_destination(&self) -> bool {
        true
    }
    fn frozen_user_proxy(&self) -> Option<&aura_tool_contract::ToolNetworkProxyV1> {
        self.network_proxy()
    }
    fn uses_user_proxy(&self) -> bool {
        true
    }
    fn trusted_endpoint_content(&self) -> bool {
        true
    }
    fn reports_unsent_failures(&self) -> bool {
        true
    }
    fn carries_own_credentials(&self) -> bool {
        true
    }
}

impl sealed::Sealed for crate::ordinary_tools::OrdinaryToolRequestGrant {}
impl GatewayGrantPurpose for crate::ordinary_tools::OrdinaryToolRequestGrant {
    fn validate_request(&self, request: &GatewayRequest) -> Result<(), NetworkControlError> {
        self.validate(request)
    }
    fn is_general_tool(&self) -> bool {
        true
    }
    fn is_ordinary_tool(&self) -> bool {
        true
    }
    fn tool_credential_binding(&self) -> Option<&aura_permission_domain::ToolCredentialUseV1> {
        self.credential()
    }
    fn frozen_user_proxy(&self) -> Option<&aura_tool_contract::ToolNetworkProxyV1> {
        self.network_proxy()
    }
}

impl sealed::Sealed for crate::ControlledWebRequestGrant {}
impl GatewayGrantPurpose for crate::ControlledWebRequestGrant {
    fn validate_request(&self, request: &GatewayRequest) -> Result<(), NetworkControlError> {
        self.validate(request)
    }
    fn is_controlled_web(&self) -> bool {
        true
    }
}

impl sealed::Sealed for StandardNetworkRequestGrant {}

impl GatewayGrantPurpose for StandardNetworkRequestGrant {
    fn validate_request(&self, _request: &GatewayRequest) -> Result<(), NetworkControlError> {
        Ok(())
    }
}

impl sealed::Sealed for ProviderTransportAttemptGrant {}

impl GatewayGrantPurpose for ProviderTransportAttemptGrant {
    fn validate_request(&self, request: &GatewayRequest) -> Result<(), NetworkControlError> {
        validate_provider_transport_request(self, request)
    }

    fn provider_credential_binding(&self) -> Option<&ConfigurationRequestCredentialBindingV1> {
        self.credential_binding.as_ref()
    }

    fn allows_ui_configuration_private_destination(&self) -> bool {
        matches!(
            self.authorization,
            ProviderTransportAuthorizationBinding::UserConfiguration { .. }
        )
    }
}

impl sealed::Sealed for ToolHttpAttemptGrantV1 {}

impl GatewayGrantPurpose for ToolHttpAttemptGrantV1 {
    fn validate_request(&self, request: &GatewayRequest) -> Result<(), NetworkControlError> {
        self.validate_request(request)
    }

    fn tool_credential_binding(&self) -> Option<&aura_permission_domain::ToolCredentialUseV1> {
        self.credential()
    }

    fn tool_success_status_ranges(&self) -> Option<&[(u16, u16)]> {
        Some(self.success_status_ranges())
    }
    fn is_general_tool(&self) -> bool {
        true
    }
}
