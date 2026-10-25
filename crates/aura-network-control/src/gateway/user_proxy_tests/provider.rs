use super::proxy::{Behavior, proxy};
use super::*;
use crate::{
    AuthenticatedConfigurationChannelIdentity, ConfigurationOperationKind,
    ConfigurationOperationRequestManifestV1, ConfigurationRequestDataClass,
    ConfigurationRequestPurpose, ConfigurationRequestTargetV1, ConfigurationResponseMediaPolicyV1,
    UserConfigurationOperationIdentity,
};
use aura_domain_common::{
    CommandInvocationId, CorrelationId, EndpointId, IpcChannelId, ModelDiscoveryRunId,
    NetworkRequestId, Revision, RuntimeInstanceId, TransportAttemptId, UtcTimestamp,
};

#[tokio::test]
async fn real_provider_gateway_keeps_direct_transport_after_a_proxied_tool_on_the_same_gateway() {
    let target = target(true, vec![Reply::ok()]).await;
    let proxy = proxy(ToolNetworkProxyProtocolV1::Http, Behavior::Relay, &[target.address]).await;
    let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
    let url = target.url("https", "proxy-target.test");
    run(&gateway, request(url.clone()), Some(proxy.settings.clone()), true)
        .await
        .expect("proxied tool");
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
            origin: NormalizedOrigin {
                scheme: "https".into(),
                host: "proxy-target.test".into(),
                port: target.address.port(),
            },
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
    let response = gateway
        .execute_provider_transport(ProviderTransportGatewayRequest {
            authority: ProviderTransportRequestAuthority::UserConfiguration(identity),
            grant,
            immutable_request: ProviderTransportImmutableRequest {
                request_id,
                method: "GET".into(),
                requested_url: url,
                headers: vec![],
                body: vec![],
                redirect_policy: manifest.redirect_policy,
                byte_limits,
                connect_timeout_ms: 1000,
                timeout_ms: Some(2000),
            },
        })
        .await
        .expect("direct provider fixture");
    assert_eq!(response.body, b"local proxy fixture");
    assert_eq!(proxy.destinations.lock().expect("tool only").len(), 1);
    assert_eq!(target.requests.lock().expect("tool and provider").len(), 2);
}
