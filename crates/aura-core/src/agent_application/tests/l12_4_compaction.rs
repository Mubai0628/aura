//! A compaction that falls short never ends the turn. When the request still does not fit, older
//! tool results are cleared to make room, and when nothing is left to clear the step waits for the
//! user's decision. Every compaction here is answered in shape but far past the length it was
//! asked for, as a real model did twice in a row.
use super::l10_15_compaction::{compactions, start};
use super::l10_19_compaction::{
    asks_compaction, before_the_third, cleared, last_two_steps, resize,
};
use super::*;

/// The compactions of a conversation that completed.
fn completed(runtime: &crate::RuntimeState, session: SessionId) -> usize {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let store = services.broker.lock().expect("V2 store");
    store
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM context_compactions WHERE session_id=?1 AND state='completed'",
            [session.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .expect("compaction count") as usize
}

#[tokio::test]
async fn a_compaction_that_falls_short_waits_for_the_user_instead_of_ending_the_turn() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario_with_long_compactions(Scenario::CompactOnPrefix).await;
    let (runtime, session, model) = before_the_third(&host, &provider, 64_000).await;
    let third = start(&runtime, session, model, "AURA-L124-THIRD: answer at length").await;
    // Ending the turn instead fails here, with the stored error, as an unexpected termination.
    waiting::wait(&runtime, &third, |v| {
        matches!(v.run.state, AgentRunState::AwaitingContextRemediation { .. })
    })
    .await;
    assert_eq!(compactions(&runtime, session), 1, "one pass was recorded");
    assert_eq!(completed(&runtime, session), 0, "and it did not complete");
    let requests = provider.requests.lock().expect("provider requests").clone();
    let asked = requests.iter().filter(|request| asks_compaction(request)).count();
    assert_eq!(asked, 2, "one pass: two attempts, and it is not asked again in the step");
}

#[tokio::test]
async fn a_compaction_that_falls_short_clears_old_results_and_the_turn_goes_on() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario_with_long_compactions(Scenario::ClearOldResults).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    for index in 0..=10 {
        let file = host.root.path().join("waiting-workspace").join(format!("read-{index}.txt"));
        let text = format!("file {index:02} ") + &"read one by one ".repeat(436);
        std::fs::write(file, text).expect("a file a step reads");
    }
    let run = start(&runtime, session, model, "AURA-L124-READS: read the files in order").await;
    tokio::time::timeout(std::time::Duration::from_secs(30), provider.parked.notified())
        .await
        .expect("the tenth request is parked");
    // As in `a_request_over_the_line_is_compacted_deeply_before_any_old_result_is_cleared`: the
    // eleventh request misses the room it must leave for the output by about half a step.
    let (ninth, tenth) = last_two_steps(&runtime, session).await;
    let usable = tenth + (tenth - ninth) / 2 + 32_000;
    let window = (usable * 100).div_ceil(95).max(usable + 4_096);
    resize(&runtime, model, window, 64_000).await;
    provider.release_window.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(30), provider.parked.notified())
        .await
        .expect("the eleventh request is parked");
    provider.release_window.notify_one();
    waiting::wait(&runtime, &run, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    assert_eq!(compactions(&runtime, session), 1, "one pass was recorded");
    assert_eq!(completed(&runtime, session), 0, "and it did not complete");
    let requests = provider.requests.lock().expect("provider requests").clone();
    assert_eq!(requests.iter().filter(|request| asks_compaction(request)).count(), 2);
    assert!(
        requests.iter().any(|request| !cleared(request).is_empty()),
        "older results were cleared to make room"
    );
}
