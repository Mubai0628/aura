#![forbid(unsafe_code)]

pub mod connect_failure;
mod controlled_web;
mod credential_reflection_scan;
#[cfg(feature = "test-fixture-dns")]
mod fixture_dns;
mod general_tool;
#[cfg(feature = "test-fixture-dns")]
pub use fixture_dns::TestFixtureDnsRecord;
pub mod ordinary_tools;
pub use controlled_web::ControlledWebRequestGrant;
mod gateway;
mod grant;
mod mcp_grant;
mod mcp_oauth_grant;
mod provider_grant;
mod proxy;
mod quarantine;
mod resolver;
mod secret;
mod tool_grant;

use std::time::Duration;

use aura_domain_common::{CredentialId, CredentialRef, CredentialRevision, NetworkGrantId};
use aura_model_domain::CredentialBindingFingerprint;
use aura_permission_domain::{
    AuthorizedNetworkRequest, NetworkGrant, NetworkRequestPlan, NormalizedOrigin,
};
use thiserror::Error;

pub use aura_permission_domain::{
    CredentialReflectionEncodingSetV1, CredentialReflectionEncodingV1,
    CredentialReflectionEvidenceErrorV1, CredentialReflectionScanEvidenceV1,
    CredentialReflectionScanStatusV1, CredentialReflectionSurfaceSetV1,
    CredentialReflectionSurfaceV1,
};
pub use gateway::{
    CoreHttpClientGateway, GatewayBodyStream, GatewayGrantPurpose, GatewayHopRouteV1,
    GatewayRequest, GatewayResponse, GatewayRetryAfterV1, GatewayRouteV1, GatewayStreamResponse,
    OrdinaryGatewayResponse, OrdinaryGatewayStream, ProviderTransportGatewayRequest,
    ProviderTransportImmutableRequest, ProviderTransportObservedResponse,
    ToolHttpGatewayResponseV1, UserProxyFailureKindV1, UserProxyFailureV1, probe_user_proxy,
};
pub use grant::{
    InMemoryNetworkGrantStore, NetworkGrantStore, PersistenceNetworkGrantStore, RateLimitConfig,
    RateLimiter,
};
pub use mcp_grant::McpHttpRequestGrant;
pub use mcp_oauth_grant::McpOAuthRequestGrant;
pub use provider_grant::{
    AuthenticatedConfigurationChannelIdentity, CONFIGURATION_MANIFEST_SCHEMA_VERSION,
    CONFIGURATION_REDIRECT_MAX_HOPS, ConfigurationOperationKind,
    ConfigurationOperationRequestManifestV1, ConfigurationRequestCredentialBindingV1,
    ConfigurationRequestDataClass, ConfigurationRequestPurpose, ConfigurationRequestTargetV1,
    ConfigurationResponseMediaPolicyV1, ConfigurationTransportLimitsV1,
    ConfigurationTransportPolicyV1, ModelDiscoveryOperationLimitsV1,
    PROVIDER_TRANSPORT_GRANT_TTL_SECONDS, ProviderGrantContractError,
    ProviderTransportAttemptGrant, ProviderTransportAuthorizationBinding,
    ProviderTransportRequestAuthority, StandardNetworkRequestGrant,
    UserConfigurationOperationIdentity, canonical_non_secret_headers_hash,
    invocation_identity_binding_hash, issue_configuration_provider_transport_grant,
};
pub use proxy::{
    EgressProxyAuthorization, EgressProxyHandle, EgressProxyServer, ProxyAccessToken,
    ProxyConnectionEvent, ProxyConnectionState, ProxyTokenAuthority,
};
pub use quarantine::{ExecutableDetector, QuarantineStore, QuarantineWriteRequest};
pub use resolver::{
    ProxyDnsPolicy, ProxyDnsResolver, ProxyResolvedAddressSet, classify_ip, normalize_and_resolve,
};
pub use secret::{SecretDetection, SecretScanner, is_authentication_header};
pub use tool_grant::{
    ResolvedToolCredential, ToolCredentialBinding, ToolCredentialResolver, ToolHttpAttemptGrantV1,
    ToolHttpGatewayRequestV1, ToolHttpGrantContractErrorV1, ToolHttpRequestAuthorityV1,
};

#[derive(Debug, Error)]
pub enum NetworkControlError {
    #[error("network.invalid_url")]
    InvalidUrl,
    #[error("network.url_credentials_forbidden")]
    UrlCredentialsForbidden,
    #[error("network.scheme_blocked")]
    SchemeBlocked,
    #[error("network.invalid_method")]
    InvalidMethod,
    #[error("network.destination_blocked")]
    DestinationBlocked,
    #[error("network.control_endpoint_blocked")]
    ControlEndpointBlocked,
    #[error("network.private_network_approval_required")]
    PrivateNetworkApprovalRequired,
    #[error("network.data_egress_approval_required")]
    DataEgressApprovalRequired,
    #[error("network.secret_detected at {location} ({kind})")]
    SecretDetected { location: String, kind: String },
    #[error("network.credential_material_forbidden")]
    CredentialMaterialForbidden,
    #[error("network.grant_expired")]
    GrantExpired,
    #[error("network.grant_not_applicable")]
    GrantNotApplicable,
    #[error("network.core_grant_already_consumed")]
    CoreGrantAlreadyConsumed,
    #[error("network.core_grant_binding_mismatch")]
    CoreGrantBindingMismatch,
    #[error("provider_transport.authority_context_mismatch")]
    ProviderTransportAuthorityContextMismatch,
    #[error("network.redirect_blocked")]
    RedirectBlocked,
    #[error("network.redirect_limit_exceeded")]
    RedirectLimitExceeded,
    #[error("network.rate_limited")]
    RateLimited,
    #[error("network.upload_limit_exceeded")]
    UploadLimitExceeded,
    #[error("network.download_limit_exceeded")]
    DownloadLimitExceeded,
    #[error("transport.content_encoding_unsupported")]
    ContentEncodingUnsupported,
    #[error("transport.decompression_failed")]
    DecompressionFailed,
    #[error("transport.response_too_large")]
    ResponseTooLarge,
    #[error("network.timeout")]
    Timeout,
    #[error("{0}")]
    UserProxyFailure(Box<UserProxyFailureV1>),
    #[error("{error}")]
    RoutedTransport {
        #[source]
        error: Box<NetworkControlError>,
        routing: Vec<GatewayHopRouteV1>,
        /// True only if no hop in this logical request could have sent an HTTP request.
        request_not_started: bool,
    },
    #[error("network.proxy_unauthorized")]
    ProxyUnauthorized,
    #[error("network.proxy_protocol_error")]
    ProxyProtocol,
    #[error("network.tls_server_name_mismatch")]
    TlsServerNameMismatch,
    #[error("network.proxy_interrupted")]
    ProxyInterrupted,
    #[error("network.credential_binding_mismatch")]
    CredentialBindingMismatch,
    #[error("network.credential_unavailable")]
    CredentialUnavailable,
    #[error("network.credential_reflection_detected")]
    CredentialReflectionDetected { evidence: Box<CredentialReflectionScanEvidenceV1> },
    #[error("network.credential_reflection_scan_incomplete")]
    CredentialReflectionScanIncomplete {
        evidence: Box<CredentialReflectionScanEvidenceV1>,
        /// True only when the complete HTTP response body was received and
        /// the remaining failure is confined to deterministic scanner
        /// evidence construction. This is an internal classification fact;
        /// it is never serialized or exposed to the Renderer.
        response_terminal_known: bool,
    },
    #[error("network.transport_failed: {0}")]
    Transport(String),
    #[error("network.quarantine_io_failed: {0}")]
    QuarantineIo(#[from] std::io::Error),
    #[error("network.quarantine_integrity_failed")]
    QuarantineIntegrity,
    #[error("network.quarantine_not_found")]
    QuarantineNotFound,
    #[error("network.quarantine_release_not_approved")]
    QuarantineReleaseNotApproved,
    #[error(transparent)]
    Persistence(#[from] aura_persistence::PersistenceError),
}

#[derive(Clone, Debug)]
pub struct CredentialBinding {
    pub credential_ref: CredentialRef,
    pub origin: NormalizedOrigin,
    pub header_name: String,
}

pub struct ResolvedCredential {
    pub binding: CredentialBinding,
    pub secret: Vec<u8>,
}

impl Drop for ResolvedCredential {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
    }
}

pub trait CredentialResolver: Send + Sync {
    fn resolve(&self, reference: &CredentialRef)
    -> Result<ResolvedCredential, NetworkControlError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCredentialBinding {
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub credential_binding_fingerprint: CredentialBindingFingerprint,
    pub origin: NormalizedOrigin,
    pub header_name: String,
}

pub struct ResolvedProviderCredential {
    pub binding: ProviderCredentialBinding,
    pub secret_header_value: Vec<u8>,
}

impl Drop for ResolvedProviderCredential {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret_header_value.zeroize();
    }
}

pub trait ProviderCredentialResolver: Send + Sync {
    fn resolve(
        &self,
        binding: &ConfigurationRequestCredentialBindingV1,
    ) -> Result<ResolvedProviderCredential, NetworkControlError>;
}

#[allow(async_fn_in_trait)]
pub trait NetworkControlPlane {
    async fn authorize(
        &self,
        plan: NetworkRequestPlan,
    ) -> Result<AuthorizedNetworkRequest, NetworkControlError>;
}

#[derive(Clone, Debug)]
pub struct NetworkControlOptions {
    pub dns_timeout: Duration,
    pub rate_limits: RateLimitConfig,
    /// Shared so a running test gateway can install records without rebuilding clients.
    #[cfg(feature = "test-fixture-dns")]
    test_fixture_dns: std::sync::Arc<std::sync::Mutex<Option<fixture_dns::TestFixtureDns>>>,
    #[cfg(feature = "test-fixture-tls")]
    test_fixture_tls: std::sync::Arc<std::sync::Mutex<Option<TestFixtureTlsOptions>>>,
}

impl Default for NetworkControlOptions {
    fn default() -> Self {
        Self {
            dns_timeout: Duration::from_secs(5),
            rate_limits: RateLimitConfig::default(),
            #[cfg(feature = "test-fixture-dns")]
            test_fixture_dns: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(feature = "test-fixture-tls")]
            test_fixture_tls: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

#[cfg(feature = "test-fixture-tls")]
#[derive(Clone, Debug)]
struct TestFixtureTlsOptions {
    root_certificate_pem: Vec<u8>,
    accept_invalid_hostname: bool,
}

#[cfg(feature = "test-fixture-tls")]
impl NetworkControlOptions {
    /// Enables one local contract-test CA. The resulting pinned client ignores
    /// ambient proxy configuration so the exact loopback fixture remains
    /// isolated. Destination, origin, grant and TLS certificate-chain checks
    /// remain active.
    #[doc(hidden)]
    #[must_use]
    pub fn with_local_test_fixture_tls(
        self,
        root_certificate_pem: Vec<u8>,
        accept_invalid_hostname: bool,
    ) -> Self {
        *self.test_fixture_tls.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(TestFixtureTlsOptions { root_certificate_pem, accept_invalid_hostname });
        self
    }

    pub(crate) fn test_fixture_tls(&self) -> Option<TestFixtureTlsOptions> {
        self.test_fixture_tls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// Replaces the fixture CA on this options value, including one already owned by a gateway.
    #[doc(hidden)]
    pub fn install_test_fixture_tls(&self, root_certificate_pem: Vec<u8>) {
        *self.test_fixture_tls.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(TestFixtureTlsOptions { root_certificate_pem, accept_invalid_hostname: false });
    }
}

fn selected_grants(
    plan: &NetworkRequestPlan,
    store: &dyn NetworkGrantStore,
) -> Result<Vec<NetworkGrant>, NetworkControlError> {
    let mut grants = Vec::new();
    for grant_id in &plan.network_grant_ids {
        grants.push(store.get(*grant_id).ok_or(NetworkControlError::GrantNotApplicable)?);
    }
    Ok(grants)
}

fn grant_id_set(grants: &[NetworkGrant]) -> Vec<NetworkGrantId> {
    grants.iter().map(|grant| grant.id).collect()
}

pub use gateway::GatewayStartedHook;
