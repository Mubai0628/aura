//! Reasoning that keeps coming back to one paragraph before any answer is stopped, and the step is
//! asked again as after a broken stream; a later answer completes the turn.
use super::l10_15_compaction::start;
use super::*;

#[tokio::test]
async fn reasoning_that_loops_is_stopped_and_the_step_asked_again() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::ReasoningLoopThenText).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let run = start(&runtime, session, model, "AURA-L124-LOOP: answer briefly").await;
    waiting::wait(&runtime, &run, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    assert_eq!(
        provider.requests.lock().expect("provider requests").len(),
        3,
        "two attempts that looped, then the answer"
    );
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let store = services.broker.lock().expect("V2 store");
    let loops: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind='model.failure' AND payload LIKE '%model_reasoning_loop%'",
            [],
            |row| row.get(0),
        )
        .expect("loop failures");
    assert_eq!(loops, 2, "each looping attempt was stopped as a reasoning loop");
}
