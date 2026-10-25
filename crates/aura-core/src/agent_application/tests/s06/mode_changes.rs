use super::super::provider_fixture::Scenario;
use super::super::waiting::{prepare_in_mode, start, wait};
use super::*;

async fn change(runtime: &crate::RuntimeState, session: SessionId, mode: PermissionMode) {
    let before = crate::r03_tests::success(
        runtime,
        "permission.get_session_state",
        json!({"session_id":session}),
    )
    .await;
    crate::r03_tests::success(
        runtime,
        "permission.set_session_mode",
        json!({"session_id":session,"expected_revision":before["revision"],"mode":mode}),
    )
    .await;
}

#[tokio::test]
async fn s06_e3_mode_changes_preserve_model_generation() {
    for (before, after) in [
        (PermissionMode::ReadOnly, PermissionMode::FullAccess),
        (PermissionMode::FullAccess, PermissionMode::ReadOnly),
    ] {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(Scenario::Hold).await;
        let (runtime, session, model) = prepare_in_mode(&host, &provider, before).await;
        let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
        let request =
            AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
        wait(&runtime, &request, |p| matches!(p.run.state, AgentRunState::ModelRunning { .. }))
            .await;
        change(&runtime, session, after).await;
        let view: AgentRunProjection = serde_json::from_value(
            crate::r03_tests::success(&runtime, "agent.status", json!(request)).await,
        )
        .expect("production status");
        assert!(
            matches!(view.run.state, AgentRunState::ModelRunning { .. }),
            "{before:?} -> {after:?} cancelled model generation: {:?}",
            view.run.state
        );
        crate::r03_tests::success(&runtime, "agent.cancel", json!(request)).await;
    }
}

#[tokio::test]
async fn s06_e3_mode_changes_preserve_the_pending_user_question() {
    for (before, after) in [
        (PermissionMode::ReadOnly, PermissionMode::FullAccess),
        (PermissionMode::FullAccess, PermissionMode::ReadOnly),
    ] {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(Scenario::Query).await;
        let (runtime, session, model) = prepare_in_mode(&host, &provider, before).await;
        let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
        let request =
            AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
        let waiting = wait(&runtime, &request, |p| {
            matches!(p.turn.state, TurnTaskState::AwaitingQuery { .. })
        })
        .await;
        change(&runtime, session, after).await;
        let view: AgentRunProjection = serde_json::from_value(
            crate::r03_tests::success(&runtime, "agent.status", json!(request)).await,
        )
        .expect("production status");
        assert_eq!(
            view.pending_query, waiting.pending_query,
            "mode change removed the user's question"
        );
        assert!(
            matches!(view.turn.state, TurnTaskState::AwaitingQuery { .. }),
            "{before:?} -> {after:?} cancelled the waiting turn: {:?}",
            view.turn.state
        );
        let question = view.pending_query.expect("question preserved");
        crate::r03_tests::success(
            &runtime,
            "agent.query.answer",
            json!(AgentQueryAnswerRequest {
                session_id: session,
                agent_run_id: request.agent_run_id,
                query_id: question.query_id,
                expected_revision: question.revision,
                answers: vec![QueryAnswer {
                    question_id: "choice".into(),
                    value: QueryAnswerValue::Confirm { accepted: true }
                }],
            }),
        )
        .await;
        wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    }
}
