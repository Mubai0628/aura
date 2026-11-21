//! A complete background can be longer than its suggestion, including in a running tool loop.
use super::l10_15_compaction::{compactions, start};
use super::l10_19_compaction::{
    PARKED_WITHIN, asks_compaction, before_the_third, last_two_steps, resize,
};
use super::*;

/// The compactions of a conversation that completed.
pub(super) fn completed(runtime: &crate::RuntimeState, session: SessionId) -> usize {
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
async fn a_long_model_summary_replaces_history_without_a_second_request() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario_with_long_compactions(Scenario::CompactOnPrefix).await;
    let (runtime, session, model) = before_the_third(&host, &provider, 64_000).await;
    let third = start(&runtime, session, model, "AURA-L124-THIRD: answer at length").await;
    waiting::wait(&runtime, &third, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    assert_eq!(compactions(&runtime, session), 1, "one pass was recorded");
    assert_eq!(completed(&runtime, session), 1, "the full model background was committed");
    let requests = provider.requests.lock().expect("provider requests").clone();
    assert_eq!(requests.iter().filter(|request| asks_compaction(request)).count(), 1);
    let after = requests
        .iter()
        .rfind(|request| !asks_compaction(request))
        .expect("the step's request after the compaction")
        .to_string();
    assert!(!after.contains("moved out of the context without a new summary"));
    assert!(
        after.contains("Everything that happened, retold at length."),
        "the provider summary is used in the next request"
    );
}

#[tokio::test]
async fn a_long_model_background_keeps_the_tool_loop_running() {
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
    tokio::time::timeout(PARKED_WITHIN, provider.parked.notified())
        .await
        .expect("the tenth request is parked");
    // As in `a_request_over_the_line_is_compacted_deeply_before_any_old_result_is_cleared`: the
    // eleventh request misses the room it must leave for the output by about half a step.
    let (ninth, tenth) = last_two_steps(&runtime, session).await;
    let usable = tenth + (tenth - ninth) / 2 + 32_000;
    let window = (usable * 100).div_ceil(95).max(usable + 4_096);
    resize(&runtime, model, window, 64_000).await;
    provider.release_window.notify_one();
    tokio::time::timeout(PARKED_WITHIN, provider.parked.notified())
        .await
        .expect("the eleventh request is parked");
    provider.release_window.notify_one();
    waiting::wait(&runtime, &run, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    assert_eq!(compactions(&runtime, session), 1, "one pass was recorded");
    assert_eq!(completed(&runtime, session), 1, "the model background was committed");
    let requests = provider.requests.lock().expect("provider requests").clone();
    assert_eq!(requests.iter().filter(|request| asks_compaction(request)).count(), 1);
}
