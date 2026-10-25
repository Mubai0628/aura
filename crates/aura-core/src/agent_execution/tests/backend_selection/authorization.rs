use super::support::*;
use super::*;
use aura_security_contract::{
    ApprovalDecisionKind, ApprovalState, AuthorizationApproval, AuthorizationGrant,
    AuthorizationLifetime, AuthorizationScope, BackendRequirement, SecurityProfile,
};

#[tokio::test]
async fn readonly_network_shell_asks_for_missing_slices_without_attempt() {
    let (f, context) = case(PermissionMode::ReadOnly, "shell.cmd", true).await;
    let compat = descriptor(&f).await;
    let mut legacy = legacy_restricted(&compat);
    legacy.readiness =
        BackendReadiness::RepairRequired { reason: "runner.core_binding_mismatch.file_id".into() };
    match select(&f, &context, vec![candidate(legacy), candidate(compat)], None).await {
        Ok(Selection::ApprovalRequired { proposal, backend }) => {
            assert_eq!(backend.id.0, aura_backend_windows_user::compat::COMPAT_BACKEND_ID);
            assert_eq!(
                proposal.requested_capabilities,
                Some(vec![ResolvedCapability::ProcessExecute, ResolvedCapability::OrdinaryNetwork])
            );
        }
        other => panic!("the missing slices must be asked on compat: {other:?}"),
    }
    let store = f.service().broker.lock().expect("read the admitted ledger");
    assert!(
        aura_persistence_v2::tool::execution::call(
            store.database.connection(),
            context.validated.call.tool_call_id
        )
        .expect("admitted call")
        .attempt_id
        .is_none()
    );
}

fn running_grants(
    f: &Fixture,
    approved: &CallContext,
    later: &CallContext,
) -> Vec<AuthorizationGrant> {
    let runtime = f.service().agent_execution_required().runtime;
    let at = approved.snapshot.created_at;
    let approval = AuthorizationApproval {
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: AuthorizationScope {
            operation_fingerprint: approved.operation.fingerprint.clone(),
            profile: SecurityProfile::for_permission_mode(approved.snapshot.mode),
            backend_requirement: BackendRequirement {
                allowed_classes: [BackendClass::Native].into(),
                guarantees: Default::default(),
            },
            lifetime: AuthorizationLifetime::Once { operation_id: approved.operation.operation_id },
            consent: None,
            network_consent: None,
            memory_consent: None,
            mcp_consent: None,
            file_consent: None,
            capability_limit: Some(vec![ResolvedCapability::ProcessExecute]),
        },
        allowed_decisions: vec![ApprovalDecisionKind::AllowSession, ApprovalDecisionKind::Deny],
        state: ApprovalState::Pending,
        requested_at: at,
        expires_at: UtcTimestamp::from_value(at.value() + time::Duration::minutes(10)),
    };
    let mut store = f.service().broker.lock().expect("approval store");
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::security::insert_approval(tx, &approval)?;
            aura_persistence_v2::security_approval::resolve(
                tx,
                approval.approval_id,
                Revision::INITIAL,
                &approved.operation,
                ApprovalDecisionKind::AllowSession,
                BackendClass::Native,
                approved.snapshot.session_permission_revision,
                runtime,
                at,
            )
        })
        .expect("resolve the real session approval")
        .expect("mint the explicit running consent");
    aura_persistence_v2::security::grants_for_call(
        store.database.connection(),
        &later.operation.fingerprint,
        later.operation.content.identity.session_id,
    )
    .expect("offer the recorded consent to the later call")
}

#[tokio::test]
async fn a_live_running_consent_selects_compat_and_never_legacy() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["shell.cmd"]).await;
    let args = |script| json!({"script":script,"requires_network":false,"working_directory":".","stdin":"","timeout_ms":1000,"max_output_bytes":4096});
    let batch = f.admit(&[("shell.cmd", args("echo approved")), ("shell.cmd", args("echo later"))]);
    let approved = &batch.calls[&batch.batch.calls[0].tool_call_id];
    let later = &batch.calls[&batch.batch.calls[1].tool_call_id];
    assert_ne!(
        approved.operation.fingerprint, later.operation.fingerprint,
        "a different operation reuses only the explicit scope"
    );
    let grants = running_grants(&f, approved, later);
    assert!(
        grants.iter().any(|grant| grant.scope.consent.is_some()),
        "the production Store records the running answer"
    );
    let compat = descriptor(&f).await;
    let mut policy =
        later.policy(UtcTimestamp::now(), f.service().agent_execution_required().runtime);
    policy.grants = &grants;
    let candidates = vec![candidate(legacy_restricted(&compat)), candidate(compat)];
    assert_compat(
        f.service().agent_execution_required().select_backend(&policy, &candidates, None).await,
    );
}
