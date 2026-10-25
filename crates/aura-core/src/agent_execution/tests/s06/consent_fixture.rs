use super::*;

pub(super) fn approve_file(
    f: &Fixture,
    call: &CallContext,
) -> aura_security_contract::AuthorizationGrant {
    use aura_security_contract::{ApprovalDecisionKind, ApprovalState, AuthorizationApproval};
    let engine = f.service().agent_execution_required();
    let backend = engine.broker_backend(call).expect("file broker").descriptor();
    let mut policy = call.policy(UtcTimestamp::now(), engine.runtime);
    policy.profile = SecurityProfile::Guided;
    let PolicyAuthorization::Ask { proposal } =
        aura_permission_engine::policy::authorize(&policy, &backend, None)
    else {
        panic!("explicit file approval proposal");
    };
    let now = UtcTimestamp::now();
    let request = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: now,
        expires_at: UtcTimestamp::from_value(now.value() + time::Duration::minutes(10)),
    };
    f.service()
        .broker
        .lock()
        .expect("journal")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::security::insert_approval(tx, &request)?;
            aura_persistence_v2::security_approval::resolve(
                tx,
                request.approval_id,
                request.revision,
                &call.operation,
                ApprovalDecisionKind::AllowSession,
                backend.class,
                call.snapshot.session_permission_revision,
                engine.runtime,
                now,
            )
        })
        .expect("durable file approval")
        .expect("issued grant")
}

pub(super) async fn set_mode(f: &Fixture, mode: PermissionMode) {
    let projection =
        f.service().persistence.session_permission_state(f.turn.session_id).expect("mode");
    crate::r03_tests::success(
        &f.runtime,
        "permission.set_session_mode",
        json!({
            "session_id": f.turn.session_id, "expected_revision": projection.revision, "mode": mode,
        }),
    )
    .await;
}
