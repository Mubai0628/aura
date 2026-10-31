use crate::model_channel::authorize_request;
use aura_context_domain::*;
use aura_context_runtime::*;
use aura_domain_common::*;
use aura_network_control::{
    CoreHttpClientGateway, InMemoryNetworkGrantStore, NetworkControlOptions,
};
use aura_security_contract::*;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Sources {
    session: SessionId,
    profile: ModelProfileId,
    readable: bool,
}
impl ModelEgressAuthorizationFacts for Sources {
    fn session_model_channel_is_authorized(
        &self,
        session: SessionId,
        profile: ModelProfileId,
    ) -> bool {
        session == self.session && profile == self.profile
    }
    async fn persisted_event(
        &self,
        _: EventId,
    ) -> Result<Option<PersistedContextEventAuthorizationFact>, ModelEgressAuthorizationError> {
        Ok(None)
    }
    fn source_grant_binding_is_valid(
        &self,
        _: SessionId,
        _: &ContextSourceRef,
        _: ModelEgressSourceKind,
        _: NetworkDataClass,
        _: &SourceAuthorizationBinding,
    ) -> Result<bool, ModelEgressAuthorizationError> {
        Ok(self.readable)
    }
}

#[tokio::test]
async fn configured_loopback_provider_uses_real_gateway_without_tls_or_network_override() {
    let fixture = aura_contract_schema::canonical_fixture().expect("fixture");
    let mut snapshot = fixture.models.execution_snapshot.expect("snapshot");
    let mut endpoint =
        fixture.models.registry.expect("registry").endpoints[&snapshot.endpoint_id].clone();
    let mut identity = aura_contract_schema::v2_kernel_fixture()
        .expect("identity")
        .dispatch
        .operation
        .content
        .identity;
    identity.actor = InvocationActor::ParentAgent;
    identity.agent_run_id = Some(AgentRunId::new());
    identity.turn_task_id = Some(TurnTaskId::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("fixture peer");
    endpoint.base_url = format!("http://{}/v1", listener.local_addr().expect("address"));
    endpoint.auth_scheme = aura_model_domain::EndpointAuthScheme::None;
    snapshot.auth_scheme = endpoint.auth_scheme;
    snapshot.endpoint_config_revision = endpoint.revision.value();
    snapshot.base_url = endpoint.base_url.clone();
    snapshot.resolved_request_url =
        aura_model_config::resolve_api_url(&endpoint.base_url, endpoint.api_format)
            .expect("url")
            .to_string();
    snapshot.endpoint_fingerprint =
        aura_model_config::endpoint_fingerprint(&endpoint).expect("fingerprint");
    snapshot.credential_id = None;
    snapshot.credential_revision = None;
    snapshot.credential_binding_fingerprint = None;
    let text = "synthetic user file: -----BEGIN PRIVATE KEY----- example only";
    let manifest = ModelEgressManifestV1::new(
        snapshot.context_snapshot_id,
        vec![ModelEgressItem {
            ordinal: 1,
            source_ref: ContextSourceRef::Event { event_id: EventId::new() },
            source_kind: ModelEgressSourceKind::ExternalFileText,
            data_class: NetworkDataClass::UserFileContent,
            content_hash: ContentHash::sha256(text.as_bytes()),
            byte_length: text.len() as u64,
            authorization_basis: ModelEgressAuthorizationBasis::SourceGrants {
                bindings: vec![SourceAuthorizationBinding {
                    authorization: SourceAuthorizationRef::ExternalGrant {
                        external_grant_id: ExternalGrantId::new(),
                    },
                    authorization_revision: Revision::INITIAL,
                    binding_fingerprint: ContentHash::sha256(b"authorized read"),
                }],
            },
        }],
    )
    .expect("read binding, no separate egress approval");
    let mut evidence = fixture.models.provider_request_evidence.expect("request evidence");
    evidence.operation_context = aura_model_domain::TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    let body = serde_json::to_vec(&serde_json::json!({"input":text})).expect("body");
    evidence.body_sha256 = ContentHash::sha256(&body);
    evidence.body_byte_length = body.len() as u64;
    let plan = aura_model_transport::ProviderRequestPlan {
        method: "POST".into(),
        request_url: snapshot.resolved_request_url.clone(),
        headers: vec![("content-type".into(), "application/json".into())],
        body,
        safe_optional_parameters: Default::default(),
        request_evidence: evidence,
    };
    let mut facts = Sources {
        session: identity.session_id.expect("session"),
        profile: snapshot.model_profile_id,
        readable: true,
    };
    let request = authorize_request(
        &identity,
        &endpoint,
        &snapshot,
        &manifest,
        &facts,
        &plan,
        TransportAttemptId::new(),
        NetworkRequestId::new(),
        UtcTimestamp::now(),
    )
    .await
    .expect("configured authority");
    facts.readable = false;
    assert!(
        authorize_request(
            &identity,
            &endpoint,
            &snapshot,
            &manifest,
            &facts,
            &plan,
            TransportAttemptId::new(),
            NetworkRequestId::new(),
            UtcTimestamp::now()
        )
        .await
        .is_err(),
        "revoked read denied"
    );
    facts.readable = true;
    let mut changed = endpoint.clone();
    changed.revision = Revision::new(endpoint.revision.value() + 1);
    assert!(
        authorize_request(
            &identity,
            &changed,
            &snapshot,
            &manifest,
            &facts,
            &plan,
            TransportAttemptId::new(),
            NetworkRequestId::new(),
            UtcTimestamp::now()
        )
        .await
        .is_err(),
        "stale endpoint denied"
    );
    let expected = plan.body;
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("connection");
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let count = socket.read(&mut chunk).await.expect("request");
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(header_end) = bytes.windows(4).position(|v| v == b"\r\n\r\n")
                && bytes.len() >= header_end + 4 + expected.len()
            {
                assert_eq!(&bytes[header_end + 4..], &expected);
                break;
            }
            assert!(bytes.len() < 16_384);
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").await.expect("response");
    });
    let gateway = CoreHttpClientGateway::new(
        Arc::new(InMemoryNetworkGrantStore::default()),
        None,
        NetworkControlOptions::default(),
    );
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        gateway.execute_provider_transport(request),
    )
    .await
    .expect("bounded request")
    .expect("normal production gateway");
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"{\"ok\":true}");
    peer.await.expect("peer verified actual transmitted bytes");
}
