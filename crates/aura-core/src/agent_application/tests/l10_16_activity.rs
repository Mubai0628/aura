//! The sidebar asks Core which
//! conversations have a turn under way; a turn the model is answering shows, and a cancelled one
//! leaves.
use super::*;

async fn activity(runtime: &crate::RuntimeState) -> AgentSessionsActivity {
    serde_json::from_value(
        crate::r03_tests::success(runtime, "agent.sessions.activity", json!({})).await,
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
    assert_eq!(
        activity(&runtime).await.sessions,
        [AgentSessionActivity {
            session_id: request.session_id,
            state: AgentSessionActivityState::Running
        }]
    );

    crate::r03_tests::success(&runtime, "agent.cancel", json!(request)).await;
    assert!(activity(&runtime).await.sessions.is_empty(), "a cancelled turn is not under way");
}
