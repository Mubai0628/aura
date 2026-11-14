//! Reasoning that keeps coming back to one paragraph before any answer is stopped, and the step is
//! asked again as after a broken stream, with a note at the end of the request that says why the
//! attempt before it stopped; a later answer completes the turn.
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
    let requests = provider.requests.lock().expect("provider requests").clone();
    assert_eq!(requests.len(), 3, "two attempts that looped, then the answer");
    assert!(!requests[0].to_string().contains("previous attempt"), "the first request has no note");
    for retry in &requests[1..] {
        let messages = retry["messages"].as_array().cloned().unwrap_or_default();
        let note = messages.last().expect("the retry's last message");
        assert_eq!(note["role"], "user", "{note}");
        let text = note["content"].as_str().unwrap_or_default();
        assert!(
            text.contains("reasoning kept returning to the same passage")
                && text.contains("Decide the next step early"),
            "each retry ends with the note on why the attempt before it stopped: {text}"
        );
    }
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

/// Each step asked again leaves one line in Core's log with the failure's code, its order in the
/// streak and the provider's request id, since the conversation keeps only the answer that came
/// after it. Nothing the model or the provider wrote is in the line.
#[tokio::test]
async fn each_step_asked_again_leaves_its_cause_in_the_core_log() {
    let folder = tempfile::tempdir().expect("log folder");
    let log = folder.path().join("core.jsonl");
    let _route = crate::CoreLogRoute::to(log.clone());
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::ReasoningLoopThenText).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let run = start(&runtime, session, model, "AURA-L124-LOOP: answer briefly").await;
    waiting::wait(&runtime, &run, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let asked: Vec<serde_json::Value> = text
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|line| line["event"] == "model.step_asked_again")
        // The log is Core's own: other tests running at the same time write to it too.
        .filter(|line| line["fields"]["agent_run_id"] == run.agent_run_id.to_string())
        .collect();
    assert_eq!(asked.len(), 2, "one line per step asked again: {text}");
    for (streak, line) in asked.iter().enumerate() {
        let fields = &line["fields"];
        assert_eq!(fields["code"], "model_reasoning_loop", "{line}");
        assert_eq!(fields["streak"], streak, "{line}");
        assert_eq!(fields["backup_connection"], false, "{line}");
        assert!(
            fields["model_step_id"].is_string() && fields["agent_run_id"].is_string(),
            "{line}"
        );
        assert!(fields.get("provider_request_id").is_some(), "{line}");
        assert!(!line.to_string().contains("AURA-L124-LOOP"), "no conversation text: {line}");
    }
}
