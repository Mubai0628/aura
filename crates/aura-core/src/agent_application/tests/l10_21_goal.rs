//! DeepSeek finished the
//! game and called goal.complete citing the files it wrote as evidence. Aura refused the call before
//! it ran - the evidence had to be artifact ids, which the schema let through as any string, and
//! decoding them failed as "persistent JSON is invalid" - so the model ended the turn with
//! normal.complete and the Goal stayed open.
use super::*;

/// Runs one Goal turn on `scenario` and returns the turn's Goal and how many requests it took.
async fn goal_after(scenario: Scenario) -> (aura_agent_domain::GoalState, usize) {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(scenario).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Goal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let done =
        wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let asked = provider.requests.lock().expect("requests").len();
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let store = services.broker.lock().expect("journal");
    let goal =
        aura_persistence_v2::agent::work::goal(store.database.connection(), done.turn.turn_task_id)
            .expect("the turn's Goal");
    (goal, asked)
}

#[tokio::test]
async fn a_goal_completed_citing_the_files_it_wrote_is_complete() {
    let (goal, asked) = goal_after(Scenario::GoalWithPaths).await;
    assert!(goal.completed, "goal.complete ran: {goal:?}");
    assert_eq!(asked, 1, "the completion is the turn's only step");
}

#[tokio::test]
async fn an_artifact_id_that_is_not_this_runs_is_still_refused() {
    let (goal, asked) = goal_after(Scenario::GoalWithUnknownArtifact).await;
    assert!(!goal.completed, "an unknown artifact is no evidence: {goal:?}");
    assert_eq!(asked, 2, "refused, then the turn ends through normal.complete");
}
