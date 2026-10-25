//! Real loopback HTTP/TLS/CONNECT/SOCKS exchanges. Core admission/persistence belongs to B3.
mod credentials;
mod failures;
mod mcp;
mod protocols;
mod provider;
mod proxy;
mod routes;
mod support;

use super::*;
use aura_tool_contract::{ToolNetworkProxyProtocolV1, ToolNetworkProxyV1};
use std::sync::Arc;
use support::*;

struct FixturePurpose {
    proxy: Option<ToolNetworkProxyV1>,
    ordinary: bool,
    hash: aura_domain_common::jcs::CanonicalSha256HashV1,
    credential: Option<aura_permission_domain::ToolCredentialUseV1>,
}
impl sealed::Sealed for FixturePurpose {}
impl GatewayGrantPurpose for FixturePurpose {
    fn validate_request(&self, request: &GatewayRequest) -> Result<(), NetworkControlError> {
        if crate::ordinary_tools::ordinary_request_hash(request, self.credential.as_ref())?
            != self.hash
        {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        Ok(())
    }
    fn is_general_tool(&self) -> bool {
        true
    }
    fn is_ordinary_tool(&self) -> bool {
        self.ordinary
    }
    fn frozen_user_proxy(&self) -> Option<&ToolNetworkProxyV1> {
        self.proxy.as_ref()
    }
    fn tool_credential_binding(&self) -> Option<&aura_permission_domain::ToolCredentialUseV1> {
        self.credential.as_ref()
    }
}

fn request(url: String) -> GatewayRequest {
    use aura_domain_common::{CorrelationId, NetworkRequestId, SessionId, WorkspaceId};
    GatewayRequest {
        plan: NetworkRequestPlan {
            request_id: NetworkRequestId::new(),
            caller: NetworkCallerKind::AuraTool,
            identity: aura_security_contract::InvocationIdentity::runtime_user(
                WorkspaceId::new(),
                SessionId::new(),
                CorrelationId::new(),
            ),
            method: "GET".into(),
            requested_url: url,
            redirect_policy: RedirectPolicy::ReauthorizeEachHop { max_hops: 5 },
            destination: None,
            data_classes: vec![NetworkDataClass::PublicQuery],
            request_data: vec![],
            byte_limits: aura_permission_domain::NetworkByteLimits {
                max_upload_bytes: 16_384,
                max_download_bytes: 16_384,
                max_decompressed_bytes: 16_384,
            },
            network_grant_ids: vec![],
            credential_refs: vec![],
            timeout_ms: 2_000,
        },
        headers: vec![],
        body: vec![],
    }
}

fn grant(
    request: &GatewayRequest,
    proxy: Option<ToolNetworkProxyV1>,
    ordinary: bool,
) -> CoreRequestGrant<FixturePurpose> {
    let url = Url::parse(&request.plan.requested_url).expect("url");
    CoreRequestGrant {
        logical_request_id: request.plan.request_id,
        purpose: FixturePurpose {
            proxy,
            ordinary,
            hash: crate::ordinary_tools::ordinary_request_hash(request, None).expect("hash"),
            credential: None,
        },
        caller: request.plan.caller,
        destination_policy: aura_permission_domain::NetworkDestinationPolicy {
            protocol: NetworkProtocolClass::Http,
            host: url.host_str().expect("host").into(),
            port: url.port_or_known_default().expect("port"),
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
        expires_at: aura_domain_common::UtcTimestamp::from_value(
            aura_domain_common::UtcTimestamp::now().value() + time::Duration::minutes(1),
        ),
    }
}

fn gateway(records: &[(&str, &[&str])]) -> CoreHttpClientGateway {
    let options = NetworkControlOptions::default()
        .with_local_test_fixture_tls(CERTIFICATE.to_vec(), false)
        .with_test_fixture_dns(
            records
                .iter()
                .map(|(host, addresses)| crate::TestFixtureDnsRecord {
                    host: (*host).into(),
                    classified_addresses: addresses
                        .iter()
                        .map(|ip| ip.parse().expect("classified IP"))
                        .collect(),
                    connect_addresses: vec!["127.0.0.1".parse().expect("loopback")],
                })
                .collect(),
        )
        .expect("local DNS fixture");
    CoreHttpClientGateway::new(Arc::new(crate::InMemoryNetworkGrantStore::default()), None, options)
}

async fn run(
    gateway: &CoreHttpClientGateway,
    request: GatewayRequest,
    proxy: Option<ToolNetworkProxyV1>,
    ordinary: bool,
) -> Result<GatewayResponse, NetworkControlError> {
    let grant = grant(&request, proxy, ordinary);
    let opened = gateway.open_response(grant, request, None, None).await?;
    gateway.collect_response(opened).await.map(|(response, _)| response)
}
