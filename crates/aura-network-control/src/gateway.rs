mod client;
mod ordinary;
mod user_proxy;
use client::{CoreClientCacheKey, HopClient, pinned_client};
pub use user_proxy::{
    GatewayHopRouteV1, GatewayRouteV1, UserProxyFailureKindV1, UserProxyFailureV1, probe_user_proxy,
};
mod request;
mod test_fixtures;
mod tools;
#[cfg(all(test, feature = "test-fixture-dns", feature = "test-fixture-tls"))]
mod user_proxy_tests;
pub use ordinary::{OrdinaryGatewayResponse, OrdinaryGatewayStream};
use std::{
    collections::BTreeMap,
    error::Error as StdError,
    fmt, io,
    pin::Pin,
    str::FromStr,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, SystemTime},
};

use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZlibDecoder, ZstdDecoder};
use aura_domain_common::ContentHash;
use aura_permission_domain::{
    AuthorizedNetworkRequest, CoreRequestGrant, CredentialReflectionScanEvidenceV1,
    CredentialReflectionSurfaceV1, NetworkProtocolClass, NetworkRequestPlan, NormalizedOrigin,
    RedirectPolicy,
};
use aura_security_contract::{
    NetworkCallerKind, NetworkDataClass, NetworkDestinationClass, ResolvedNetworkDestination,
};
use bytes::Bytes;
use futures_util::Stream;
use reqwest::{
    Method,
    header::{HeaderMap, HeaderName, HeaderValue, LOCATION},
};
use tokio::io::{AsyncRead, AsyncReadExt, BufReader};
use tokio_util::io::StreamReader;
use url::Url;

use crate::{
    ConfigurationRequestCredentialBindingV1, CredentialResolver, NetworkControlError,
    NetworkControlOptions, NetworkControlPlane, NetworkGrantStore, ProviderCredentialResolver,
    ProviderTransportAttemptGrant, ProviderTransportAuthorizationBinding,
    ProviderTransportRequestAuthority, RateLimiter, SecretScanner, StandardNetworkRequestGrant,
    ToolCredentialResolver, ToolHttpAttemptGrantV1, ToolHttpGatewayRequestV1,
    canonical_non_secret_headers_hash,
    credential_reflection_scan::CredentialReflectionScannerV1,
    grant::{GrantMatchQuery, matching_grants},
    grant_id_set, invocation_identity_binding_hash, selected_grants,
};

#[derive(Clone, Debug)]
pub struct GatewayRequest {
    pub plan: NetworkRequestPlan,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Immutable, non-secret HTTP request material bound into one Provider grant.
///
/// Unlike `NetworkRequestPlan`, this structure has no identity field. The
/// trusted authority travels separately and therefore configuration requests
/// never need a synthetic P06/P08 invocation identity.
#[derive(Clone, Debug)]
pub struct ProviderTransportImmutableRequest {
    pub request_id: aura_domain_common::NetworkRequestId,
    pub method: String,
    pub requested_url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub redirect_policy: RedirectPolicy,
    pub byte_limits: aura_permission_domain::NetworkByteLimits,
    pub connect_timeout_ms: u64,
    pub timeout_ms: Option<u64>,
}

/// The only accepted Provider-transport Gateway input.
///
/// The grant is a single authorization envelope. `authority` proves which
/// frozen identity path produced it, while `immutable_request` supplies the
/// exact bytes and target covered by its hashes.
pub struct ProviderTransportGatewayRequest {
    pub authority: ProviderTransportRequestAuthority,
    pub grant: CoreRequestGrant<ProviderTransportAttemptGrant>,
    pub immutable_request: ProviderTransportImmutableRequest,
}

/// Provider response observation used by configuration diagnostics.
///
/// Once a syntactically valid HTTP response exists, its status and sanitized
/// headers remain available even when Content-Encoding or body collection
/// fails. This lets Model Availability distinguish the HTTP layer from the
/// model-protocol layer without exposing raw response text.
pub struct ProviderTransportObservedResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub final_origin: NormalizedOrigin,
    pub final_destination: ResolvedNetworkDestination,
    pub redirect_hops: u8,
    pub body: Result<Vec<u8>, NetworkControlError>,
}

mod sealed {
    pub trait Sealed {}
}

mod body_stream;
pub use body_stream::GatewayBodyStream;
#[cfg(test)]
use body_stream::*;
mod purpose;
pub use purpose::GatewayGrantPurpose;

#[derive(Clone, Debug)]
pub struct GatewayResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub final_origin: NormalizedOrigin,
    pub final_destination: ResolvedNetworkDestination,
    pub redirect_hops: u8,
}

/// Gateway-parsed `Retry-After` evidence. Raw response Header values never
/// cross into P07 retry policy, and callers cannot manufacture this value
/// from a Package or Renderer DTO.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GatewayRetryAfterV1 {
    delay_ms: u64,
}

impl GatewayRetryAfterV1 {
    #[must_use]
    pub const fn delay_ms(self) -> u64 {
        self.delay_ms
    }
}

fn parse_gateway_retry_after(headers: &[(String, String)]) -> Option<GatewayRetryAfterV1> {
    parse_gateway_retry_after_at(headers, SystemTime::now())
}

fn parse_gateway_retry_after_at(
    headers: &[(String, String)],
    now: SystemTime,
) -> Option<GatewayRetryAfterV1> {
    let mut values = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
        .map(|(_, value)| value.trim());
    let value = values.next()?;
    if values.next().is_some() || value.is_empty() {
        return None;
    }
    let delay = if value.bytes().all(|byte| byte.is_ascii_digit()) {
        Duration::from_secs(value.parse::<u64>().ok()?)
    } else {
        httpdate::parse_http_date(value).ok()?.duration_since(now).unwrap_or_default()
    };
    let delay_ms = u64::try_from(delay.as_millis()).ok()?;
    Some(GatewayRetryAfterV1 { delay_ms })
}

/// A fully scanned P07 HTTP response. `None` means the exact request did not
/// inject a Tool credential; otherwise only validated `passed` evidence can
/// accompany the response body.
pub struct ToolHttpGatewayResponseV1 {
    pub response: GatewayResponse,
    pub credential_reflection: Option<CredentialReflectionScanEvidenceV1>,
    pub retry_after: Option<GatewayRetryAfterV1>,
}

pub struct GatewayStreamResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub final_origin: NormalizedOrigin,
    pub final_destination: ResolvedNetworkDestination,
    pub redirect_hops: u8,
    pub body: GatewayBodyStream,
}

struct OpenedGatewayResponse {
    final_url: String,
    routing: Vec<GatewayHopRouteV1>,
    redirect_chain: Vec<String>,
    status: u16,
    headers: Vec<(String, String)>,
    final_origin: NormalizedOrigin,
    final_destination: ResolvedNetworkDestination,
    redirect_hops: u8,
    response: reqwest::Response,
    rate_permit: crate::grant::RateLimitPermit,
    max_download_bytes: u64,
    max_decompressed_bytes: u64,
    credential_reflection: Option<CredentialReflectionScannerV1>,
    tool_success_status_ranges: Option<Vec<(u16, u16)>>,
}

/// What [`CoreHttpClientGateway::execute_started`] runs once the request has really begun.
///
/// It is an **awaitable** hook because Core's own hook records its request-start
/// journal line, and that write belongs on Core's bounded owner rather than on the reactor
/// thread polling the transport. The request future is held across the await; nothing about
/// the grant, the request or the byte accounting is replaced by it.
pub type GatewayStartedHook = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<(), NetworkControlError>> + Send + 'static>,
>;

pub struct CoreHttpClientGateway {
    grants: Arc<dyn NetworkGrantStore>,
    credentials: Option<Arc<dyn CredentialResolver>>,
    provider_credentials: Option<Arc<dyn ProviderCredentialResolver>>,
    tool_credentials: Option<Arc<dyn ToolCredentialResolver>>,
    options: NetworkControlOptions,
    limiter: RateLimiter,
    scanner: SecretScanner,
    transactions:
        Mutex<BTreeMap<aura_domain_common::NetworkRequestId, aura_domain_common::UtcTimestamp>>,
}

impl CoreHttpClientGateway {
    #[must_use]
    pub fn new(
        grants: Arc<dyn NetworkGrantStore>,
        credentials: Option<Arc<dyn CredentialResolver>>,
        options: NetworkControlOptions,
    ) -> Self {
        let limiter = RateLimiter::new(options.rate_limits.clone());
        Self {
            grants,
            credentials,
            provider_credentials: None,
            tool_credentials: None,
            options,
            limiter,
            scanner: SecretScanner,
            transactions: Mutex::new(BTreeMap::new()),
        }
    }

    #[must_use]
    pub fn with_provider_credentials(
        mut self,
        provider_credentials: Arc<dyn ProviderCredentialResolver>,
    ) -> Self {
        self.provider_credentials = Some(provider_credentials);
        self
    }

    #[must_use]
    pub fn with_tool_credentials(
        mut self,
        tool_credentials: Arc<dyn ToolCredentialResolver>,
    ) -> Self {
        self.tool_credentials = Some(tool_credentials);
        self
    }

    /// Executes one Provider request only after the Rust-only authority, the
    /// one-shot grant and every immutable request field agree exactly.
    pub async fn execute_provider_transport(
        &self,
        request: ProviderTransportGatewayRequest,
    ) -> Result<GatewayResponse, NetworkControlError> {
        let GatewayStreamResponse {
            status,
            headers,
            final_origin,
            final_destination,
            redirect_hops,
            mut body,
        } = self.execute_provider_transport_stream(request).await?;
        let mut collected = Vec::new();
        while let Some(chunk) = body.next_chunk().await? {
            collected.extend_from_slice(&chunk);
        }
        Ok(GatewayResponse {
            status,
            headers,
            body: collected,
            final_origin,
            final_destination,
            redirect_hops,
        })
    }

    /// Executes and fully collects a Provider response while preserving the
    /// fact that the HTTP response was received when body decoding fails.
    pub async fn execute_provider_transport_observed(
        &self,
        request: ProviderTransportGatewayRequest,
    ) -> Result<ProviderTransportObservedResponse, NetworkControlError> {
        let ProviderTransportGatewayRequest { authority, grant, immutable_request } = request;
        validate_provider_authority(&authority, &grant)?;
        validate_provider_immutable_request(&grant, &immutable_request)?;
        self.consume_core_request_grant(&grant)?;
        let OpenedGatewayResponse {
            status,
            headers,
            final_origin,
            final_destination,
            redirect_hops,
            response,
            rate_permit,
            max_download_bytes,
            max_decompressed_bytes,
            ..
        } = self.open_provider_response(&authority, &grant, immutable_request).await?;
        let body = match GatewayBodyStream::new(
            response,
            &headers,
            rate_permit,
            max_download_bytes,
            max_decompressed_bytes,
        ) {
            Ok(mut stream) => {
                let mut collected = Vec::new();
                loop {
                    match stream.next_chunk().await {
                        Ok(Some(chunk)) => collected.extend_from_slice(&chunk),
                        Ok(None) => break Ok(collected),
                        Err(error) => break Err(error),
                    }
                }
            }
            Err(error) => Err(error),
        };
        Ok(ProviderTransportObservedResponse {
            status,
            headers,
            final_origin,
            final_destination,
            redirect_hops,
            body,
        })
    }

    /// Streaming counterpart of `execute_provider_transport`.
    pub async fn execute_provider_transport_stream(
        &self,
        request: ProviderTransportGatewayRequest,
    ) -> Result<GatewayStreamResponse, NetworkControlError> {
        let ProviderTransportGatewayRequest { authority, grant, immutable_request } = request;
        validate_provider_authority(&authority, &grant)?;
        validate_provider_immutable_request(&grant, &immutable_request)?;
        self.consume_core_request_grant(&grant)?;
        let OpenedGatewayResponse {
            status,
            headers,
            final_origin,
            final_destination,
            redirect_hops,
            response,
            rate_permit,
            max_download_bytes,
            max_decompressed_bytes,
            ..
        } = self.open_provider_response(&authority, &grant, immutable_request).await?;
        let body = GatewayBodyStream::new(
            response,
            &headers,
            rate_permit,
            max_download_bytes,
            max_decompressed_bytes,
        )?;
        Ok(GatewayStreamResponse {
            status,
            headers,
            final_origin,
            final_destination,
            redirect_hops,
            body,
        })
    }

    async fn open_provider_response(
        &self,
        authority: &ProviderTransportRequestAuthority,
        grant: &CoreRequestGrant<ProviderTransportAttemptGrant>,
        mut request: ProviderTransportImmutableRequest,
    ) -> Result<OpenedGatewayResponse, NetworkControlError> {
        if u64::try_from(request.body.len()).unwrap_or(u64::MAX)
            > grant.byte_limits.max_upload_bytes
        {
            return Err(NetworkControlError::UploadLimitExceeded);
        }
        for (name, value) in &request.headers {
            self.scanner.scan_header(name, value)?;
        }
        if !matches!(authority, ProviderTransportRequestAuthority::AgentModel(_))
            && let Ok(text) = std::str::from_utf8(&request.body)
        {
            self.scanner.scan_text("request.body", text)?;
        }

        let original_method = request.method.clone();
        let mut redirect_hops = 0_u8;
        let mut clients: BTreeMap<CoreClientCacheKey, reqwest::Client> = BTreeMap::new();
        loop {
            let (_, origin, destination) =
                crate::resolver::normalize_with_options(&request.requested_url, &self.options)
                    .await?;
            validate_provider_destination(authority, grant, &destination, redirect_hops == 0)?;
            let rate_permit = self
                .limiter
                .acquire(&origin, u64::try_from(request.body.len()).unwrap_or(u64::MAX))?;
            let client_key = CoreClientCacheKey::from_destination(&destination);
            let client = if let Some(client) = clients.get(&client_key) {
                client.clone()
            } else {
                let client = pinned_client(
                    &destination,
                    request.connect_timeout_ms,
                    request.timeout_ms,
                    &self.options,
                )?;
                clients.insert(client_key, client.clone());
                client
            };
            let method = Method::from_bytes(request.method.as_bytes())
                .map_err(|_| NetworkControlError::InvalidMethod)?;
            let mut headers = HeaderMap::new();
            for (name, value) in &request.headers {
                let name =
                    HeaderName::from_str(name).map_err(|_| NetworkControlError::InvalidMethod)?;
                let value = HeaderValue::from_str(value).map_err(|_| {
                    NetworkControlError::SecretDetected {
                        location: "request.header".to_owned(),
                        kind: "invalid_header_value".to_owned(),
                    }
                })?;
                headers.append(name, value);
            }
            self.inject_provider_credential(
                grant.purpose.provider_credential_binding(),
                &origin,
                &mut headers,
            )?;
            let response = client
                .request(method, &request.requested_url)
                .headers(headers)
                .body(request.body.clone())
                .send()
                .await
                .map_err(map_reqwest_error)?;
            let status = response.status();
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned);

            if status.is_redirection() {
                let Some(location) = location else {
                    return Err(NetworkControlError::RedirectBlocked);
                };
                if !request.body.is_empty() || !matches!(original_method.as_str(), "GET" | "HEAD") {
                    return Err(NetworkControlError::RedirectBlocked);
                }
                let max_hops = match request.redirect_policy {
                    RedirectPolicy::Deny => return Err(NetworkControlError::RedirectBlocked),
                    RedirectPolicy::SameOrigin { max_hops }
                    | RedirectPolicy::ReauthorizeEachHop { max_hops } => max_hops,
                };
                redirect_hops = redirect_hops.saturating_add(1);
                if redirect_hops > max_hops {
                    return Err(NetworkControlError::RedirectLimitExceeded);
                }
                let current = Url::parse(&request.requested_url)
                    .map_err(|_| NetworkControlError::InvalidUrl)?;
                let next = current.join(&location).map_err(|_| NetworkControlError::InvalidUrl)?;
                if matches!(request.redirect_policy, RedirectPolicy::SameOrigin { .. }) {
                    let next_port =
                        next.port_or_known_default().ok_or(NetworkControlError::InvalidUrl)?;
                    if next.scheme() != origin.scheme
                        || next.host_str().is_none_or(|host| {
                            !host.trim_end_matches('.').eq_ignore_ascii_case(&origin.host)
                        })
                        || next_port != origin.port
                    {
                        return Err(NetworkControlError::RedirectBlocked);
                    }
                }
                request.requested_url = next.into();
                continue;
            }

            let response_headers = sanitized_response_headers(response.headers());
            if response
                .content_length()
                .is_some_and(|length| length > request.byte_limits.max_download_bytes)
            {
                return Err(NetworkControlError::ResponseTooLarge);
            }
            return Ok(OpenedGatewayResponse {
                final_url: request.requested_url.clone(),
                routing: vec![],
                redirect_chain: vec![],
                status: status.as_u16(),
                headers: response_headers,
                final_origin: origin,
                final_destination: destination,
                redirect_hops,
                response,
                rate_permit,
                max_download_bytes: request.byte_limits.max_download_bytes,
                max_decompressed_bytes: request.byte_limits.max_decompressed_bytes,
                credential_reflection: None,
                tool_success_status_ranges: None,
            });
        }
    }

    fn consume_core_request_grant<P>(
        &self,
        grant: &CoreRequestGrant<P>,
    ) -> Result<(), NetworkControlError> {
        self.consume_core_request_grant_at(grant, aura_domain_common::UtcTimestamp::now())
    }

    fn consume_core_request_grant_at<P>(
        &self,
        grant: &CoreRequestGrant<P>,
        now: aura_domain_common::UtcTimestamp,
    ) -> Result<(), NetworkControlError> {
        if grant.expires_at <= now {
            return Err(NetworkControlError::GrantExpired);
        }
        let mut transactions = self.transactions.lock().map_err(|_| {
            NetworkControlError::Transport("Core HTTP transaction store poisoned".to_owned())
        })?;
        transactions.retain(|_, expires_at| *expires_at > now);
        if transactions.insert(grant.logical_request_id, grant.expires_at).is_some() {
            return Err(NetworkControlError::CoreGrantAlreadyConsumed);
        }
        Ok(())
    }

    fn inject_credentials(
        &self,
        plan: &NetworkRequestPlan,
        origin: &NormalizedOrigin,
        headers: &mut HeaderMap,
    ) -> Result<(), NetworkControlError> {
        if plan.credential_refs.is_empty() {
            return Ok(());
        }
        let resolver =
            self.credentials.as_ref().ok_or(NetworkControlError::CredentialUnavailable)?;
        for reference in &plan.credential_refs {
            let credential = resolver.resolve(reference)?;
            if credential.binding.credential_ref != *reference
                || credential.binding.origin != *origin
            {
                return Err(NetworkControlError::CredentialBindingMismatch);
            }
            let name = HeaderName::from_str(&credential.binding.header_name)
                .map_err(|_| NetworkControlError::CredentialBindingMismatch)?;
            let value = HeaderValue::from_bytes(&credential.secret)
                .map_err(|_| NetworkControlError::CredentialBindingMismatch)?;
            headers.insert(name, value);
        }
        Ok(())
    }

    fn inject_provider_credential(
        &self,
        requested: Option<&ConfigurationRequestCredentialBindingV1>,
        origin: &NormalizedOrigin,
        headers: &mut HeaderMap,
    ) -> Result<(), NetworkControlError> {
        let Some(requested) = requested else {
            return Ok(());
        };
        let resolver =
            self.provider_credentials.as_ref().ok_or(NetworkControlError::CredentialUnavailable)?;
        let credential = resolver.resolve(requested)?;
        if credential.binding.credential_id != requested.credential_id
            || credential.binding.credential_revision != requested.credential_revision
            || credential.binding.credential_binding_fingerprint
                != requested.credential_binding_fingerprint
            || credential.binding.origin != *origin
            || !matches!(credential.binding.header_name.as_str(), "authorization" | "x-api-key")
        {
            return Err(NetworkControlError::CredentialBindingMismatch);
        }
        let name = HeaderName::from_str(&credential.binding.header_name)
            .map_err(|_| NetworkControlError::CredentialBindingMismatch)?;
        let value = HeaderValue::from_bytes(&credential.secret_header_value)
            .map_err(|_| NetworkControlError::CredentialBindingMismatch)?;
        headers.insert(name, value);
        Ok(())
    }

    fn inject_tool_credential(
        &self,
        requested: Option<&aura_permission_domain::ToolCredentialUseV1>,
        credentials: Option<&dyn ToolCredentialResolver>,
        origin: &NormalizedOrigin,
        headers: &mut HeaderMap,
        scanner: &mut Option<CredentialReflectionScannerV1>,
    ) -> Result<(), NetworkControlError> {
        let Some(requested) = requested else {
            return Ok(());
        };
        let expected_header_name = match &requested.placement {
            aura_permission_domain::ToolCredentialPlacementV1::BearerAuthorization => {
                "authorization"
            }
            aura_permission_domain::ToolCredentialPlacementV1::FixedHeader { header_name } => {
                header_name.as_str()
            }
            aura_permission_domain::ToolCredentialPlacementV1::Environment { .. } => {
                return Err(NetworkControlError::CredentialBindingMismatch);
            }
        };
        if headers.contains_key(expected_header_name) {
            return Err(NetworkControlError::CredentialBindingMismatch);
        }
        if scanner.is_none() {
            let resolver = credentials
                .or(self.tool_credentials.as_deref())
                .ok_or(NetworkControlError::CredentialUnavailable)?;
            let credential = resolver.resolve(requested)?;
            if credential.binding.credential_id != requested.credential_id
                || credential.binding.credential_revision != requested.credential_revision
                || credential.binding.origin != *origin
                || !credential.binding.header_name.eq_ignore_ascii_case(expected_header_name)
            {
                return Err(NetworkControlError::CredentialBindingMismatch);
            }
            *scanner = Some(CredentialReflectionScannerV1::new(
                credential.binding.credential_id,
                credential.binding.credential_revision,
                credential.binding.origin.clone(),
                &credential.secret_header_value,
            )?);
        }
        let scanner = scanner.as_ref().ok_or(NetworkControlError::CredentialUnavailable)?;
        if !scanner.matches_binding(requested.credential_id, requested.credential_revision, origin)
        {
            return Err(NetworkControlError::CredentialBindingMismatch);
        }
        let name = HeaderName::from_str(expected_header_name)
            .map_err(|_| NetworkControlError::CredentialBindingMismatch)?;
        let value = HeaderValue::from_bytes(scanner.injected_value())
            .map_err(|_| NetworkControlError::CredentialBindingMismatch)?;
        headers.insert(name, value);
        Ok(())
    }

    async fn authorize_with_context(
        &self,
        mut plan: NetworkRequestPlan,
        allow_ui_configuration_private_destination: bool,
        trusted_endpoint_content: bool,
    ) -> Result<AuthorizedNetworkRequest, NetworkControlError> {
        validate_method(&plan.method)?;
        if plan.timeout_ms == 0
            || plan.byte_limits.max_upload_bytes == 0
                && (plan.method != "GET" || !plan.request_data.is_empty())
            || plan.byte_limits.max_download_bytes == 0
            || plan.byte_limits.max_decompressed_bytes == 0
        {
            return Err(NetworkControlError::DestinationBlocked);
        }
        self.scanner.scan_plan(&plan)?;
        if plan.data_classes.contains(&NetworkDataClass::CredentialMaterial) {
            return Err(NetworkControlError::CredentialMaterialForbidden);
        }
        let (_, origin, resolved) =
            crate::resolver::normalize_with_options(&plan.requested_url, &self.options).await?;
        if plan.destination.as_ref().is_some_and(|expected| expected != &resolved) {
            return Err(NetworkControlError::DestinationBlocked);
        }
        let selected = selected_grants(&plan, self.grants.as_ref())?;
        let upload_bytes = estimated_upload_bytes(&plan);
        let matched = matching_grants(
            &selected,
            GrantMatchQuery {
                identity: &plan.identity,
                caller: plan.caller,
                origin: &origin,
                destination: &resolved,
                method: &plan.method,
                data_classes: &plan.data_classes,
                upload_bytes,
                download_limit: plan.byte_limits.max_download_bytes,
            },
        )?;
        if resolved.classes.iter().any(|class| matches!(class, NetworkDestinationClass::Metadata)) {
            return Err(NetworkControlError::DestinationBlocked);
        }
        let private_destination =
            resolved.classes.iter().any(|class| *class != NetworkDestinationClass::Public);
        let ui_configuration_private_allowed = allow_ui_configuration_private_destination
            && resolved.classes.iter().all(|class| {
                matches!(
                    class,
                    NetworkDestinationClass::Public
                        | NetworkDestinationClass::Private
                        | NetworkDestinationClass::Loopback
                )
            });
        let sensitive_egress = plan.data_classes.iter().any(|class| {
            matches!(
                class,
                NetworkDataClass::WorkspaceSource
                    | NetworkDataClass::UserFileContent
                    | NetworkDataClass::PersonalData
                    | NetworkDataClass::DiagnosticData
                    | NetworkDataClass::Unknown
            )
        }) && plan.caller != NetworkCallerKind::ModelTransport
            && !trusted_endpoint_content;
        let sandbox_requires_grant = plan.caller == NetworkCallerKind::SandboxProcess;
        if matched.is_empty() && private_destination && !ui_configuration_private_allowed {
            return Err(NetworkControlError::PrivateNetworkApprovalRequired);
        }
        if matched.is_empty() && (sensitive_egress || sandbox_requires_grant) {
            return Err(NetworkControlError::DataEgressApprovalRequired);
        }
        plan.destination = Some(resolved.clone());
        Ok(AuthorizedNetworkRequest {
            plan,
            destination: resolved,
            matched_grant_ids: grant_id_set(&matched),
        })
    }
}

impl NetworkControlPlane for CoreHttpClientGateway {
    async fn authorize(
        &self,
        plan: NetworkRequestPlan,
    ) -> Result<AuthorizedNetworkRequest, NetworkControlError> {
        self.authorize_with_context(plan, false, false).await
    }
}

fn validate_provider_transport_request(
    grant: &ProviderTransportAttemptGrant,
    request: &GatewayRequest,
) -> Result<(), NetworkControlError> {
    grant.validate_intrinsic().map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?;
    let url =
        Url::parse(&request.plan.requested_url).map_err(|_| NetworkControlError::InvalidUrl)?;
    let host = url.host_str().ok_or(NetworkControlError::InvalidUrl)?;
    let port = url.port_or_known_default().ok_or(NetworkControlError::InvalidUrl)?;
    let mut path_and_query = url.path().to_owned();
    if let Some(query) = url.query() {
        path_and_query.push('?');
        path_and_query.push_str(query);
    }
    let body_length = u64::try_from(request.body.len()).unwrap_or(u64::MAX);
    if request.plan.caller != NetworkCallerKind::ModelTransport
        || !request.plan.credential_refs.is_empty()
        || request.plan.method != grant.target.method
        || url.scheme() != grant.target.origin.scheme
        || !host.trim_end_matches('.').eq_ignore_ascii_case(&grant.target.origin.host)
        || port != grant.target.origin.port
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || path_and_query != grant.target.path_and_query
        || body_length != grant.request_body_length
        || aura_domain_common::ContentHash::sha256(&request.body) != grant.request_body_hash
        || canonical_non_secret_headers_hash(&request.headers)
            .map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?
            != grant.non_secret_headers_hash
        || request.plan.redirect_policy != grant.redirect_policy
        || request.plan.byte_limits != grant.byte_limits
    {
        return Err(NetworkControlError::CoreGrantBindingMismatch);
    }
    match &grant.authorization {
        ProviderTransportAuthorizationBinding::AgentModel {
            invocation_identity_binding_hash: expected,
            ..
        } => {
            if invocation_identity_binding_hash(&request.plan.identity)
                .map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?
                != *expected
            {
                return Err(NetworkControlError::CoreGrantBindingMismatch);
            }
        }
        ProviderTransportAuthorizationBinding::UserConfiguration { .. } => {
            if !request.plan.data_classes.is_empty() {
                return Err(NetworkControlError::CoreGrantBindingMismatch);
            }
        }
    }
    Ok(())
}

fn validate_provider_authority(
    authority: &ProviderTransportRequestAuthority,
    grant: &CoreRequestGrant<ProviderTransportAttemptGrant>,
) -> Result<(), NetworkControlError> {
    grant
        .purpose
        .validate_intrinsic()
        .map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?;
    match (authority, &grant.purpose.authorization, &grant.purpose.operation_context) {
        (
            ProviderTransportRequestAuthority::AgentModel(identity),
            ProviderTransportAuthorizationBinding::AgentModel {
                invocation_identity_binding_hash: expected_identity_hash,
                ..
            },
            aura_model_domain::TransportOperationContext::AgentModelAttempt { .. },
        ) if invocation_identity_binding_hash(identity)
            .is_ok_and(|actual| &actual == expected_identity_hash) =>
        {
            Ok(())
        }
        (
            ProviderTransportRequestAuthority::UserConfiguration(identity),
            ProviderTransportAuthorizationBinding::UserConfiguration {
                operation_kind,
                identity_binding_hash,
                models_config_revision,
            },
            aura_model_domain::TransportOperationContext::ModelDiscovery { .. }
            | aura_model_domain::TransportOperationContext::ModelAvailabilityTest { .. },
        ) if identity.operation_kind() == *operation_kind
            && identity.config_revision() == *models_config_revision
            && identity.target_endpoint_id() == grant.purpose.endpoint_id
            && identity.binding_hash().is_ok_and(|actual| &actual == identity_binding_hash) =>
        {
            Ok(())
        }
        _ => Err(NetworkControlError::ProviderTransportAuthorityContextMismatch),
    }
}

fn validate_provider_immutable_request(
    grant: &CoreRequestGrant<ProviderTransportAttemptGrant>,
    request: &ProviderTransportImmutableRequest,
) -> Result<(), NetworkControlError> {
    let url = Url::parse(&request.requested_url).map_err(|_| NetworkControlError::InvalidUrl)?;
    let host = url.host_str().ok_or(NetworkControlError::InvalidUrl)?;
    let port = url.port_or_known_default().ok_or(NetworkControlError::InvalidUrl)?;
    let mut path_and_query = url.path().to_owned();
    if let Some(query) = url.query() {
        path_and_query.push('?');
        path_and_query.push_str(query);
    }
    let body_length = u64::try_from(request.body.len()).unwrap_or(u64::MAX);
    if grant.logical_request_id != request.request_id
        || grant.caller != NetworkCallerKind::ModelTransport
        || grant.destination_policy.protocol != NetworkProtocolClass::Http
        || grant.purpose.target.method != request.method
        || grant.methods != [request.method.clone()]
        || grant.purpose.target.origin.scheme != url.scheme()
        || !host.trim_end_matches('.').eq_ignore_ascii_case(&grant.purpose.target.origin.host)
        || port != grant.purpose.target.origin.port
        || grant.destination_policy.host != grant.purpose.target.origin.host
        || grant.destination_policy.port != grant.purpose.target.origin.port
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || path_and_query != grant.purpose.target.path_and_query
        || body_length != grant.purpose.request_body_length
        || ContentHash::sha256(&request.body) != grant.purpose.request_body_hash
        || canonical_non_secret_headers_hash(&request.headers)
            .map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?
            != grant.purpose.non_secret_headers_hash
        || request.redirect_policy != grant.redirect_policy
        || request.redirect_policy != grant.purpose.redirect_policy
        || request.byte_limits != grant.byte_limits
        || request.byte_limits != grant.purpose.byte_limits
        || request.connect_timeout_ms == 0
        || request.timeout_ms == Some(0)
    {
        return Err(NetworkControlError::CoreGrantBindingMismatch);
    }
    Ok(())
}

fn validate_provider_destination(
    authority: &ProviderTransportRequestAuthority,
    grant: &CoreRequestGrant<ProviderTransportAttemptGrant>,
    destination: &ResolvedNetworkDestination,
    first_hop: bool,
) -> Result<(), NetworkControlError> {
    if destination.addresses.is_empty()
        || destination
            .classes
            .iter()
            .any(|class| !grant.destination_policy.allowed_destination_classes.contains(class))
        || destination.classes.contains(&NetworkDestinationClass::Metadata)
    {
        return Err(NetworkControlError::DestinationBlocked);
    }
    if first_hop
        && (!destination
            .host
            .trim_end_matches('.')
            .eq_ignore_ascii_case(&grant.destination_policy.host)
            || destination.port != grant.destination_policy.port)
    {
        return Err(NetworkControlError::CoreGrantBindingMismatch);
    }
    if matches!(grant.redirect_policy, RedirectPolicy::Deny | RedirectPolicy::SameOrigin { .. })
        && (!destination
            .host
            .trim_end_matches('.')
            .eq_ignore_ascii_case(&grant.destination_policy.host)
            || destination.port != grant.destination_policy.port)
    {
        return Err(NetworkControlError::RedirectBlocked);
    }
    let private_destination =
        destination.classes.iter().any(|class| *class != NetworkDestinationClass::Public);
    if private_destination
        && !matches!(authority, ProviderTransportRequestAuthority::UserConfiguration(_))
        && !grant.destination_policy.allowed_destination_classes.iter().any(|class| {
            matches!(class, NetworkDestinationClass::Private | NetworkDestinationClass::Loopback)
        })
    {
        return Err(NetworkControlError::PrivateNetworkApprovalRequired);
    }
    Ok(())
}

fn validate_core_request_binding<P>(
    grant: &CoreRequestGrant<P>,
    plan: &NetworkRequestPlan,
    first_hop: bool,
) -> Result<(), NetworkControlError> {
    if grant.logical_request_id != plan.request_id
        || grant.caller != plan.caller
        || grant.destination_policy.protocol != NetworkProtocolClass::Http
        || grant.redirect_policy != plan.redirect_policy
        || !grant.methods.iter().any(|method| method == &plan.method)
        || plan.data_classes.iter().any(|class| !grant.data_classes.contains(class))
        || plan.byte_limits.max_upload_bytes > grant.byte_limits.max_upload_bytes
        || plan.byte_limits.max_download_bytes > grant.byte_limits.max_download_bytes
        || plan.byte_limits.max_decompressed_bytes > grant.byte_limits.max_decompressed_bytes
    {
        return Err(NetworkControlError::CoreGrantBindingMismatch);
    }
    let url = Url::parse(&plan.requested_url).map_err(|_| NetworkControlError::InvalidUrl)?;
    let host = url.host_str().ok_or(NetworkControlError::InvalidUrl)?;
    let port = url.port_or_known_default().ok_or(NetworkControlError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(NetworkControlError::SchemeBlocked);
    }
    if first_hop
        && (!host.trim_end_matches('.').eq_ignore_ascii_case(&grant.destination_policy.host)
            || port != grant.destination_policy.port)
    {
        return Err(NetworkControlError::CoreGrantBindingMismatch);
    }
    Ok(())
}

fn validate_core_hop<P>(
    grant: &CoreRequestGrant<P>,
    authorized: &AuthorizedNetworkRequest,
) -> Result<(), NetworkControlError> {
    let destination = &authorized.destination;
    if destination.addresses.is_empty()
        || destination
            .classes
            .iter()
            .any(|class| !grant.destination_policy.allowed_destination_classes.contains(class))
    {
        return Err(NetworkControlError::CoreGrantBindingMismatch);
    }
    if matches!(grant.redirect_policy, RedirectPolicy::Deny | RedirectPolicy::SameOrigin { .. })
        && (!destination.host.eq_ignore_ascii_case(&grant.destination_policy.host)
            || destination.port != grant.destination_policy.port)
    {
        return Err(NetworkControlError::RedirectBlocked);
    }
    Ok(())
}

fn validate_method(method: &str) -> Result<(), NetworkControlError> {
    if method.is_empty()
        || method.len() > 16
        || !method.bytes().all(|byte| byte.is_ascii_uppercase() || byte == b'-')
    {
        Err(NetworkControlError::InvalidMethod)
    } else {
        Ok(())
    }
}

fn estimated_upload_bytes(plan: &NetworkRequestPlan) -> u64 {
    plan.request_data.iter().fold(0_u64, |total, item| {
        let bytes = match item {
            aura_permission_domain::NetworkDataReference::InlineText { value, .. }
            | aura_permission_domain::NetworkDataReference::Header { value, .. } => {
                u64::try_from(value.len()).unwrap_or(u64::MAX)
            }
            aura_permission_domain::NetworkDataReference::JsonBody { value } => {
                serde_json::to_vec(value)
                    .map(|value| u64::try_from(value.len()).unwrap_or(u64::MAX))
                    .unwrap_or(u64::MAX)
            }
            aura_permission_domain::NetworkDataReference::FileReference { byte_length, .. }
            | aura_permission_domain::NetworkDataReference::Artifact { byte_length, .. } => {
                *byte_length
            }
        };
        total.saturating_add(bytes)
    })
}

fn sanitized_response_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            let value = if matches!(
                name.as_str(),
                "set-cookie" | "www-authenticate" | "proxy-authenticate"
            ) {
                "<redacted>".to_owned()
            } else {
                value.to_str().unwrap_or("<non-utf8>").to_owned()
            };
            (name.as_str().to_owned(), value)
        })
        .collect()
}

fn map_reqwest_error(error: reqwest::Error) -> NetworkControlError {
    if error.is_timeout() {
        NetworkControlError::Timeout
    } else {
        NetworkControlError::Transport("request failed".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::{io, io::Write, time::UNIX_EPOCH};

    use aura_domain_common::{
        CorrelationId, CredentialRef, NetworkGrantId, NetworkRequestId, PermissionSnapshotId,
        SessionId, UtcTimestamp, WorkspaceId,
    };
    use aura_permission_domain::{
        AuthorizedNetworkRequest, CoreRequestGrant, NetworkByteLimits, NetworkDestinationPolicy,
        NetworkGrant, NetworkGrantIdentityScope, NetworkProtocolClass, RedirectPolicy,
    };
    use aura_security_contract::{
        InvocationIdentity, NetworkDataClass, NetworkDestinationClass, PermissionGrantLifetime,
        ResolvedNetworkDestination,
    };
    use flate2::{Compression, write::GzEncoder};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::{
        CredentialBinding, CredentialResolver, InMemoryNetworkGrantStore, ResolvedCredential,
    };

    #[test]
    fn retry_after_is_gateway_parsed_and_ambiguous_values_fail_closed() {
        assert_eq!(
            parse_gateway_retry_after_at(&[("Retry-After".to_owned(), "3".to_owned())], UNIX_EPOCH,),
            Some(GatewayRetryAfterV1 { delay_ms: 3_000 })
        );
        assert_eq!(
            parse_gateway_retry_after_at(
                &[("retry-after".to_owned(), "Thu, 01 Jan 1970 00:00:05 GMT".to_owned(),)],
                UNIX_EPOCH,
            ),
            Some(GatewayRetryAfterV1 { delay_ms: 5_000 })
        );
        assert_eq!(
            parse_gateway_retry_after_at(
                &[
                    ("Retry-After".to_owned(), "1".to_owned()),
                    ("retry-after".to_owned(), "2".to_owned()),
                ],
                UNIX_EPOCH,
            ),
            None
        );
    }

    struct StaticCredentialResolver {
        reference: CredentialRef,
        origin: NormalizedOrigin,
    }

    impl CredentialResolver for StaticCredentialResolver {
        fn resolve(
            &self,
            reference: &CredentialRef,
        ) -> Result<ResolvedCredential, NetworkControlError> {
            if reference != &self.reference {
                return Err(NetworkControlError::CredentialUnavailable);
            }
            Ok(ResolvedCredential {
                binding: CredentialBinding {
                    credential_ref: self.reference.clone(),
                    origin: self.origin.clone(),
                    header_name: "authorization".to_owned(),
                },
                secret: b"Bearer p04-exact-origin-canary".to_vec(),
            })
        }
    }

    fn plan(url: &str, classes: Vec<NetworkDataClass>) -> NetworkRequestPlan {
        let workspace_id = WorkspaceId::new();
        let session_id = SessionId::new();
        let mut identity =
            InvocationIdentity::runtime_user(workspace_id, session_id, CorrelationId::new());
        identity.permission_snapshot_id = Some(PermissionSnapshotId::new());
        NetworkRequestPlan {
            request_id: NetworkRequestId::new(),
            identity,
            caller: NetworkCallerKind::AuraTool,
            method: "GET".to_owned(),
            requested_url: url.to_owned(),
            destination: None,
            request_data: vec![],
            data_classes: classes,
            redirect_policy: RedirectPolicy::Deny,
            byte_limits: aura_permission_domain::NetworkByteLimits {
                max_upload_bytes: 1024,
                max_download_bytes: 1024,
                max_decompressed_bytes: 1024,
            },
            timeout_ms: 1_000,
            credential_refs: vec![],
            network_grant_ids: vec![],
        }
    }

    fn loopback_request_and_grant(
        store: &InMemoryNetworkGrantStore,
        actual_origin: &NormalizedOrigin,
        credential_ref: CredentialRef,
    ) -> (GatewayRequest, CoreRequestGrant<StandardNetworkRequestGrant>) {
        let mut request_plan = plan(
            &format!("{}://{}:{}/", actual_origin.scheme, actual_origin.host, actual_origin.port),
            vec![NetworkDataClass::PublicQuery],
        );
        let session_id = request_plan.identity.session_id.expect("session");
        let network_grant_id = NetworkGrantId::new();
        store
            .insert(NetworkGrant {
                id: network_grant_id,
                identity_scope: NetworkGrantIdentityScope::Session,
                identity_scope_id: session_id.to_string(),
                lifetime: PermissionGrantLifetime::Session,
                origins: vec![actual_origin.clone()],
                destination_classes: vec![NetworkDestinationClass::Loopback],
                methods: vec!["GET".to_owned()],
                allowed_data_classes: vec![NetworkDataClass::PublicQuery],
                max_upload_bytes: Some(1024),
                max_download_bytes: Some(1024),
                source_fingerprint: None,
                expires_at: None,
                revoked_at: None,
            })
            .expect("insert loopback network grant");
        request_plan.network_grant_ids.push(network_grant_id);
        request_plan.credential_refs.push(credential_ref);

        let grant = CoreRequestGrant {
            logical_request_id: request_plan.request_id,
            purpose: StandardNetworkRequestGrant,
            caller: request_plan.caller,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host: actual_origin.host.clone(),
                port: actual_origin.port,
                allowed_destination_classes: vec![NetworkDestinationClass::Loopback],
            },
            methods: vec!["GET".to_owned()],
            redirect_policy: request_plan.redirect_policy,
            data_classes: request_plan.data_classes.clone(),
            byte_limits: request_plan.byte_limits.clone(),
            expires_at: UtcTimestamp::from_value(
                UtcTimestamp::now().value() + time::Duration::minutes(1),
            ),
        };
        (GatewayRequest { plan: request_plan, headers: Vec::new(), body: Vec::new() }, grant)
    }

    fn loopback_request_and_grant_without_credential(
        store: &InMemoryNetworkGrantStore,
        actual_origin: &NormalizedOrigin,
        byte_limits: NetworkByteLimits,
    ) -> (GatewayRequest, CoreRequestGrant<StandardNetworkRequestGrant>) {
        let mut request_plan = plan(
            &format!("{}://{}:{}/", actual_origin.scheme, actual_origin.host, actual_origin.port),
            vec![NetworkDataClass::PublicQuery],
        );
        request_plan.byte_limits = byte_limits.clone();
        let session_id = request_plan.identity.session_id.expect("session");
        let network_grant_id = NetworkGrantId::new();
        store
            .insert(NetworkGrant {
                id: network_grant_id,
                identity_scope: NetworkGrantIdentityScope::Session,
                identity_scope_id: session_id.to_string(),
                lifetime: PermissionGrantLifetime::Session,
                origins: vec![actual_origin.clone()],
                destination_classes: vec![NetworkDestinationClass::Loopback],
                methods: vec!["GET".to_owned()],
                allowed_data_classes: vec![NetworkDataClass::PublicQuery],
                max_upload_bytes: Some(byte_limits.max_upload_bytes),
                max_download_bytes: Some(byte_limits.max_download_bytes),
                source_fingerprint: None,
                expires_at: None,
                revoked_at: None,
            })
            .expect("insert loopback grant");
        request_plan.network_grant_ids.push(network_grant_id);
        let grant = CoreRequestGrant {
            logical_request_id: request_plan.request_id,
            purpose: StandardNetworkRequestGrant,
            caller: request_plan.caller,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host: actual_origin.host.clone(),
                port: actual_origin.port,
                allowed_destination_classes: vec![NetworkDestinationClass::Loopback],
            },
            methods: vec!["GET".to_owned()],
            redirect_policy: request_plan.redirect_policy,
            data_classes: request_plan.data_classes.clone(),
            byte_limits: request_plan.byte_limits.clone(),
            expires_at: UtcTimestamp::from_value(
                UtcTimestamp::now().value() + time::Duration::minutes(1),
            ),
        };
        (GatewayRequest { plan: request_plan, headers: Vec::new(), body: Vec::new() }, grant)
    }

    async fn execute_loopback_encoded_response(
        encoded_body: Vec<u8>,
        content_encoding: &str,
        byte_limits: NetworkByteLimits,
    ) -> Result<GatewayResponse, NetworkControlError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let port = listener.local_addr().expect("listener address").port();
        let origin =
            NormalizedOrigin { scheme: "http".to_owned(), host: "127.0.0.1".to_owned(), port };
        let response_header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Encoding: {content_encoding}\r\nConnection: close\r\n\r\n",
            encoded_body.len()
        );
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept request");
            let mut request = vec![0_u8; 4096];
            let _ = socket.read(&mut request).await.expect("read request");
            socket.write_all(response_header.as_bytes()).await.expect("write headers");
            socket.write_all(&encoded_body).await.expect("write body");
            socket.shutdown().await.expect("shutdown response");
        });
        let store = Arc::new(InMemoryNetworkGrantStore::default());
        let (request, grant) =
            loopback_request_and_grant_without_credential(&store, &origin, byte_limits);
        let gateway = CoreHttpClientGateway::new(store, None, NetworkControlOptions::default());
        let result = gateway.execute(grant, request).await;
        server.await.expect("server task");
        result
    }

    async fn execute_loopback_raw_http(
        response_bytes: Vec<u8>,
        byte_limits: NetworkByteLimits,
    ) -> Result<GatewayResponse, NetworkControlError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let port = listener.local_addr().expect("listener address").port();
        let origin =
            NormalizedOrigin { scheme: "http".to_owned(), host: "127.0.0.1".to_owned(), port };
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept request");
            let mut request = vec![0_u8; 4096];
            let _ = socket.read(&mut request).await.expect("read request");
            socket.write_all(&response_bytes).await.expect("write response");
            socket.shutdown().await.expect("shutdown response");
        });
        let store = Arc::new(InMemoryNetworkGrantStore::default());
        let (request, grant) =
            loopback_request_and_grant_without_credential(&store, &origin, byte_limits);
        let gateway = CoreHttpClientGateway::new(store, None, NetworkControlOptions::default());
        let result = gateway.execute(grant, request).await;
        server.await.expect("server task");
        result
    }

    #[tokio::test]
    async fn execution_start_notification_precedes_response_and_does_not_cancel_body() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let origin = NormalizedOrigin {
            scheme: "http".into(),
            host: "127.0.0.1".into(),
            port: listener.local_addr().expect("address").port(),
        };
        let (release, released) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("request");
            let mut bytes = [0; 4096];
            let received = socket.read(&mut bytes).await.expect("request bytes");
            assert!(received > 0, "actual request reached the local peer");
            released.await.expect("release response after start");
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone")
                .await
                .expect("response");
        });
        let store = Arc::new(InMemoryNetworkGrantStore::default());
        let (request, grant) = loopback_request_and_grant_without_credential(
            &store,
            &origin,
            NetworkByteLimits {
                max_upload_bytes: 1024,
                max_download_bytes: 1024,
                max_decompressed_bytes: 1024,
            },
        );
        let gateway = CoreHttpClientGateway::new(store, None, NetworkControlOptions::default());
        let (started, notice) = tokio::sync::oneshot::channel();
        let execution = gateway.execute_started(grant, request, || {
            Box::pin(async move {
                started.send(()).expect("start receiver");
                Ok(())
            })
        });
        tokio::pin!(execution);
        tokio::select! {
            result = &mut execution => panic!("response cannot finish before release: {result:?}"),
            result = notice => result.expect("actual request start"),
            _ = tokio::time::sleep(Duration::from_secs(5)) => panic!("start was not notified"),
        }
        release.send(()).expect("release");
        assert_eq!(execution.await.expect("existing stream completes").body, b"done");
        server.await.expect("server");
    }

    #[test]
    fn content_encoding_v1_rejects_unknown_empty_and_layered_values() {
        assert_eq!(
            ResponseContentCoding::parse(&[]).expect("missing encoding"),
            ResponseContentCoding::Identity
        );
        assert_eq!(
            ResponseContentCoding::parse(&[("Content-Encoding".to_owned(), " GZip \t".to_owned())])
                .expect("case-insensitive gzip"),
            ResponseContentCoding::Gzip
        );
        for headers in [
            vec![("content-encoding".to_owned(), "gzip, br".to_owned())],
            vec![
                ("content-encoding".to_owned(), "gzip".to_owned()),
                ("Content-Encoding".to_owned(), "gzip".to_owned()),
            ],
            vec![("content-encoding".to_owned(), "".to_owned())],
            vec![("content-encoding".to_owned(), "compress".to_owned())],
        ] {
            assert!(matches!(
                ResponseContentCoding::parse(&headers),
                Err(NetworkControlError::ContentEncodingUnsupported)
            ));
        }
    }

    #[tokio::test]
    async fn gateway_streams_gzip_with_independent_wire_and_decompressed_limits() {
        let plain = b"provider-response".repeat(2048);
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&plain).expect("compress fixture");
        let compressed = encoder.finish().expect("finish gzip");
        let allowed = NetworkByteLimits {
            max_upload_bytes: 1024,
            max_download_bytes: u64::try_from(compressed.len()).expect("wire length"),
            max_decompressed_bytes: u64::try_from(plain.len()).expect("plain length"),
        };
        let response = execute_loopback_encoded_response(compressed.clone(), "gzip", allowed)
            .await
            .expect("decode gzip response");
        assert_eq!(response.body, plain);

        let too_small = NetworkByteLimits {
            max_upload_bytes: 1024,
            max_download_bytes: u64::try_from(compressed.len()).expect("wire length"),
            max_decompressed_bytes: u64::try_from(plain.len() - 1).expect("plain length"),
        };
        assert!(matches!(
            execute_loopback_encoded_response(compressed, "gzip", too_small).await,
            Err(NetworkControlError::ResponseTooLarge)
        ));
    }

    #[tokio::test]
    async fn gateway_rejects_truncated_known_encoding() {
        let limits = NetworkByteLimits {
            max_upload_bytes: 1024,
            max_download_bytes: 1024,
            max_decompressed_bytes: 1024,
        };
        assert!(matches!(
            execute_loopback_encoded_response(vec![0x1f, 0x8b, 0x08], "gzip", limits).await,
            Err(NetworkControlError::DecompressionFailed)
        ));
    }

    #[test]
    fn map_body_read_error_preserves_wire_limit_through_io_error_other() {
        let error = io::Error::other(WireBodyReadError::ResponseTooLarge);
        assert!(matches!(
            map_body_read_error(&error, ResponseContentCoding::Identity),
            NetworkControlError::ResponseTooLarge
        ));
        let truncated = io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed");
        assert!(matches!(
            map_body_read_error(&truncated, ResponseContentCoding::Identity),
            NetworkControlError::Transport(_)
        ));
    }

    #[tokio::test]
    async fn gateway_rejects_known_length_response_over_download_limit() {
        let body = vec![b'A'; 128];
        let mut wire = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        wire.extend_from_slice(&body);
        let limits = NetworkByteLimits {
            max_upload_bytes: 1024,
            max_download_bytes: 64,
            max_decompressed_bytes: 1024,
        };
        assert!(matches!(
            execute_loopback_raw_http(wire, limits).await,
            Err(NetworkControlError::ResponseTooLarge)
        ));
    }

    #[tokio::test]
    async fn gateway_rejects_chunked_response_over_download_limit() {
        let chunk = vec![b'B'; 64];
        let mut wire =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n40\r\n"
                .to_vec();
        wire.extend_from_slice(&chunk);
        wire.extend_from_slice(b"\r\n0\r\n\r\n");
        let limits = NetworkByteLimits {
            max_upload_bytes: 1024,
            max_download_bytes: 32,
            max_decompressed_bytes: 1024,
        };
        let error = execute_loopback_raw_http(wire, limits)
            .await
            .expect_err("chunked body over the download limit");
        assert!(
            matches!(error, NetworkControlError::ResponseTooLarge),
            "chunked over-limit must be ResponseTooLarge, got {error:?}"
        );
    }

    #[tokio::test]
    async fn gateway_denies_real_http_redirect() {
        let wire = b"HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec();
        let limits = NetworkByteLimits {
            max_upload_bytes: 1024,
            max_download_bytes: 1024,
            max_decompressed_bytes: 1024,
        };
        assert!(matches!(
            execute_loopback_raw_http(wire, limits).await,
            Err(NetworkControlError::RedirectBlocked)
        ));
    }

    #[tokio::test]
    async fn gateway_classifies_truncated_identity_body_as_transport_not_limit() {
        let advertised = 1000_usize;
        let sent = vec![b'C'; 16];
        let mut wire =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {advertised}\r\nConnection: close\r\n\r\n")
                .into_bytes();
        wire.extend_from_slice(&sent);
        let limits = NetworkByteLimits {
            max_upload_bytes: 1024,
            max_download_bytes: 1024,
            max_decompressed_bytes: 1024,
        };
        let error =
            execute_loopback_raw_http(wire, limits).await.expect_err("truncated identity body");
        assert!(
            matches!(error, NetworkControlError::Transport(_)),
            "truncated body must stay Transport, got {error:?}"
        );
        assert!(
            !matches!(
                error,
                NetworkControlError::ResponseTooLarge | NetworkControlError::DownloadLimitExceeded
            ),
            "truncated body must not be classified as a download limit"
        );
    }

    #[tokio::test]
    async fn credential_binding_rejects_scheme_host_and_port_mismatch_before_http_send() {
        for mismatch in ["scheme", "host", "port"] {
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind no-send listener");
            let port = listener.local_addr().expect("listener address").port();
            let actual_origin =
                NormalizedOrigin { scheme: "http".to_owned(), host: "127.0.0.1".to_owned(), port };
            let binding_origin = match mismatch {
                "scheme" => {
                    NormalizedOrigin { scheme: "https".to_owned(), ..actual_origin.clone() }
                }
                "host" => {
                    NormalizedOrigin { host: "localhost".to_owned(), ..actual_origin.clone() }
                }
                "port" => NormalizedOrigin {
                    port: if port == u16::MAX { port - 1 } else { port + 1 },
                    ..actual_origin.clone()
                },
                _ => unreachable!("fixed mismatch cases"),
            };
            let credential_ref =
                CredentialRef::for_test("p04-origin", mismatch).expect("credential ref");
            let store = Arc::new(InMemoryNetworkGrantStore::default());
            let (request, grant) =
                loopback_request_and_grant(&store, &actual_origin, credential_ref.clone());
            let resolver = Arc::new(StaticCredentialResolver {
                reference: credential_ref,
                origin: binding_origin,
            });
            let gateway =
                CoreHttpClientGateway::new(store, Some(resolver), NetworkControlOptions::default());

            let mut headers = HeaderMap::new();
            assert!(matches!(
                gateway.inject_credentials(&request.plan, &actual_origin, &mut headers),
                Err(NetworkControlError::CredentialBindingMismatch)
            ));
            assert!(headers.is_empty(), "{mismatch} mismatch injected a credential header");

            let logical_request_id = grant.logical_request_id;
            assert!(matches!(
                gateway.execute(grant.clone(), request.clone()).await,
                Err(NetworkControlError::CredentialBindingMismatch)
            ));
            assert!(
                gateway
                    .transactions
                    .lock()
                    .expect("transaction store")
                    .contains_key(&logical_request_id),
                "{mismatch} mismatch did not retain the consumed logical transaction"
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(100), listener.accept()).await.is_err(),
                "{mismatch} mismatch sent an HTTP request"
            );
            assert!(matches!(
                gateway.execute(grant, request).await,
                Err(NetworkControlError::CoreGrantAlreadyConsumed)
            ));
        }
    }

    #[tokio::test]
    async fn public_read_is_allowed_but_loopback_and_workspace_egress_require_grants() {
        let store = Arc::new(InMemoryNetworkGrantStore::default());
        let gateway = CoreHttpClientGateway::new(store, None, NetworkControlOptions::default());
        gateway
            .authorize(plan("https://8.8.8.8/", vec![NetworkDataClass::PublicQuery]))
            .await
            .expect("public query");
        assert!(matches!(
            gateway.authorize(plan("http://127.0.0.1/", vec![NetworkDataClass::PublicQuery])).await,
            Err(NetworkControlError::PrivateNetworkApprovalRequired)
        ));
        assert!(matches!(
            gateway
                .authorize(plan("https://8.8.8.8/", vec![NetworkDataClass::WorkspaceSource]))
                .await,
            Err(NetworkControlError::DataEgressApprovalRequired)
        ));
    }

    #[tokio::test]
    async fn trusted_ui_configuration_allows_exact_private_target_but_never_metadata() {
        let store = Arc::new(InMemoryNetworkGrantStore::default());
        let gateway = CoreHttpClientGateway::new(store, None, NetworkControlOptions::default());
        gateway
            .authorize_with_context(plan("http://127.0.0.1/", Vec::new()), true, false)
            .await
            .expect("trusted UI configuration may reach its exact loopback endpoint");
        assert!(matches!(
            gateway
                .authorize_with_context(plan("http://169.254.169.254/", Vec::new()), true, false)
                .await,
            Err(NetworkControlError::DestinationBlocked)
        ));
    }

    #[tokio::test]
    async fn exact_session_grant_can_allow_loopback_but_not_a_different_origin() {
        let store = Arc::new(InMemoryNetworkGrantStore::default());
        let mut request = plan("http://127.0.0.1:8080/", vec![NetworkDataClass::PublicQuery]);
        let session_id = request.identity.session_id.expect("session");
        let grant_id = NetworkGrantId::new();
        store
            .insert(NetworkGrant {
                id: grant_id,
                identity_scope: NetworkGrantIdentityScope::Session,
                identity_scope_id: session_id.to_string(),
                lifetime: PermissionGrantLifetime::Session,
                origins: vec![NormalizedOrigin {
                    scheme: "http".to_owned(),
                    host: "127.0.0.1".to_owned(),
                    port: 8080,
                }],
                destination_classes: vec![NetworkDestinationClass::Loopback],
                methods: vec!["GET".to_owned()],
                allowed_data_classes: vec![NetworkDataClass::PublicQuery],
                max_upload_bytes: Some(1024),
                max_download_bytes: Some(1024),
                source_fingerprint: None,
                expires_at: None,
                revoked_at: None,
            })
            .expect("insert grant");
        request.network_grant_ids.push(grant_id);
        let gateway = CoreHttpClientGateway::new(store, None, NetworkControlOptions::default());
        gateway.authorize(request.clone()).await.expect("loopback grant");
        request.requested_url = "http://127.0.0.1:8081/".to_owned();
        assert!(matches!(
            gateway.authorize(request).await,
            Err(NetworkControlError::PrivateNetworkApprovalRequired)
        ));
    }

    #[test]
    fn core_request_grant_creates_exactly_one_logical_transaction() {
        let store = Arc::new(InMemoryNetworkGrantStore::default());
        let gateway = CoreHttpClientGateway::new(store, None, NetworkControlOptions::default());
        let request = plan("https://8.8.8.8/", vec![NetworkDataClass::PublicQuery]);
        let grant = CoreRequestGrant {
            logical_request_id: request.request_id,
            purpose: StandardNetworkRequestGrant,
            caller: request.caller,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host: "8.8.8.8".to_owned(),
                port: 443,
                allowed_destination_classes: vec![NetworkDestinationClass::Public],
            },
            methods: vec!["GET".to_owned()],
            redirect_policy: request.redirect_policy,
            data_classes: request.data_classes.clone(),
            byte_limits: request.byte_limits.clone(),
            expires_at: UtcTimestamp::from_value(
                UtcTimestamp::now().value() + time::Duration::minutes(1),
            ),
        };

        gateway.consume_core_request_grant(&grant).expect("first logical transaction creation");
        assert!(matches!(
            gateway.consume_core_request_grant(&grant),
            Err(NetworkControlError::CoreGrantAlreadyConsumed)
        ));
    }

    #[test]
    fn unconsumed_grant_ttl_is_exclusive_at_thirty_seconds() {
        let store = Arc::new(InMemoryNetworkGrantStore::default());
        let gateway = CoreHttpClientGateway::new(store, None, NetworkControlOptions::default());
        let issued_at = UtcTimestamp::now();
        let expires_at = UtcTimestamp::from_value(issued_at.value() + time::Duration::seconds(30));
        let grant = CoreRequestGrant {
            logical_request_id: NetworkRequestId::new(),
            purpose: StandardNetworkRequestGrant,
            caller: NetworkCallerKind::AuraTool,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host: "8.8.8.8".to_owned(),
                port: 443,
                allowed_destination_classes: vec![NetworkDestinationClass::Public],
            },
            methods: vec!["GET".to_owned()],
            redirect_policy: RedirectPolicy::Deny,
            data_classes: vec![NetworkDataClass::PublicQuery],
            byte_limits: NetworkByteLimits {
                max_upload_bytes: 1024,
                max_download_bytes: 1024,
                max_decompressed_bytes: 1024,
            },
            expires_at,
        };
        let before_deadline =
            UtcTimestamp::from_value(issued_at.value() + time::Duration::milliseconds(29_999));
        gateway
            .consume_core_request_grant_at(&grant, before_deadline)
            .expect("29.999 seconds remains consumable");

        let expired = CoreRequestGrant { logical_request_id: NetworkRequestId::new(), ..grant };
        assert!(matches!(
            gateway.consume_core_request_grant_at(&expired, expires_at),
            Err(NetworkControlError::GrantExpired)
        ));
    }

    #[test]
    fn core_request_grant_rechecks_binding_and_each_redirect_destination() {
        let mut request = plan("https://8.8.8.8/", vec![NetworkDataClass::PublicQuery]);
        request.redirect_policy = RedirectPolicy::ReauthorizeEachHop { max_hops: 5 };
        let grant = CoreRequestGrant {
            logical_request_id: request.request_id,
            purpose: StandardNetworkRequestGrant,
            caller: request.caller,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host: "8.8.8.8".to_owned(),
                port: 443,
                allowed_destination_classes: vec![NetworkDestinationClass::Public],
            },
            methods: vec!["GET".to_owned()],
            redirect_policy: request.redirect_policy,
            data_classes: request.data_classes.clone(),
            byte_limits: NetworkByteLimits {
                max_upload_bytes: 1024,
                max_download_bytes: 1024,
                max_decompressed_bytes: 1024,
            },
            expires_at: UtcTimestamp::from_value(
                UtcTimestamp::now().value() + time::Duration::minutes(1),
            ),
        };
        validate_core_request_binding(&grant, &request, true).expect("initial binding");

        let redirected = AuthorizedNetworkRequest {
            plan: request.clone(),
            destination: ResolvedNetworkDestination {
                scheme: "https".to_owned(),
                host: "cdn.example.test".to_owned(),
                port: 443,
                addresses: vec!["8.8.4.4".to_owned()],
                classes: vec![NetworkDestinationClass::Public],
            },
            matched_grant_ids: vec![],
        };
        validate_core_hop(&grant, &redirected).expect("approved public redirect hop");

        let mut private_redirect = redirected;
        private_redirect.destination.addresses = vec!["10.0.0.1".to_owned()];
        private_redirect.destination.classes = vec![NetworkDestinationClass::Private];
        assert!(matches!(
            validate_core_hop(&grant, &private_redirect),
            Err(NetworkControlError::CoreGrantBindingMismatch)
        ));

        request.method = "POST".to_owned();
        assert!(matches!(
            validate_core_request_binding(&grant, &request, true),
            Err(NetworkControlError::CoreGrantBindingMismatch)
        ));
    }
}
