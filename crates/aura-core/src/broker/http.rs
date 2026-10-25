use crate::{JsonRpcRequest, RuntimeServices, RuntimeState};
use aura_domain_common::{NetworkRequestId, UtcTimestamp};
use aura_execution_contract::{
    BrokerEvidence, OperationNetwork, SecuredOperation, SideEffectStateV1,
};
use aura_permission_domain::*;
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::{
    NetworkCallerKind, NetworkDataClass, PermissionGrantLifetime, ResolvedCapability,
    ResourceDescriptor,
};
use base64::Engine as _;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) async fn dispatch_http(
    state: &RuntimeState,
    services: &RuntimeServices,
    request: &JsonRpcRequest,
) -> Result<Value, IpcErrorEnvelope> {
    let input: BrokerHttpReadRequest = serde_json::from_value(request.params.clone())
        .map_err(|e| super::error_envelope(request.id, e))?;
    let url = url::Url::parse(&input.url).map_err(|e| super::error_envelope(request.id, e))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !(1..=1024 * 1024).contains(&input.max_bytes)
    {
        return Err(super::error_envelope(request.id, "invalid credential-free HTTP read"));
    }
    let origin = NormalizedOrigin {
        scheme: url.scheme().into(),
        host: match url
            .host()
            .ok_or_else(|| super::error_envelope(request.id, "HTTP host missing"))?
        {
            url::Host::Domain(host) => host.trim_end_matches('.').to_ascii_lowercase(),
            url::Host::Ipv4(address) => address.to_string(),
            url::Host::Ipv6(address) => address.to_string(),
        },
        port: url
            .port_or_known_default()
            .ok_or_else(|| super::error_envelope(request.id, "HTTP port missing"))?,
    };
    let mut operation = super::operation(
        &request.method,
        &request.params,
        request.id,
        vec![ResolvedCapability::PublicNetworkRead],
        vec![ResourceDescriptor::NetworkOrigin {
            scheme: origin.scheme.clone(),
            host: origin.host.clone(),
            port: origin.port,
        }],
    )
    .map_err(|e| super::error_envelope(request.id, e))?;
    operation.content.network = OperationNetwork::ExactOrigins;
    operation = SecuredOperation::new(operation.operation_id, operation.content)
        .map_err(|e| super::error_envelope(request.id, e))?;
    let identity = operation.content.identity.clone();
    let http_persistence = services.persistence.clone();
    let http_session = input.session_id;
    let http_workspace = input.workspace_id;
    let http_grant = input.network_grant_id;
    let http_max_bytes = input.max_bytes;
    let inspect_origin = origin.clone();
    let inspect: super::OwnedInspect = Arc::new(move || {
        anyhow::ensure!(
            http_persistence
                .workspace_session_projection()?
                .sessions
                .iter()
                .any(|s| s.session_id == http_session && s.workspace_id == http_workspace),
            "HTTP session scope mismatch"
        );
        let grant = http_persistence
            .network_grant(http_grant)?
            .ok_or_else(|| anyhow::anyhow!("HTTP approval required"))?;
        anyhow::ensure!(
            grant.revoked_at.is_none()
                && grant.expires_at.is_none_or(|t| UtcTimestamp::now() < t)
                && grant.identity_scope == NetworkGrantIdentityScope::Session
                && grant.identity_scope_id == http_session.to_string()
                && grant.lifetime == PermissionGrantLifetime::Session
                && grant.origins.contains(&inspect_origin)
                && grant.methods.iter().any(|m| m == "GET")
                && grant.allowed_data_classes.contains(&NetworkDataClass::PublicQuery)
                && grant.max_download_bytes.is_none_or(|m| u64::from(http_max_bytes) <= m),
            "HTTP approval is missing, expired or does not cover the exact read"
        );
        Ok(json!({"grant":grant,"origin":inspect_origin}))
    });
    let event_method = request.method.clone();
    let evidence: super::OwnedEvidence<Value> = Arc::new(
        move |dispatch: &aura_execution_contract::DispatchRequest,
              output: &Result<Value, IpcErrorEnvelope>| {
            Ok(BrokerEvidence {
        action_kind:event_method.clone(),request_fingerprint:dispatch.operation.fingerprint.0.clone(),before:dispatch.plan.prepared.preparation.clone(),
        after:output.as_ref().ok().map_or(Value::Null,|v|json!({"content_hash":v["content_hash"],"origin":v["final_origin"],"status":v["status"]})),
        domain_journal:json!({"method":"GET","redirects":"deny","credentials":false}),side_effect:SideEffectStateV1::NoEffect,
    })
        },
    );
    let call_inspect = Arc::clone(&inspect);
    let handler_request = request.clone();
    let handler_http = services.broker_http.clone();
    super::run(
        state,
        &services.broker,
        operation,
        request.id.to_string(),
        request.command_invocation_id,
        inspect,
        evidence,
        move || async move {
            let current =
                call_inspect().map_err(|e| super::error_envelope(handler_request.id, e))?;
            let grant: NetworkGrant = serde_json::from_value(current["grant"].clone())
                .map_err(|e| super::error_envelope(handler_request.id, e))?;
            let request_id = NetworkRequestId::new();
            let limits = NetworkByteLimits {
                max_upload_bytes: 0,
                max_download_bytes: u64::from(input.max_bytes),
                max_decompressed_bytes: u64::from(input.max_bytes),
            };
            let plan = NetworkRequestPlan {
                request_id,
                identity,
                caller: NetworkCallerKind::AuraTool,
                method: "GET".into(),
                requested_url: input.url.clone(),
                destination: None,
                request_data: vec![],
                data_classes: vec![NetworkDataClass::PublicQuery],
                redirect_policy: RedirectPolicy::Deny,
                byte_limits: limits.clone(),
                timeout_ms: 10_000,
                credential_refs: vec![],
                network_grant_ids: vec![input.network_grant_id],
            };
            let authorization = CoreRequestGrant {
                logical_request_id: request_id,
                purpose: aura_network_control::StandardNetworkRequestGrant,
                caller: plan.caller,
                destination_policy: NetworkDestinationPolicy {
                    protocol: NetworkProtocolClass::Http,
                    host: origin.host.clone(),
                    port: origin.port,
                    allowed_destination_classes: grant.destination_classes,
                },
                methods: vec!["GET".into()],
                redirect_policy: RedirectPolicy::Deny,
                data_classes: plan.data_classes.clone(),
                byte_limits: limits,
                expires_at: UtcTimestamp::from_value(
                    UtcTimestamp::now().value() + time::Duration::seconds(15),
                ),
            };
            let response = handler_http
                .execute(
                    authorization,
                    aura_network_control::GatewayRequest { plan, headers: vec![], body: vec![] },
                )
                .await
                .map_err(|e| super::error_envelope(handler_request.id, e))?;
            let result = BrokerHttpReadResult {
                status: response.status,
                body_base64: base64::engine::general_purpose::STANDARD.encode(&response.body),
                content_hash: aura_domain_common::ContentHash::blake3(&response.body),
                final_origin: response.final_origin,
            };
            serde_json::to_value(result).map_err(|e| super::error_envelope(handler_request.id, e))
        },
    )
    .await
}
