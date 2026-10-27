//! The sidebar asks Core which
//! conversations have a turn under way; a turn the model is answering shows, and a cancelled one
//! leaves. Asked about, the turn that left says how it ended, for the notifications.
use super::*;

async fn activity(
    runtime: &crate::RuntimeState,
    ended: &[aura_domain_common::TurnTaskId],
) -> AgentSessionsActivity {
    serde_json::from_value(
        crate::r03_tests::success(
            runtime,
            "agent.sessions.activity",
            json!({"ended_turns": ended}),
        )
        .await,
    )
    .expect("activity")
}

#[tokio::test]
async fn the_sidebar_sees_a_turn_under_way_until_it_ends() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Hold).await;
    let (runtime, start) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    waiting::wait(&runtime, &request, |p| {
        matches!(p.run.state, AgentRunState::ModelRunning { .. })
    })
    .await;
    let turn = start.turn.turn_task_id;
    assert_eq!(
        activity(&runtime, &[]).await.sessions,
        [AgentSessionActivity {
            session_id: request.session_id,
            state: AgentSessionActivityState::Running,
            turn_task_ids: vec![turn],
            waiting_for: None,
        }]
    );
    assert!(activity(&runtime, &[turn]).await.ended.is_empty(), "it has not ended");

    crate::r03_tests::success(&runtime, "agent.cancel", json!(request)).await;
    let after = activity(&runtime, &[turn]).await;
    assert!(after.sessions.is_empty(), "a cancelled turn is not under way");
    assert_eq!(
        after.ended,
        [AgentTurnEnded {
            session_id: request.session_id,
            turn_task_id: turn,
            outcome: AgentTurnOutcome::Cancelled,
        }]
    );
}
