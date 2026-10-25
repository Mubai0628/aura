//! Shared policy fixture; behavior scenarios remain in their owning modules.
use super::*;
pub(super) struct Case {
    pub(super) op: SecuredOperation,
    pub(super) backend: BackendDescriptor,
    pub(super) snapshot: PermissionSnapshot,
    pub(super) trust: TrustState,
    pub(super) grants: Vec<AuthorizationGrant>,
    pub(super) verified_external: Vec<ResolvedCapability>,
    pub(super) profile: SecurityProfile,
    /// The Core runtime instance used by historical Native consent checks.
    pub(super) runtime_instance: aura_domain_common::RuntimeInstanceId,
    /// Whose network the call is, as Core derives it from the tool.
    pub(super) caller: aura_security_contract::NetworkCallerKind,
}

impl Case {
    pub(super) fn new(process: bool) -> Self {
        let f = aura_contract_schema::v2_kernel_fixture().expect("fixture");
        let mut op = f.dispatch.operation;
        op.content.credentials.clear();
        op.content.identity.actor = InvocationActor::ParentAgent;
        op.content.identity.agent_run_id = Some(aura_domain_common::AgentRunId::new());
        op.content.identity.turn_task_id = Some(aura_domain_common::TurnTaskId::new());
        op.content.source =
            SecuritySourceRef::BuiltInTool { identity_hash: op.fingerprint.0.clone() };
        let mut backend = f.dispatch.plan.prepared.backend;
        if process {
            op.content.capabilities = vec![ResolvedCapability::ProcessExecute];
            backend.capabilities = op.content.capabilities.iter().copied().collect();
            backend.class = BackendClass::RestrictedUserOffline;
            backend.guarantees.extend([
                EnforcementGuarantee::IdentityIsolation,
                EnforcementGuarantee::DirectNetworkBlockedV1,
                EnforcementGuarantee::ChildProcessControl,
                EnforcementGuarantee::BoundedOutput,
            ]);
            backend.known_limitations.insert(BackendKnownLimitation::DnscacheServiceMediatedDns);
        }
        let snapshot = PermissionSnapshot {
            id: PermissionSnapshotId::new(),
            session_id: op.content.identity.session_id.expect("session"),
            workspace_id: op.content.identity.workspace_id.expect("workspace"),
            mode: PermissionMode::WorkspaceReadWrite,
            session_permission_revision: 1,
            active_grant_ids: vec![],
            created_at: f.authority.decided_at,
        };
        op.content.identity.permission_snapshot_id = Some(snapshot.id);
        op = SecuredOperation::new(op.operation_id, op.content).expect("operation");
        Self {
            op,
            backend,
            snapshot,
            trust: TrustState::SystemTrusted,
            grants: vec![],
            verified_external: vec![],
            profile: SecurityProfile::Balanced,
            runtime_instance: aura_domain_common::RuntimeInstanceId::new(),
            caller: aura_security_contract::NetworkCallerKind::AuraTool,
        }
    }
    pub(super) fn refresh(&mut self) {
        self.op =
            SecuredOperation::new(self.op.operation_id, self.op.content.clone()).expect("rehash");
    }
    pub(super) fn input(&self) -> PolicyInput<'_> {
        PolicyInput {
            network_caller: self.caller,
            operation: &self.op,
            snapshot: &self.snapshot,
            profile: self.profile,
            source_enabled: true,
            trust: &self.trust,
            source_fingerprint: self.op.content.source_revision_fingerprint.as_ref(),
            hard_boundaries_satisfied: true,
            denied_for_turn: false,
            grants: &self.grants,
            verified_external_grants: &[],
            verified_external_capabilities: &self.verified_external,
            now: self.snapshot.created_at,
            expires_at: "2026-09-07T01:00:00Z".parse().expect("expiry"),
            core_runtime_instance_id: Some(self.runtime_instance),
        }
    }
    pub(super) fn allow(&self) -> AuthorizationDecision {
        match authorize(&self.input(), &self.backend, None) {
            PolicyAuthorization::Allow { authorization } => authorization,
            other => panic!("expected allow: {other:?}"),
        }
    }
    pub(super) fn request(&self, authority: &AuthorizationDecision) -> DispatchRequest {
        let plan = ExecutionPlan::new(PreparedExecution {
            operation_fingerprint: self.op.fingerprint.clone(),
            backend: self.backend.clone(),
            preparation: serde_json::json!({"revision":1}),
        })
        .expect("plan");
        let binding = DispatchBinding {
            attempt: AttemptRef {
                operation_id: self.op.operation_id,
                ordinal: std::num::NonZeroU32::MIN,
            },
            operation_fingerprint: self.op.fingerprint.clone(),
            authorization_fingerprint: authority.fingerprint().expect("auth hash"),
            plan_fingerprint: plan.fingerprint.clone(),
            cancellation_revision: Revision::INITIAL,
        };
        DispatchRequest { operation: self.op.clone(), plan, binding }
    }
    pub(super) fn approve(&mut self, lifetime: AuthorizationLifetime) {
        let PolicyAuthorization::Ask { proposal } = authorize(&self.input(), &self.backend, None)
        else {
            panic!("ask")
        };
        let mut scope = proposal.scope;
        scope.lifetime = lifetime.clone();
        // A session answer records the reusable file consent, exactly as the production approval
        // resolution does. Without it a session Grant stays an exact-operation approval, which is
        // why a genuinely new call to the same target used to be asked about again.
        if let AuthorizationLifetime::Session { session_id } = lifetime {
            let mut targets = self
                .op
                .content
                .resources
                .iter()
                .filter_map(aura_security_contract::file_consent_target_of)
                .collect::<Vec<_>>();
            if !targets.is_empty() {
                targets.sort();
                targets.dedup();
                scope.file_consent = Some(aura_security_contract::FileSessionConsentRef {
                    approval_id: aura_domain_common::ApprovalRequestId::new(),
                    session_id,
                    principal_id: self.op.content.identity.principal_id,
                    capabilities: self.op.content.capabilities.clone(),
                    targets,
                    approved_operation_fingerprint: self.op.fingerprint.clone(),
                    grant_revision: Revision::INITIAL,
                    revoked_grant_revision: None,
                });
            }
        }
        let grant = AuthorizationGrant {
            grant_id: aura_domain_common::PermissionGrantId::new(),
            revision: Revision::INITIAL,
            scope,
            issued_at: self.input().now,
            expires_at: Some(self.input().expires_at),
            revoked_at: None,
            consumed_at: None,
        };
        self.snapshot.active_grant_ids.push(grant.grant_id);
        self.grants.push(grant);
    }
}
pub(super) fn cancel() -> CancellationState {
    CancellationState { revision: Revision::INITIAL, requested: false }
}
