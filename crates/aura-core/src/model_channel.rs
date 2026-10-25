//! Model channel authority is derived from the user's configured Endpoint and
//! the prepared context. Agent tool modes are deliberately not an input.
use anyhow::{Result, ensure};
use aura_context_domain::ModelEgressManifestV1;
use aura_context_runtime::{ModelEgressAuthorizationFacts, ModelEgressAuthorizer};
use aura_domain_common::{
    ContentHash, NetworkRequestId, TransportAttemptId, UtcTimestamp, canonical_json_v1_sha256,
};
use aura_model_domain::{ModelEndpointConfig, ModelExecutionSnapshot, TransportOperationContext};
use aura_model_transport::ProviderRequestPlan;
use aura_network_control::{
    ConfigurationRequestCredentialBindingV1, ConfigurationRequestTargetV1,
    ConfigurationResponseMediaPolicyV1, ConfigurationTransportPolicyV1,
    ProviderTransportAttemptGrant, ProviderTransportAuthorizationBinding,
    ProviderTransportGatewayRequest, ProviderTransportImmutableRequest,
    ProviderTransportRequestAuthority, canonical_non_secret_headers_hash,
    invocation_identity_binding_hash,
};
use aura_permission_domain::{
    CoreRequestGrant, NetworkByteLimits, NetworkDestinationPolicy, NetworkProtocolClass,
    NormalizedOrigin, RedirectPolicy,
};
use aura_security_contract::{
    InvocationActor, InvocationIdentity, NetworkCallerKind, NetworkDestinationClass,
};

/// Core-only inputs: reload `endpoint` from Config and credential revisions from
/// Secret Store before this call. `facts` is the context source-access authority.
/// No request/DTO may assert that it belongs to the Model channel.
///
/// This is the egress verification entry of the Agent path, and the facts it reads are
/// persisted rows. It is awaitable so the caller (the transport's `authorize_model_request`) can
/// await the store fact on its own bounded owner instead of the reactor holding the journal. A
/// fact that could not be read inside that budget is reported as `model_egress.
/// authorization_fact_store_failed`, never as an authorization success.
#[allow(clippy::too_many_arguments)]
pub async fn authorize_request<F: ModelEgressAuthorizationFacts>(
    identity: &InvocationIdentity,
    endpoint: &ModelEndpointConfig,
    snapshot: &ModelExecutionSnapshot,
    manifest: &ModelEgressManifestV1,
    facts: &F,
    plan: &ProviderRequestPlan,
    attempt: TransportAttemptId,
    request_id: NetworkRequestId,
    now: UtcTimestamp,
) -> Result<ProviderTransportGatewayRequest> {
    ensure!(
        matches!(identity.actor, InvocationActor::ParentAgent | InvocationActor::ChildAgent)
            && identity.agent_run_id.is_some()
            && identity.turn_task_id.is_some()
            && identity.workspace_id.is_some(),
        "model.identity_invalid"
    );
    let session = identity.session_id.ok_or_else(|| anyhow::anyhow!("model.session_missing"))?;
    ensure!(
        endpoint.id == snapshot.endpoint_id
            && endpoint.provider_id == snapshot.provider_id
            && endpoint.auth_scheme == snapshot.auth_scheme
            && endpoint.api_format == snapshot.api_format
            && endpoint.revision.value() == snapshot.endpoint_config_revision
            && aura_model_config::endpoint_fingerprint(endpoint)? == snapshot.endpoint_fingerprint
            && aura_model_config::resolve_api_url(&endpoint.base_url, endpoint.api_format)?
                .as_str()
                == snapshot.resolved_request_url
            && plan.request_url == snapshot.resolved_request_url
            && plan.method == "POST"
            && !plan.body.is_empty()
            && manifest.context_snapshot_id == snapshot.context_snapshot_id,
        "model.endpoint_or_context_changed"
    );
    ModelEgressAuthorizer::new(facts)
        .validate(session, snapshot.model_profile_id, manifest)
        .await?;
    let operation_context = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    ensure!(
        plan.request_evidence.operation_context == operation_context
            && plan.request_evidence.body_sha256 == ContentHash::sha256(&plan.body)
            && plan.request_evidence.body_byte_length == plan.body.len() as u64,
        "model.attempt_context_changed"
    );
    ensure!(
        !plan.headers.iter().any(|(name, _)| matches!(
            name.to_ascii_lowercase().as_str(),
            "cookie" | "set-cookie" | "authorization" | "proxy-authorization" | "x-api-key"
        )),
        "model.credential_header_forbidden"
    );
    let url = url::Url::parse(&plan.request_url)?;
    ensure!(
        url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
        "model.url_invalid"
    );
    let origin = NormalizedOrigin {
        scheme: url.scheme().into(),
        host: url.host_str().ok_or_else(|| anyhow::anyhow!("model.host"))?.into(),
        port: url.port_or_known_default().ok_or_else(|| anyhow::anyhow!("model.port"))?,
    };
    let response_limit = ConfigurationTransportPolicyV1::agent_model_response_limit_bytes(
        snapshot.request_options.effective_max_output_tokens,
    )
    .ok_or_else(|| anyhow::anyhow!("model.response_limit"))?;
    let limits = NetworkByteLimits {
        max_upload_bytes: 32 * 1024 * 1024,
        max_download_bytes: response_limit,
        max_decompressed_bytes: response_limit,
    };
    let credential_binding = match (
        snapshot.credential_id,
        snapshot.credential_revision,
        snapshot.credential_binding_fingerprint.as_ref(),
    ) {
        (Some(credential_id), Some(credential_revision), Some(fingerprint)) => {
            Some(ConfigurationRequestCredentialBindingV1 {
                credential_id,
                credential_revision,
                credential_binding_fingerprint: fingerprint.clone(),
            })
        }
        (None, None, None) => None,
        _ => anyhow::bail!("model.credential_binding_incomplete"),
    };
    ensure!(
        credential_binding.is_some()
            == (endpoint.auth_scheme != aura_model_domain::EndpointAuthScheme::None),
        "model.credential_configuration_mismatch"
    );
    let redirect_policy = RedirectPolicy::SameOrigin { max_hops: 3 };
    let purpose = ProviderTransportAttemptGrant {
        transport_attempt_id: attempt,
        operation_context,
        endpoint_id: endpoint.id,
        endpoint_security_fingerprint: snapshot.endpoint_fingerprint.clone(),
        request_manifest_hash: canonical_json_v1_sha256(&(
            snapshot.id,
            snapshot.endpoint_id,
            snapshot.endpoint_config_revision,
            snapshot.model_config_revision,
            &snapshot.endpoint_fingerprint,
            &manifest.manifest_hash,
            &plan.request_evidence,
        ))?,
        non_secret_headers_hash: canonical_non_secret_headers_hash(&plan.headers)?,
        request_body_hash: ContentHash::sha256(&plan.body),
        request_body_length: plan.body.len() as u64,
        target: ConfigurationRequestTargetV1 {
            method: plan.method.clone(),
            origin: origin.clone(),
            path_and_query: match url.query() {
                Some(q) => format!("{}?{q}", url.path()),
                None => url.path().into(),
            },
        },
        credential_binding,
        byte_limits: limits.clone(),
        redirect_policy,
        response_media_policy: ConfigurationResponseMediaPolicyV1 {
            accepted_media_types: ["text/event-stream".into(), "application/json".into()].into(),
            require_content_type: true,
        },
        authorization: ProviderTransportAuthorizationBinding::AgentModel {
            invocation_identity_binding_hash: invocation_identity_binding_hash(identity)?,
            model_egress_manifest_hash: manifest.manifest_hash.clone(),
        },
    };
    purpose.validate_intrinsic()?;
    Ok(ProviderTransportGatewayRequest {
        authority: ProviderTransportRequestAuthority::AgentModel(identity.clone()),
        grant: CoreRequestGrant {
            logical_request_id: request_id,
            caller: NetworkCallerKind::ModelTransport,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host: origin.host,
                port: origin.port,
                // Same configured target rule as discovery/availability. No test
                // loopback override, wildcard grant or tool permission is used.
                allowed_destination_classes: vec![
                    NetworkDestinationClass::Public,
                    NetworkDestinationClass::Private,
                    NetworkDestinationClass::Loopback,
                ],
            },
            methods: vec![plan.method.clone()],
            redirect_policy,
            data_classes: vec![],
            byte_limits: limits.clone(),
            purpose,
            expires_at: UtcTimestamp::from_value(now.value() + time::Duration::seconds(30)),
        },
        immutable_request: ProviderTransportImmutableRequest {
            request_id,
            method: plan.method.clone(),
            requested_url: plan.request_url.clone(),
            headers: plan.headers.clone(),
            body: plan.body.clone(),
            redirect_policy,
            byte_limits: limits,
            connect_timeout_ms: endpoint.timeouts.connect_ms,
            timeout_ms: endpoint.timeouts.total_ms,
        },
    })
}
