use aura_domain_common::{
    AgentRunId, CommandInvocationId, CorrelationId, ModelStepId, NetworkRequestId,
    PermissionSnapshotId, PrincipalId, Revision, SessionId, TurnTaskId, WorkspaceId,
};
use aura_domain_common::{ToolId, ToolInstanceId, ToolRegistrySnapshotId};
use aura_permission_domain::{
    NetworkAccessRequirement, NetworkByteLimits, NetworkDataReference, ResolvedScope,
    ToolSecuritySourceRefV1,
};
use aura_security_contract::{
    InvocationActor, InvocationIdentity, NetworkDataClass, NetworkDestinationClass,
    ResolvedCapability, ResolvedNetworkDestination,
};
use aura_tool_contract::{
    CoreInvocationIdentityV1, SignedBuiltinToolContractFingerprintV1, ToolDefinitionSnapshotRef,
    ToolPurposeTextV1, ToolRegistrySnapshotRef, ToolSourceContractFingerprintV1, ToolSourceKindV1,
    ToolVersion,
};
use serde_json::json;

use super::*;

fn fixture() -> (ToolHttpRequestAuthorityV1, GatewayRequest) {
    let workspace_id = WorkspaceId::from_bytes([1; 16]);
    let session_id = SessionId::from_bytes([2; 16]);
    let turn_task_id = TurnTaskId::from_bytes([3; 16]);
    let agent_run_id = AgentRunId::from_bytes([4; 16]);
    let permission_snapshot = PermissionSnapshot {
        id: PermissionSnapshotId::from_bytes([5; 16]),
        session_id,
        workspace_id,
        mode: aura_security_contract::PermissionMode::FullAccess,
        session_permission_revision: 1,
        active_grant_ids: Vec::new(),
        created_at: "2026-09-05T00:00:00Z".parse().expect("timestamp"),
    };
    let invocation = CoreInvocationIdentityV1 {
        command_invocation_id: CommandInvocationId::from_bytes([6; 16]),
        session_id,
        turn_task_id,
        agent_run_id,
        model_step_id: ModelStepId::from_bytes([7; 16]),
    };
    let registry = ToolRegistrySnapshotRef {
        tool_registry_snapshot_id: ToolRegistrySnapshotId::from_bytes([8; 16]),
        registry_revision: Revision::INITIAL,
        content_hash: CanonicalHashV1::sha256(b"registry"),
    };
    let source = ToolSecuritySourceRefV1::new(
        ToolDefinitionSnapshotRef {
            tool_id: ToolId::from_bytes([9; 16]),
            tool_version: ToolVersion::new(None, Revision::INITIAL).expect("version"),
            registry_snapshot_ref: registry,
        },
        ToolInstanceId::from_bytes([10; 16]),
        ToolSourceKindV1::BuiltIn,
        CanonicalHashV1::sha256(b"definition"),
        ToolSourceContractFingerprintV1::BuiltIn {
            signed_builtin_tool_contract_fingerprint: SignedBuiltinToolContractFingerprintV1::new(
                CanonicalHashV1::sha256(b"builtin"),
            ),
        },
    )
    .expect("source");
    let body = br#"{"value":"fixture"}"#.to_vec();
    let headers = vec![("content-type".to_owned(), "application/json".to_owned())];
    let exact_url = "https://example.com/v1/items".to_owned();
    let runtime = aura_permission_domain::NonSecretHttpRuntimeFactsV1 {
        runtime_kind: aura_permission_domain::ToolPreparedRuntimeKindV1::DeclarativeHttp,
        method: "POST".to_owned(),
        exact_url: exact_url.clone(),
        canonical_path: "/v1/items".to_owned(),
        headers_hash: canonical_sha256(&headers).expect("headers hash"),
        body_hash: CanonicalHashV1::sha256(&body),
        body_byte_length: body.len() as u64,
        credential: None,
        success_status_ranges: vec![(200, 299)],
        prepared_request_hash: CanonicalHashV1::sha256(b"prepared"),
        grant_manifest_hash: CanonicalHashV1::sha256(b"manifest"),
        timeout_ms: 120_000,
        response_wire_limit: 16 * 1024 * 1024,
        response_decompressed_limit: 32 * 1024 * 1024,
    };
    let operation = W7ToolOperationFingerprintPreimageV1::new(
        invocation.clone(),
        ToolBatchId::from_bytes([11; 16]),
        ToolCallId::from_bytes([12; 16]),
        source,
        ContentHash::sha256(b"source"),
        CanonicalHashV1::sha256(b"execution plan"),
        vec![ResolvedCapability::PublicNetworkRead],
        vec![ResolvedScope::NetworkOrigin {
            scheme: "https".to_owned(),
            host: "example.com".to_owned(),
            port: 443,
        }],
        Vec::new(),
        ToolPurposeTextV1::new("Fetch fixture data").expect("purpose"),
        json!({"value": "fixture"}),
        canonical_sha256(&json!({"value": "fixture"})).expect("arguments hash"),
        NonSecretToolRuntimeFactsV1::NetworkGateway { facts: Box::new(runtime) },
        None,
        NetworkAccessRequirement::Required,
    )
    .expect("operation");
    let identity = InvocationIdentity {
        principal_id: PrincipalId::from_bytes([13; 16]),
        actor: InvocationActor::ParentAgent,
        workspace_id: Some(workspace_id),
        session_id: Some(session_id),
        turn_task_id: Some(turn_task_id),
        agent_run_id: Some(agent_run_id),
        parent_agent_run_id: None,
        permission_snapshot_id: Some(permission_snapshot.id),
        correlation_id: CorrelationId::from_bytes([14; 16]),
    };
    let request = GatewayRequest {
        plan: aura_permission_domain::NetworkRequestPlan {
            request_id: NetworkRequestId::from_bytes([15; 16]),
            identity: identity.clone(),
            caller: NetworkCallerKind::AuraTool,
            method: "POST".to_owned(),
            requested_url: exact_url,
            destination: Some(ResolvedNetworkDestination {
                scheme: "https".to_owned(),
                host: "example.com".to_owned(),
                port: 443,
                addresses: vec!["93.184.216.34".to_owned()],
                classes: vec![NetworkDestinationClass::Public],
            }),
            request_data: vec![NetworkDataReference::JsonBody {
                value: json!({"value": "fixture"}),
            }],
            data_classes: vec![NetworkDataClass::PublicQuery],
            redirect_policy: RedirectPolicy::SameOrigin { max_hops: 3 },
            byte_limits: NetworkByteLimits {
                max_upload_bytes: 8 * 1024 * 1024,
                max_download_bytes: 16 * 1024 * 1024,
                max_decompressed_bytes: 32 * 1024 * 1024,
            },
            timeout_ms: 120_000,
            credential_refs: Vec::new(),
            network_grant_ids: Vec::new(),
        },
        headers,
        body,
    };
    let authority = ToolHttpRequestAuthorityV1::new(
        SecurityOperationId::from_bytes([16; 16]),
        operation,
        identity,
        permission_snapshot,
        PermissionAllowAuthorizationBasisV1::PermissionMode {
            mode: aura_security_contract::PermissionMode::FullAccess,
        },
        ToolExecutionAttemptId::from_bytes([17; 16]),
        "2026-09-05T00:00:01Z".parse().expect("timestamp"),
    )
    .expect("authority");
    (authority, request)
}

#[test]
fn exact_tool_http_grant_binds_request_and_attempt() {
    let (authority, request) = fixture();
    let grant = authority.issue(&request).expect("grant");
    assert_eq!(grant.purpose.tool_execution_attempt_id, authority.tool_execution_attempt_id());
    assert_eq!(grant.logical_request_id, request.plan.request_id);
    assert_eq!(
        grant.expires_at.value(),
        "2026-09-05T00:02:01Z".parse::<UtcTimestamp>().expect("expiry").value()
    );
    grant.purpose.validate_request(&request).expect("request matches");
}

#[test]
fn exact_tool_http_grant_rejects_request_mutation() {
    let (authority, request) = fixture();
    let grant = authority.issue(&request).expect("grant");
    for changed in [
        {
            let mut value = request.clone();
            value.plan.method = "PUT".to_owned();
            value
        },
        {
            let mut value = request.clone();
            value.headers[0].1 = "text/plain".to_owned();
            value
        },
        {
            let mut value = request.clone();
            value.body.push(b' ');
            value
        },
    ] {
        assert!(matches!(
            grant.purpose.validate_request(&changed),
            Err(NetworkControlError::CoreGrantBindingMismatch)
        ));
    }
}

#[test]
fn authority_requires_exact_permission_and_network_runtime() {
    let (authority, _) = fixture();
    assert_ne!(authority.authorization_binding_hash().hex(), CanonicalHashV1::sha256(&[]).hex());
}

#[test]
fn authority_rejects_a_grant_issued_for_another_attempt() {
    let (authority, request) = fixture();
    let grant = authority.issue(&request).expect("grant");
    let other = ToolHttpRequestAuthorityV1::new(
        SecurityOperationId::from_bytes([19; 16]),
        authority.operation.clone(),
        authority.operation_identity.clone(),
        authority.permission_snapshot.clone(),
        authority.authorization_basis.clone(),
        ToolExecutionAttemptId::from_bytes([20; 16]),
        "2026-09-05T00:00:01Z".parse().expect("timestamp"),
    )
    .expect("other authority");

    assert!(matches!(
        other.validate_grant(&grant),
        Err(NetworkControlError::CoreGrantBindingMismatch)
    ));
}

mod full_access;
