use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    AgentRunId, ContentHash, CorrelationId, PermissionSnapshotId, PrincipalId, SecurityOperationId,
    SessionId, ToolId, ToolInstanceId, ToolRegistrySnapshotId, TurnTaskId, UtcTimestamp,
    WorkspaceId,
};
use aura_permission_domain::{
    NetworkAccessRequirement, OperationSecurityPlan, ResolvedScope, SecuritySource,
};
use aura_security_contract::{
    InvocationActor, InvocationIdentity, PermissionMode, PermissionSnapshot, ResolvedCapability,
};
use aura_tool_contract::{
    ToolDefinitionSnapshotRef, ToolRegistrySnapshotRef, ToolSourceContractFingerprintV1,
    ToolSourceKindV1, ToolVersion,
};

pub fn built_in_tool_source() -> SecuritySource {
    SecuritySource::Tool {
        tool: Box::new(
            aura_permission_domain::ToolSecuritySourceRefV1::new(
                ToolDefinitionSnapshotRef {
                    tool_id: ToolId::new(),
                    tool_version: ToolVersion::new(None, aura_domain_common::Revision::INITIAL)
                        .expect("version"),
                    registry_snapshot_ref: ToolRegistrySnapshotRef {
                        tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                        registry_revision: aura_domain_common::Revision::INITIAL,
                        content_hash: CanonicalHashV1::sha256(b"permission-matrix-registry"),
                    },
                },
                ToolInstanceId::new(),
                ToolSourceKindV1::BuiltIn,
                CanonicalHashV1::sha256(b"permission-matrix-definition"),
                ToolSourceContractFingerprintV1::BuiltIn {
                    signed_builtin_tool_contract_fingerprint:
                        aura_tool_contract::SignedBuiltinToolContractFingerprintV1::new(
                            CanonicalHashV1::sha256(b"permission-matrix-source"),
                        ),
                },
            )
            .expect("built-in source"),
        ),
    }
}

pub fn fixture(mode: PermissionMode) -> (OperationSecurityPlan, PermissionSnapshot) {
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    let snapshot = PermissionSnapshot {
        id: PermissionSnapshotId::new(),
        session_id,
        workspace_id,
        mode,
        session_permission_revision: 1,
        active_grant_ids: Vec::new(),
        created_at: UtcTimestamp::now(),
    };
    let run = AgentRunId::new();
    let plan = OperationSecurityPlan {
        operation_id: SecurityOperationId::new(),
        identity: InvocationIdentity {
            principal_id: PrincipalId::new(),
            actor: InvocationActor::ParentAgent,
            workspace_id: Some(workspace_id),
            session_id: Some(session_id),
            turn_task_id: Some(TurnTaskId::new()),
            agent_run_id: Some(run),
            parent_agent_run_id: None,
            correlation_id: CorrelationId::new(),
            permission_snapshot_id: Some(snapshot.id),
        },
        source: built_in_tool_source(),
        capabilities: vec![ResolvedCapability::WorkspaceFileWrite],
        scopes: vec![ResolvedScope::WorkspaceRoot { workspace_id }],
        risk_flags: Vec::new(),
        purpose: Some("Update the requested source file".to_owned()),
        source_fingerprint: None,
        requested_sandbox: None,
        network_access: NetworkAccessRequirement::NotRequired,
        network_plan: None,
        operation_fingerprint: ContentHash::blake3(b"operation"),
    };
    (plan, snapshot)
}
