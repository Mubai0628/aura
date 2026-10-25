use super::*;
use reqwest::redirect::Policy;
use std::net::{IpAddr, SocketAddr};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct CoreClientCacheKey {
    scheme: String,
    host: String,
    port: u16,
    addresses: Vec<String>,
    route: GatewayRouteV1,
}

impl CoreClientCacheKey {
    pub(super) fn from_destination(destination: &ResolvedNetworkDestination) -> Self {
        let mut addresses = destination.addresses.clone();
        addresses.sort();
        addresses.dedup();
        Self {
            scheme: destination.scheme.clone(),
            host: destination.host.clone(),
            port: destination.port,
            addresses,
            route: GatewayRouteV1::Direct,
        }
    }
    pub(super) fn routed(
        destination: &ResolvedNetworkDestination,
        proxy: Option<&aura_tool_contract::ToolNetworkProxyV1>,
    ) -> Self {
        Self { route: GatewayRouteV1::from_proxy(proxy), ..Self::from_destination(destination) }
    }
}

#[derive(Clone)]
pub(super) enum HopClient {
    Direct(reqwest::Client),
    UserProxy(super::user_proxy::ProxyClient),
}
impl HopClient {
    pub(super) fn new(
        destination: &ResolvedNetworkDestination,
        request: &GatewayRequest,
        options: &NetworkControlOptions,
        ordinary: bool,
        proxy: Option<&aura_tool_contract::ToolNetworkProxyV1>,
    ) -> Result<Self, NetworkControlError> {
        match proxy {
            Some(proxy) => {
                super::user_proxy::ProxyClient::new(proxy.clone(), options).map(Self::UserProxy)
            }
            None => build_pinned_client(
                destination,
                request.plan.timeout_ms,
                Some(request.plan.timeout_ms),
                options,
                !ordinary,
            )
            .map(Self::Direct),
        }
    }
    pub(super) async fn send(
        &self,
        request: GatewayRequest,
        method: Method,
        headers: HeaderMap,
    ) -> Result<reqwest::Response, super::user_proxy::HopSendError> {
        match self {
            Self::UserProxy(client) => client.send(&request, method, headers).await,
            Self::Direct(client) => client
                .request(method, &request.plan.requested_url)
                .headers(headers)
                .body(request.body)
                .send()
                .await
                .map_err(|error| {
                    let request_not_started = error.is_connect();
                    super::user_proxy::HopSendError {
                        error: map_reqwest_error(error),
                        request_not_started,
                    }
                }),
        }
    }
}

pub(super) fn pinned_client(
    destination: &ResolvedNetworkDestination,
    connect_timeout_ms: u64,
    timeout_ms: Option<u64>,
    options: &NetworkControlOptions,
) -> Result<reqwest::Client, NetworkControlError> {
    build_pinned_client(destination, connect_timeout_ms, timeout_ms, options, true)
}

pub(super) fn build_pinned_client(
    destination: &ResolvedNetworkDestination,
    connect_timeout_ms: u64,
    timeout_ms: Option<u64>,
    options: &NetworkControlOptions,
    retries: bool,
) -> Result<reqwest::Client, NetworkControlError> {
    let addresses = destination
        .addresses
        .iter()
        .map(|address| {
            IpAddr::from_str(address)
                .map(|address| SocketAddr::new(address, destination.port))
                .map_err(|_| NetworkControlError::DestinationBlocked)
        })
        .collect::<Result<Vec<_>, _>>()?;
    #[cfg(feature = "test-fixture-dns")]
    let addresses = if let Some(fixture) = options.fixture_dns() {
        fixture.pinned(&destination.host, destination.port)?
    } else {
        addresses
    };
    let builder = reqwest::Client::builder()
        .redirect(Policy::none())
        // A process environment proxy must not replace the Core-authorized
        // destination or bypass address pinning. User-proxy hops have their own
        // explicit, frozen route and never enter this direct-client builder.
        .no_proxy()
        // The Gateway must observe encoded response bytes so the frozen wire
        // limit can be enforced independently from the decompressed limit.
        // Explicitly disable every reqwest auto-decoder to remain correct even
        // if Cargo feature unification enables one in another workspace crate.
        .no_gzip()
        .no_brotli()
        .no_zstd()
        .no_deflate()
        .connect_timeout(Duration::from_millis(connect_timeout_ms))
        .resolve_to_addrs(&destination.host, &addresses)
        .resolve_to_addrs(&format!("{}.", destination.host), &addresses);
    let builder = match timeout_ms {
        Some(ms) => builder.timeout(Duration::from_millis(ms)),
        None => builder,
    };
    let builder = if retries { builder } else { builder.retry(reqwest::retry::never()) };
    #[cfg(feature = "test-fixture-tls")]
    let builder = if let Some(fixture) = options.test_fixture_tls() {
        let certificate = reqwest::Certificate::from_pem(&fixture.root_certificate_pem)
            .map_err(|_| NetworkControlError::Transport("fixture CA is invalid".to_owned()))?;
        builder
            // Contract fixtures bind an exact loopback destination and must
            // remain isolated from ambient user or system proxy settings.
            .no_proxy()
            .add_root_certificate(certificate)
            .tls_danger_accept_invalid_hostnames(fixture.accept_invalid_hostname)
    } else {
        builder
    };
    let _ = options;
    builder
        .build()
        .map_err(|_| NetworkControlError::Transport("client construction failed".to_owned()))
}
