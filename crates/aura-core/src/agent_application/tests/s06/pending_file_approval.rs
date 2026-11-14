use super::super::provider_fixture::Scenario;
use super::super::waiting::{prepare_in_mode, start, wait};
use super::*;

#[tokio::test]
async fn s06_b4_pending_file_answer_remains_usable_after_both_mode_changes() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Write).await;
    let (runtime, session, model) =
        prepare_in_mode(&host, &provider, PermissionMode::ReadOnly).await;
    let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    let pending = wait(&runtime, &request, |p| !p.pending_approval_ids.is_empty()).await;
    let id = pending.pending_approval_ids[0];
    for mode in [PermissionMode::FullAccess, PermissionMode::ReadOnly] {
        let old = crate::r03_tests::success(
            &runtime,
            "permission.get_session_state",
            json!({"session_id":session}),
        )
        .await;
        crate::r03_tests::success(
            &runtime,
            "permission.set_session_mode",
            json!({
                "session_id":session, "expected_revision":old["revision"], "mode":mode,
            }),
        )
        .await;
    }
    let view = crate::r03_tests::success(
        &runtime,
        "agent.approval.get",
        json!({"session_id":session,"approval_id":id}),
    )
    .await;
    crate::r03_tests::success(&runtime, "agent.approval.resolve", json!({
        "session_id":session,"approval_id":id,"expected_revision":view["approval"]["revision"],"decision":"allow_session",
    })).await;
    let completed =
        wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    assert!(!completed.cancellation_requested);
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let folder = services
        .persistence
        .workspace(services.persistence.workspace_of_session(session).expect("its Workspace"))
        .expect("read the Workspace")
        .canonical_root;
    let approvals = crate::r03_tests::success(
        &runtime,
        "security.file_session_consent.list",
        json!({"session_id":session,"include_inactive":true}),
    )
    .await;
    assert_eq!(
        approvals[0]["scope"]["workspace_root"],
        json!(folder),
        "the answer records the folder its Workspace was at: {approvals}"
    );
}
