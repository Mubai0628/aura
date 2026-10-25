//! Where the
//! context was compacted, the conversation says so. The step prepared after a compaction carries
//! the marker with how much recent history stayed as it was; no other step does.
use super::*;

#[tokio::test]
async fn the_step_prepared_after_a_compaction_says_the_context_was_compacted() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Compact).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let page: AgentSessionProjection = serde_json::from_value(
        crate::r03_tests::success(
            &runtime,
            "agent.history",
            json!({"session_id": request.session_id, "limit": 10}),
        )
        .await,
    )
    .expect("history page");
    let turn = page.turns.last().expect("the turn");
    assert!(turn.next_messages.is_none(), "every step of the turn is on the page");
    let marked = turn
        .assistant_messages
        .iter()
        .filter_map(|step| {
            step.context_compacted.clone().map(|marker| (step.model_step_id, marker))
        })
        .collect::<Vec<_>>();
    assert_eq!(marked.len(), 1, "one compaction, one marker: {marked:?}");
    let (step, marker) = &marked[0];
    // The fixture's compaction is Core's own, near the window's limit; nobody asked for it.
    assert!(!marker.manual);
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { unreachable!() };
    let store = s.broker.lock().expect("V2 store");
    let compaction = aura_persistence_v2::context::compaction::latest(
        store.database.connection(),
        request.session_id,
    )
    .expect("compaction record")
    .expect("a completed compaction");
    assert_eq!(compaction.source_step_id, *step, "the marker is on the step prepared after it");
    assert_eq!(marker.retained_recent_tokens, compaction.retained_recent_tokens);
    // It says how large the request was counted before and after.
    let usage = turn
        .assistant_messages
        .iter()
        .find(|message| message.model_step_id == *step)
        .and_then(|message| message.context_usage.as_ref())
        .expect("the step's meter");
    assert_eq!(marker.input_tokens_after, Some(usage.used_input_tokens));
    let before = marker.input_tokens_before.expect("the size before");
    assert!(usage.used_input_tokens < before, "{before} -> {}", usage.used_input_tokens);
}
