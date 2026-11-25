//! `/compact` while nothing runs starts a
//! one-off task. Its message is the command with the user's note; the compaction is requested for
//! the task's run as it is created and Core takes it before the first step; Core completes the command from its receipt. While a turn runs it is refused, as `/init` is.
use super::*;
use provider_fixture::Scenario;

async fn start_compaction(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
    focus: Option<&str>,
) -> Value {
    crate::r03_tests::request(
        runtime,
        "agent.compaction.start",
        json!(AgentCompactionStartRequest {
            computer_mode: None,
            session_id: session,
            model_profile_id: model,
            focus: focus.map(str::to_owned),
        }),
    )
    .await
}

async fn idle(runtime: &crate::RuntimeState, run: AgentRunId) {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.agent_required().idle(run).await.expect("completion boundary");
}

fn manual_request(
    runtime: &crate::RuntimeState,
    request: &AgentRunRequest,
) -> Option<aura_context_domain::ManualCompactionRequestRecord> {
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { panic!("Core ready") };
    let store = s.broker.lock().expect("V2 store");
    aura_persistence_v2::context::manual_compaction::status(store.database.connection(), request)
        .expect("compaction status")
        .request
}

#[tokio::test]
async fn compact_in_an_idle_session_starts_a_task_that_compacts_before_its_first_step() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let first = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let first = AgentRunRequest { session_id: session, agent_run_id: first.run.agent_run_id };
    wait(&runtime, &first, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    idle(&runtime, first.agent_run_id).await;
    let asked = provider.requests.lock().expect("requests").len();

    let response = start_compaction(&runtime, session, model, Some("保留测试命令")).await;
    let run: AgentRunProjection = serde_json::from_value(response["result"]["value"].clone())
        .unwrap_or_else(|_| panic!("the compaction task starts: {response}"));
    assert_eq!(run.turn.mode, AgentTaskMode::Normal);
    let request = AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id };
    let done =
        wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    // Requested with the run, and taken at its first safe point: nothing was left waiting.
    let manual = manual_request(&runtime, &request).expect("the run's compaction request");
    assert_eq!(manual.state, aura_context_domain::ManualCompactionRequestState::Completed);
    assert!(manual.focus_artifact_id.is_some(), "the user's note is the compaction's focus");
    assert_eq!(done.model_steps.len(), 1, "the command closes its unused preparation slot");
    let requests = provider.requests.lock().expect("requests").clone();
    assert!(manual.source_context_snapshot_id.is_none(), "this short history fits retention");
    assert_eq!(requests.len(), asked, "a no-op needs no model confirmation");
}

#[tokio::test]
async fn compact_is_refused_as_a_task_while_a_turn_runs() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Query).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let first = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let first = AgentRunRequest { session_id: session, agent_run_id: first.run.agent_run_id };
    // The turn waits for the user's answer: it is still the session's active turn.
    wait(&runtime, &first, |p| p.pending_query.is_some()).await;
    let refused = start_compaction(&runtime, session, model, None).await;
    assert!(
        refused["error"].to_string().contains("agent.compaction_start_requires_idle"),
        "a running turn takes its compaction at a safe point instead: {refused}"
    );
}

#[tokio::test]
async fn manual_compaction_reuses_the_sessions_tool_declarations_without_a_confirmation_call() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    super::l10_19_compaction::resize(&runtime, model, 1_000_000, 4096).await;
    for part in 0..3 {
        let records = (0..2200)
            .map(|i| format!("record-{part}-{i:016x} verified"))
            .collect::<Vec<_>>()
            .join("\n");
        let turn = super::l10_15_compaction::start(&runtime, session, model, &records).await;
        wait(&runtime, &turn, |v| v.turn.state.terminal()).await;
        idle(&runtime, turn.agent_run_id).await;
    }
    let prior_tokens = super::l10_19_compaction::used(&runtime, session, 2).await;
    let window = (prior_tokens + 20_000) * 100 / 95;
    super::l10_19_compaction::resize(&runtime, model, window, 4096).await;
    let before = provider.requests.lock().expect("ordinary requests").clone();
    let started = start_compaction(&runtime, session, model, Some("keep the requirements")).await;
    let command: AgentRunProjection = serde_json::from_value(started["result"]["value"].clone())
        .expect("valid compaction fixture and receipt");
    let done = wait(
        &runtime,
        &AgentRunRequest { session_id: session, agent_run_id: command.run.agent_run_id },
        |v| v.turn.state.terminal(),
    )
    .await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }));
    let after = provider.requests.lock().expect("valid compaction fixture and receipt").clone();
    assert_eq!(after.len(), before.len() + 1);
    let summary = after.last().expect("valid compaction fixture and receipt");
    assert!(super::l10_19_compaction::asks_compaction(summary));
    let prior = before.last().expect("adopted request");
    assert_eq!(prior["tools"], summary["tools"]);
    assert!(
        summary["messages"]
            .as_array()
            .expect("compaction messages")
            .starts_with(prior["messages"].as_array().expect("adopted messages")),
        "manual compaction replaced a reusable adopted prefix"
    );
}
