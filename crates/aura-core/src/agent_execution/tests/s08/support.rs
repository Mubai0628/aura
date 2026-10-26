use super::*;

pub(super) fn arguments(script: &str, network: bool) -> serde_json::Value {
    json!({"script":script,"requires_network":network,"working_directory":".","stdin":"","timeout_ms":10000,"max_output_bytes":4096})
}
/// The question a call gets before any answer, from the real backend it would run on.
pub(in crate::agent_execution::tests) async fn ask(
    f: &Fixture,
    call: &CallContext,
) -> aura_security_contract::AuthorizationProposal {
    f.service().commands_required().wait_until_ready().await.expect("managed runtime verified");
    let engine = f.service().agent_execution_required();
    let backend =
        f.service().commands_required().candidates_for(call.snapshot.mode)[0].descriptor();
    match aura_permission_engine::policy::authorize(
        &call.policy(UtcTimestamp::now(), engine.runtime),
        &backend,
        None,
    ) {
        PolicyAuthorization::Ask { proposal } => proposal,
        other => panic!("the call runs without asking: {other:?}"),
    }
}
pub(in crate::agent_execution::tests) async fn approve(
    f: &Fixture,
    call: &CallContext,
) -> Vec<AuthorizationGrant> {
    use aura_security_contract::{ApprovalState, AuthorizationApproval};
    f.service().commands_required().wait_until_ready().await.expect("managed runtime verified");
    let engine = f.service().agent_execution_required();
    let backend = if call.operation.content.action_kind.starts_with("terminal.") {
        crate::agent_execution::terminal::backend(
            engine.clone(),
            call,
            f.service().commands_required().clone(),
        )
        .expect("real terminal backend")
        .descriptor()
    } else {
        f.service().commands_required().candidates_for(call.snapshot.mode)[0].descriptor()
    };
    let policy = call.policy(UtcTimestamp::now(), engine.runtime);
    let PolicyAuthorization::Ask { proposal } =
        aura_permission_engine::policy::authorize(&policy, &backend, None)
    else {
        panic!("the call lacks a capability slice");
    };
    let now = UtcTimestamp::now();
    let approval = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: now,
        expires_at: UtcTimestamp::from_value(now.value() + time::Duration::minutes(10)),
    };
    let store = &mut f.service().broker.lock().expect("the authoritative journal");
    let grant = store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::security::insert_approval(tx, &approval)?;
            aura_persistence_v2::security_approval::resolve(
                tx,
                approval.approval_id,
                approval.revision,
                &call.operation,
                ApprovalDecisionKind::AllowSession,
                backend.class,
                call.snapshot.session_permission_revision,
                engine.runtime,
                now,
            )
        })
        .expect("resolve the real question")
        .expect("issued authority");
    aura_persistence_v2::security::grants_for_call(
        store.database.connection(),
        &call.operation.fingerprint,
        Some(f.turn.session_id),
    )
    .expect("all split records")
    .into_iter()
    .filter(|value| {
        value.grant_id == grant.grant_id
            || value
                .scope
                .network_consent
                .as_ref()
                .is_some_and(|value| value.approval_id == approval.approval_id)
    })
    .collect()
}
pub(super) fn no_attempt(f: &Fixture, id: ToolCallId) {
    assert!(
        aura_persistence_v2::tool::execution::call(
            f.service().broker.lock().expect("journal").database.connection(),
            id
        )
        .expect("admitted call")
        .attempt_id
        .is_none()
    );
}
pub(super) async fn mode(f: &Fixture, mode: PermissionMode) {
    let before =
        f.service().persistence.session_permission_state(f.turn.session_id).expect("live default");
    crate::r03_tests::success(
        &f.runtime,
        "permission.set_session_mode",
        json!({"session_id":f.turn.session_id,"expected_revision":before.revision,"mode":mode}),
    )
    .await;
}
