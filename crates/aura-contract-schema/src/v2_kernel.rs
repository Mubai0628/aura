use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{
    ArtifactId, CredentialId, CredentialRevision, Revision, RuntimeInstanceId, SecurityOperationId,
    UtcTimestamp,
};
use aura_execution_contract::*;
use aura_security_contract::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct V2KernelFixture {
    pub dispatch: DispatchRequest,
    pub authority: AuthorizationDecision,
    pub cancellation: CancellationState,
    pub result: ExecutionResult,
    pub grant: AuthorizationGrant,
}

pub fn v2_kernel_fixture() -> anyhow::Result<V2KernelFixture> {
    let old = crate::canonical_fixture()?;
    let identity = InvocationIdentity {
        principal_id: "pri_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?,
        actor: InvocationActor::User,
        workspace_id: Some(old.ids.workspace_id),
        session_id: Some(old.ids.session_id),
        turn_task_id: None,
        agent_run_id: None,
        parent_agent_run_id: None,
        correlation_id: old.ids.correlation_id,
        permission_snapshot_id: None,
    };
    let operation_id: SecurityOperationId = "sop_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?;
    let operation = SecuredOperation::new(
        operation_id,
        OperationContent {
            compatibility: Default::default(),
            network: OperationNetwork::None,
            identity,
            source: SecuritySourceRef::UserInterface,
            source_revision_fingerprint: None,
            capabilities: vec![ResolvedCapability::WorkspaceFileRead],
            resources: vec![ResourceDescriptor::WorkspaceRoot {
                workspace_id: old.ids.workspace_id,
            }],
            risks: vec![],
            credentials: vec![CredentialUseRef {
                credential_id: "crd_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse::<CredentialId>()?,
                credential_revision: CredentialRevision::INITIAL,
                authority: SecuritySourceRef::SystemComponent {
                    component_id: "fixture-authority".into(),
                },
                use_fingerprint: CanonicalSha256HashV1::sha256(b"synthetic-use-no-secret"),
            }],
            purpose: Some("Verify V2 contract bindings".into()),
            action_kind: "fixture.read".into(),
            action_revision: Revision::INITIAL,
            action: serde_json::json!({"path":"fixture.txt"}),
        },
    )?;
    let backend = BackendDescriptor {
        known_limitations: Default::default(),
        compatibility: Default::default(),
        id: BackendId("fixture.broker".into()),
        revision: Revision::INITIAL,
        runtime_instance_id: "run_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse::<RuntimeInstanceId>()?,
        class: BackendClass::CoreBroker,
        capabilities: [ResolvedCapability::WorkspaceFileRead].into(),
        guarantees: [EnforcementGuarantee::AuthorizedWritesOnly].into(),
        readiness: BackendReadiness::Ready,
        protocol_fingerprint: CanonicalSha256HashV1::sha256(b"fixture-protocol"),
    };
    let now: UtcTimestamp = "2026-09-07T00:00:00Z".parse()?;
    let expires: UtcTimestamp = "2026-09-07T01:00:00Z".parse()?;
    let scope = AuthorizationScope {
        operation_fingerprint: operation.fingerprint.clone(),
        profile: SecurityProfile::Balanced,
        backend_requirement: BackendRequirement {
            allowed_classes: [BackendClass::CoreBroker, BackendClass::RestrictedUserOffline].into(),
            guarantees: [EnforcementGuarantee::AuthorizedWritesOnly].into(),
        },
        lifetime: AuthorizationLifetime::Once { operation_id },
        consent: None,
        network_consent: None,
        capability_limit: None,
        memory_consent: None,
        mcp_consent: None,
        chrome_consent: None,
        chrome_site_consent: None,
        file_consent: None,
    };
    let grant = AuthorizationGrant {
        grant_id: "pgr_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?,
        revision: Revision::INITIAL,
        scope,
        issued_at: now,
        expires_at: Some(expires),
        revoked_at: None,
        consumed_at: None,
    };
    let authority =
        AuthorizationDecision::from_grant(&grant, now, expires).expect("live fixture grant");
    let plan = ExecutionPlan::new(PreparedExecution {
        operation_fingerprint: operation.fingerprint.clone(),
        backend,
        preparation: serde_json::json!({"resolved_file_identity":"synthetic-file-1"}),
    })?;
    let attempt = AttemptRef { operation_id, ordinal: std::num::NonZeroU32::MIN };
    let cancellation = CancellationState { revision: Revision::INITIAL, requested: false };
    let binding = DispatchBinding {
        attempt: attempt.clone(),
        operation_fingerprint: operation.fingerprint.clone(),
        authorization_fingerprint: authority.fingerprint()?,
        plan_fingerprint: plan.fingerprint.clone(),
        cancellation_revision: cancellation.revision,
    };
    let result = ExecutionResult {
        outcome: ExecutionOutcome::Succeeded,
        side_effect: SideEffectStateV1::Unknown,
        receipt: ExecutionReceipt {
            attempt,
            evidence_ref: "art_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse::<ArtifactId>()?,
            postcondition_fingerprint: None,
            rollback: RollbackEvidence::Unavailable,
        },
        termination: TerminationEvidence::Unknown,
        completed_at: now,
        warnings: vec!["tool.side_effect.unverified".into()],
    };
    Ok(V2KernelFixture {
        dispatch: DispatchRequest { operation, plan, binding },
        authority,
        cancellation,
        result,
        grant,
    })
}
