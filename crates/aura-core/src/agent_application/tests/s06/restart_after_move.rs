//! A Core that stops between a Workspace move and the withdrawal after it finishes that withdrawal
//! at its next start, so the settings never list an approval from the old folder as in force.
//! The Workspace is pointed at a copy of its folder: the old folder stays with the approved file
//! in it, which is when the list would still show the approval in force.
use super::super::provider_fixture::Scenario;
use super::super::waiting::{prepare_in_mode, start, wait};
use super::*;
use aura_domain_common::SessionId;
use serde_json::Value;

#[tokio::test]
async fn a_restart_withdraws_the_file_approvals_a_move_left_in_force() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Write).await;
    let (runtime, session, model) =
        prepare_in_mode(&host, &provider, PermissionMode::ReadOnly).await;
    let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    let pending = wait(&runtime, &request, |p| !p.pending_approval_ids.is_empty()).await;
    let id = pending.pending_approval_ids[0];
    let view = crate::r03_tests::success(
        &runtime,
        "agent.approval.get",
        json!({"session_id":session,"approval_id":id}),
    )
    .await;
    crate::r03_tests::success(&runtime, "agent.approval.resolve", json!({
        "session_id":session,"approval_id":id,"expected_revision":view["approval"]["revision"],"decision":"allow_session",
    })).await;
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let workspace = services.persistence.workspace_of_session(session).expect("its Workspace");
    crate::r03_tests::workspace_moves::point_at_copy_without_withdrawal(
        services,
        workspace,
        &host.root.path().join("waiting-workspace"),
    );
    let listed = approvals(&runtime, session).await;
    assert_eq!(
        listed[0]["availability"],
        json!("in_force"),
        "a stop right after the move leaves the approval listed in force: {listed}"
    );
    let runtime = host.restart(runtime).await;
    let listed = approvals(&runtime, session).await;
    assert_eq!(
        listed[0]["availability"],
        json!("revoked"),
        "the next start withdraws the approval given at the old folder: {listed}"
    );
}

async fn approvals(runtime: &crate::RuntimeState, session: SessionId) -> Value {
    crate::r03_tests::success(
        runtime,
        "security.file_session_consent.list",
        json!({"session_id":session,"include_inactive":true}),
    )
    .await
}
