use super::proxy::{Behavior, proxy};
use super::*;
use crate::{
    AuthenticatedConfigurationChannelIdentity, ConfigurationOperationKind,
    ConfigurationOperationRequestManifestV1, ConfigurationRequestDataClass,
    ConfigurationRequestPurpose, ConfigurationRequestTargetV1, ConfigurationResponseMediaPolicyV1,
    ModelProxySource, UserConfigurationOperationIdentity,
};
use aura_domain_common::{
    CommandInvocationId, CorrelationId, EndpointId, IpcChannelId, ModelDiscoveryRunId,
    NetworkRequestId, Revision, RuntimeInstanceId, TransportAttemptId, UtcTimestamp,
};

/// A model discovery request to `url`, the way the model settings issue one.
fn discovery(url: &str, host: &str, port: u16) -> ProviderTransportGatewayRequest {
    let identity = UserConfigurationOperationIdentity::from_authenticated_settings_command(
        CommandInvocationId::new(),
        CorrelationId::new(),
        AuthenticatedConfigurationChannelIdentity {
            core_runtime_instance_id: RuntimeInstanceId::new(),
            authenticated_main_runtime_instance_id: RuntimeInstanceId::new(),
            authenticated_channel_id: IpcChannelId::new(),
        },
        ConfigurationOperationKind::ModelDiscovery,
        EndpointId::new(),
        Revision::INITIAL,
        UtcTimestamp::now(),
    );
    let discovery_run_id = ModelDiscoveryRunId::new();
    let manifest = ConfigurationOperationRequestManifestV1 {
        schema_version: crate::CONFIGURATION_MANIFEST_SCHEMA_VERSION,
        command_invocation_id: identity.command_invocation_id(),
        correlation_id: identity.correlation_id(),
        user_configuration_operation_identity_binding_hash: identity
            .binding_hash()
            .expect("identity hash"),
        purpose: ConfigurationRequestPurpose::ModelDiscovery {
            model_discovery_run_id: discovery_run_id,
        },
        transport_attempt_id: TransportAttemptId::new(),
        endpoint_id: identity.target_endpoint_id(),
        models_config_revision: identity.config_revision(),
        endpoint_security_fingerprint: ContentHash::blake3(b"local fixture endpoint"),
        target: ConfigurationRequestTargetV1 {
            method: "GET".into(),
            origin: NormalizedOrigin { scheme: "https".into(), host: host.into(), port },
            path_and_query: "/resource".into(),
        },
        non_secret_headers_hash: crate::canonical_non_secret_headers_hash(&[])
            .expect("headers hash"),
        request_body_hash: ContentHash::sha256(b""),
        request_body_length: 0,
        credential_binding: None,
        max_request_bytes: crate::ConfigurationTransportPolicyV1::MODEL_DISCOVERY
            .max_request_body_bytes,
        max_response_bytes: crate::ConfigurationTransportPolicyV1::MODEL_DISCOVERY
            .max_response_wire_bytes,
        max_decompressed_response_bytes: crate::ConfigurationTransportPolicyV1::MODEL_DISCOVERY
            .max_response_decompressed_bytes,
        redirect_policy: RedirectPolicy::SameOrigin {
            max_hops: crate::CONFIGURATION_REDIRECT_MAX_HOPS,
        },
        response_media_policy: ConfigurationResponseMediaPolicyV1 {
            accepted_media_types: std::collections::BTreeSet::from(["application/json".into()]),
            require_content_type: true,
        },
        data_class: ConfigurationRequestDataClass::ProtocolMetadata,
    };
    let request_id = NetworkRequestId::new();
    let grant = crate::issue_configuration_provider_transport_grant(
        &identity,
        aura_model_domain::TransportOperationContext::ModelDiscovery { discovery_run_id },
        &manifest,
        request_id,
        UtcTimestamp::now(),
    )
    .expect("real provider grant");
    let byte_limits = grant.byte_limits.clone();
    ProviderTransportGatewayRequest {
        authority: ProviderTransportRequestAuthority::UserConfiguration(identity),
        grant,
        immutable_request: ProviderTransportImmutableRequest {
            request_id,
            method: "GET".into(),
            requested_url: url.into(),
            headers: vec![],
            body: vec![],
            redirect_policy: manifest.redirect_policy,
            byte_limits,
            connect_timeout_ms: 1000,
            timeout_ms: Some(2000),
        },
    }
}

/// The proxy model requests take, as the settings would name it.
struct Named(Option<ToolNetworkProxyV1>);
impl ModelProxySource for Named {
    fn model_proxy(&self) -> Option<ToolNetworkProxyV1> {
        self.0.clone()
    }
}

#[tokio::test]
async fn real_provider_gateway_keeps_direct_transport_after_a_proxied_tool_on_the_same_gateway() {
    let target = target(true, vec![Reply::ok()]).await;
    let proxy = proxy(ToolNetworkProxyProtocolV1::Http, Behavior::Relay, &[target.address]).await;
    let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
    let url = target.url("https", "proxy-target.test");
    run(&gateway, request(url.clone()), Some(proxy.settings.clone()), true)
        .await
        .expect("proxied tool");
    let response = gateway
        .execute_provider_transport(discovery(&url, "proxy-target.test", target.address.port()))
        .await
        .expect("direct provider fixture");
    assert_eq!(response.body, b"local proxy fixture");
    assert_eq!(proxy.destinations.lock().expect("tool only").len(), 1);
    assert_eq!(target.requests.lock().expect("tool and provider").len(), 2);
}

#[tokio::test]
async fn a_model_request_goes_through_the_proxy_its_source_names() {
    for protocol in [ToolNetworkProxyProtocolV1::Http, ToolNetworkProxyProtocolV1::Socks5] {
        let target = target(true, vec![Reply::ok()]).await;
        let proxy = proxy(protocol, Behavior::Relay, &[target.address]).await;
        let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])])
            .with_model_proxy(Arc::new(Named(Some(proxy.settings.clone()))));
        let url = target.url("https", "proxy-target.test");
        let response = gateway
            .execute_provider_transport(discovery(&url, "proxy-target.test", target.address.port()))
            .await
            .expect("through the proxy");
        assert_eq!(response.body, b"local proxy fixture");
        // The proxy gets the name, so its own rules choose the way out.
        assert_eq!(
            proxy.destinations.lock().expect("destinations").as_slice(),
            &[("proxy-target.test".into(), target.address.port(), true)]
        );
        assert_eq!(target.requests.lock().expect("requests").len(), 1);
    }
}

#[tokio::test]
async fn a_model_request_goes_direct_when_its_proxy_refuses_before_sending() {
    let target = target(true, vec![Reply::ok()]).await;
    let proxy = proxy(ToolNetworkProxyProtocolV1::Http, Behavior::Refuse, &[target.address]).await;
    let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])])
        .with_model_proxy(Arc::new(Named(Some(proxy.settings.clone()))));
    let url = target.url("https", "proxy-target.test");
    let response = gateway
        .execute_provider_transport(discovery(&url, "proxy-target.test", target.address.port()))
        .await
        .expect("direct after the proxy refused");
    assert_eq!(response.body, b"local proxy fixture");
    assert_eq!(proxy.handshakes.lock().expect("handshakes").len(), 1, "the proxy was asked first");
    assert!(proxy.destinations.lock().expect("destinations").is_empty());
    assert_eq!(target.requests.lock().expect("requests").len(), 1);
}

#[tokio::test]
async fn a_model_request_to_a_local_endpoint_never_uses_the_proxy() {
    let target = target(true, vec![Reply::ok()]).await;
    let proxy = proxy(ToolNetworkProxyProtocolV1::Http, Behavior::Relay, &[target.address]).await;
    let gateway = gateway(&[("proxy-target.test", &["127.0.0.1"])])
        .with_model_proxy(Arc::new(Named(Some(proxy.settings.clone()))));
    let url = target.url("https", "proxy-target.test");
    let response = gateway
        .execute_provider_transport(discovery(&url, "proxy-target.test", target.address.port()))
        .await
        .expect("a local endpoint, direct");
    assert_eq!(response.body, b"local proxy fixture");
    assert!(proxy.handshakes.lock().expect("handshakes").is_empty());
    assert_eq!(target.requests.lock().expect("requests").len(), 1);
}
