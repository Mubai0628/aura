//! A step cut off because the provider sent nothing for the stream idle time after its answer
//! began is asked again with a note at the very end of the request, and waits longer for the
//! provider's next bytes. A turn the provider stopped tells the next turn why in words.
use super::*;

/// An idle time short enough for the cut to happen inside the test.
const TIMEOUTS: ModelEndpointTimeouts = ModelEndpointTimeouts {
    connect_ms: 5_000,
    first_event_ms: 10_000,
    stream_idle_ms: 1_200,
    total_ms: Some(60_000),
};

async fn session(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
) -> (crate::RuntimeState, SessionId, ModelProfileId) {
    let runtime = host.start().await;
    let model =
        profile_for_with(&runtime, &provider.url, ModelApiFormat::OpenAiChatCompletions, TIMEOUTS)
            .await;
    let root = host.root.path().join("idle-workspace");
    std::fs::create_dir(&root).expect("workspace folder");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Idle"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace summary");
    let session = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    (runtime, session, model)
}

/// The retry is the cut-off request with one user message added after everything, so the
/// provider's cache of the request reaches the note; it pauses longer than the connection's idle
/// time and still completes, and only the retry carries the note.
#[tokio::test]
async fn a_stream_idle_cut_is_asked_again_with_a_note_at_the_end_and_a_longer_wait() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::SilentThenText { silences: 1 }).await;
    let (runtime, session, model) = session(&host, &provider).await;
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    waiting::wait(&runtime, &request, done).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    assert_eq!(requests.len(), 2, "the cut-off attempt and one retry");
    let messages = |request: &Value| request["messages"].as_array().cloned().unwrap_or_default();
    let (first, retry) = (messages(&requests[0]), messages(&requests[1]));
    let note = retry.last().expect("the retry's last message");
    assert_eq!(note["role"], "user", "{note}");
    let text = note["content"].as_str().expect("the note's text");
    assert!(text.contains("cut off") && text.contains("smaller calls"), "{text}");
    assert!(!requests[0].to_string().contains("cut off"), "the first request has no note");
    let shared = first.len() - 1;
    assert_eq!(retry[..shared], first[..shared], "the conversation is sent as it was");
    assert_eq!(retry.len(), first.len() + 1, "the task state, then the note");
}

/// A turn the provider stopped after its retries is sent to the next turn with a note that says
/// in words what the provider did, not a bare stop code.
#[tokio::test]
async fn a_turn_the_provider_stopped_tells_the_next_turn_why_in_words() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::OverloadedThenText { failures: 3 }).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    let stopped =
        |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::RecoveryRequired { .. });
    waiting::wait(&runtime, &request, stopped).await;
    let next = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: next.run.agent_run_id };
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    waiting::wait(&runtime, &request, done).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    let last = requests.last().expect("the next turn's request").to_string();
    assert!(last.contains("the model provider could not answer its last step ("), "{last}");
    assert!(!last.contains("(agent.execution_interrupted)"), "{last}");
}
