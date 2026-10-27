//! The history a conversation polls changes revision only when its own page changes: another
//! conversation streaming a whole turn leaves it as it was, and its own turn moves it.
use super::*;

async fn revision(runtime: &crate::RuntimeState, session: SessionId) -> Revision {
    let value = crate::r03_tests::success(
        runtime,
        "agent.history",
        json!({"session_id": session, "before_turn_task_id": null, "within_turn": null, "limit": 10}),
    )
    .await;
    serde_json::from_value::<AgentSessionProjection>(value).expect("history").revision
}

async fn finish_a_turn(runtime: &crate::RuntimeState, session: SessionId, model: ModelProfileId) {
    let start = super::start(runtime, session, model, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    wait(runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
}

#[tokio::test]
async fn another_conversation_streaming_a_turn_leaves_this_ones_history_revision() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let runtime = host.start().await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("waiting-workspace");
    std::fs::create_dir(&root).expect("workspace folder");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Waiting"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let mine = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let other = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;

    let before = revision(&runtime, mine).await;
    finish_a_turn(&runtime, other, model).await;
    assert_eq!(revision(&runtime, mine).await, before, "another conversation's turn moved it");
    finish_a_turn(&runtime, mine, model).await;
    let after = revision(&runtime, mine).await;
    assert!(after > before, "its own turn must move it");
    assert_eq!(revision(&runtime, mine).await, after, "nothing changed between two reads");
}
