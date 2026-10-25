//! The real Transport/Worker branch for an unknown Provider tool call.
//!
//! These cases never call `persist_drive_failure` (or any test helper) directly:
//! the refusal is produced by the production drive loop reading a raw Responses
//! stream over a real loopback endpoint, and the assertions read the projection
//! and the journal the desktop UI reads.

use super::*;
use provider_fixture::{Provider, RESPONSES_WRITE_CALL_ID, RESPONSES_WRITE_TOKEN, Scenario};

#[tokio::test]
async fn an_unknown_wire_name_returns_tool_not_found_and_continues_the_same_turn() {
    let run = start_binding_run().await;
    let request = AgentRunRequest { session_id: run.session, agent_run_id: run.run };
    let done = waiting::wait(&run.runtime, &request, |view| view.turn.state.terminal()).await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }), "{:?}", done.turn.state);
    assert_eq!(done.turn.turn_task_id, run.turn_task_id);
    let facts = binding_facts(&run.runtime);
    assert_eq!(facts.execution_attempts, 1, "only the existing write executes");
    assert!(facts.model_failure_payloads.is_empty());
    assert!(facts.application_error_payloads.is_empty());
    assert_eq!(
        std::fs::read_to_string(run.workspace_root.join("agent-result.txt")).expect("write"),
        RESPONSES_WRITE_TOKEN
    );
    let requests = run.provider.requests.lock().expect("requests");
    let next = &requests[2];
    let result = next["input"]
        .as_array()
        .expect("history")
        .iter()
        .find(|item| item["type"] == "function_call_output" && item["call_id"] == "call_illegal")
        .expect("unknown call has a paired result");
    let envelope: Value =
        serde_json::from_str(result["output"].as_str().expect("result")).expect("envelope");
    assert_eq!(envelope["outcome"]["type"], "not_executed");
    assert_eq!(envelope["outcome"]["reason"], "tool_not_found");
    assert_eq!(envelope["side_effect_state"], "not_started");
    assert!(envelope["definition_ref"].is_null());
    // The model's view of a result names no call identity; the call is the newest one the
    // conversation's activity holds, and the read models below check it is the unknown call.
    assert!(envelope["tool_call_id"].is_null(), "{envelope}");
    let call_id = newest_call(&run);
    assert_unknown_read_models(&run, call_id);
    let message = envelope["outcome"]["error"]["message"].as_str().expect("correction");
    assert!(message.contains("No such tool") && message.contains("_aura"), "{message}");
    assert!(message.contains("Available tools"), "{message}");
    for tool in requests[1]["tools"].as_array().expect("frozen step tools") {
        assert!(message.contains(tool["name"].as_str().expect("wire name")), "{message}");
    }
}

fn newest_call(run: &BindingRun) -> ToolCallId {
    let crate::StartupMode::Ready(services) = &run.runtime.startup_mode else {
        panic!("Core ready")
    };
    let journal = services.broker.lock().expect("binding journal");
    let request = aura_tool_projection::AgentToolActivityRequest {
        session_id: run.session,
        before_call_id: None,
        limit: 1,
        call_ids: None,
    };
    aura_persistence_v2::tool::activity::page(journal.database.connection(), &request)
        .expect("activity")
        .calls[0]
        .summary
        .tool_call_id
}

fn assert_unknown_read_models(run: &BindingRun, call_id: ToolCallId) {
    use aura_persistence_v2::{context::read, tool::activity};
    use aura_tool_contract::ToolCallStatusV1;
    use aura_tool_projection::AgentToolActivityRequest;
    let crate::StartupMode::Ready(services) = &run.runtime.startup_mode else {
        panic!("Core ready")
    };
    let journal = services.broker.lock().expect("binding journal");
    let db = journal.database.connection();
    let request = AgentToolActivityRequest {
        session_id: run.session,
        before_call_id: None,
        limit: 1,
        call_ids: None,
    };
    let page = activity::page(db, &request).expect("activity with an unknown call");
    assert_eq!(page.calls.len(), 1);
    let call = &page.calls[0];
    assert_eq!(call.summary.tool_call_id, call_id);
    assert_eq!(call.summary.display_name, "_aura");
    assert_eq!(call.summary.status, ToolCallStatusV1::Finalized);
    assert!(call.summary.tool_id.is_none() && call.summary.tool_version.is_none());
    assert!(!call.summary.can_cancel && !call.shell_query_available && !call.display.internal);
    assert_eq!(page.next_before_call_id, Some(call_id));
    let exact =
        activity::page(db, &AgentToolActivityRequest { call_ids: Some(vec![call_id]), ..request })
            .expect("exact activity identity");
    assert_eq!(exact.calls.len(), 1);
    // A call the model wrote that is not stored
    // yet - its batch not admitted, or never admitted because the step was interrupted - is left
    // out; it does not fail the read of every other call asked for with it.
    let pending = ToolCallId::new();
    let with_pending = activity::page(
        db,
        &AgentToolActivityRequest { call_ids: Some(vec![pending, call_id]), limit: 2, ..request },
    )
    .expect("the stored calls are read");
    assert_eq!(
        with_pending.calls.iter().map(|c| c.summary.tool_call_id).collect::<Vec<_>>(),
        [call_id]
    );
    let tx = db.unchecked_transaction().expect("result read transaction");
    let result = read::tool_result(
        &tx,
        run.session,
        run.run,
        &aura_context_domain::AgentContextReadToolResultInput {
            tool_call_id: call_id,
            include_input: true,
            cursor: None,
        },
        UtcTimestamp::now(),
    )
    .expect("unknown result stays readable");
    assert_eq!(result.value.tool_name, "_aura");
    assert!(result.value.is_error && result.value.result.contains("tool_not_found"));
    assert!(result.next_cursor.is_none());
}

/// The one write this fixture performs, and the identity of the step that must
/// never be dispatched.
struct BindingRun {
    /// The fixture host owns the temporary root; the workspace and its files
    /// live inside it, so it has to outlive the run.
    _host: crate::r03_tests::Fixture,
    runtime: crate::RuntimeState,
    provider: Provider,
    session: SessionId,
    run: AgentRunId,
    turn_task_id: TurnTaskId,
    workspace_root: std::path::PathBuf,
}

async fn start_binding_run() -> BindingRun {
    start_binding_run_with(Scenario::ResponsesWriteThenIllegalToolName).await
}

async fn start_binding_run_with(scenario: Scenario) -> BindingRun {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(scenario).await;
    let model = profile_responses(&runtime, &provider.url).await;
    let workspace_root = host.root.path().join("agent-binding-workspace");
    std::fs::create_dir(&workspace_root).expect("Agent binding workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":workspace_root,"display_name":"Binding"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("Agent binding workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::WorkspaceReadWrite).await;
    // The queue starts a new turn from the session's binding, so the human
    // continuation later can start without re-choosing a model.
    crate::r03_tests::success(
        &runtime,
        "models.session.set_binding",
        json!({
            "expected_revision":"0",
            "session_id":session,
            "model_profile_id":model
        }),
    )
    .await;
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: "Write the fixture file once, then continue.".into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection = serde_json::from_value(started).expect("Agent binding start");
    BindingRun {
        _host: host,
        runtime,
        provider,
        session,
        run: start.run.agent_run_id,
        turn_task_id: start.turn.turn_task_id,
        workspace_root,
    }
}

/// Counts read straight from the durable facts the product writes.
struct BindingFacts {
    execution_attempts: i64,
    model_failure_payloads: Vec<String>,
    application_error_payloads: Vec<String>,
}

fn binding_facts(runtime: &crate::RuntimeState) -> BindingFacts {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let journal = services.broker.lock().expect("Agent binding journal");
    let db = journal.database.connection();
    let count =
        |sql: &str| -> i64 { db.query_row(sql, [], |row| row.get(0)).expect("binding count") };
    let payloads = |kind: &str| -> Vec<String> {
        let mut q = db
            .prepare("SELECT payload FROM events WHERE kind=?1 ORDER BY sequence")
            .expect("Agent binding query");
        q.query_map([kind], |row| row.get::<_, String>(0))
            .expect("Agent binding query")
            .collect::<Result<Vec<_>, _>>()
            .expect("Agent binding query")
    };
    BindingFacts {
        execution_attempts: count(
            "SELECT COUNT(*) FROM execution_attempts WHERE operation_id IN \
             (SELECT operation_id FROM tool_calls WHERE operation_id IS NOT NULL)",
        ),
        model_failure_payloads: payloads("model.failure"),
        application_error_payloads: payloads("agent.application_error"),
    }
}

/// The turn view the desktop projection really returns for this session.
async fn latest_turn_view(
    runtime: &crate::RuntimeState,
    session: SessionId,
) -> aura_projection_contract::AgentTurnView {
    let history = crate::r03_tests::success(
        runtime,
        "agent.history",
        json!(AgentHistoryRequest {
            session_id: session,
            before_turn_task_id: None,
            within_turn: None,
            limit: 10
        }),
    )
    .await;
    let turns: Vec<aura_projection_contract::AgentTurnView> =
        serde_json::from_value(history["turns"].clone()).expect("Agent binding history");
    turns.last().expect("Agent binding history turn").clone()
}

/// A subsequent explicit human turn carries both historical pairs and does not replay a write.
#[tokio::test]
async fn a_human_continuation_keeps_the_unknown_call_result_without_replaying_the_write() {
    let run = start_binding_run().await;
    let request = AgentRunRequest { session_id: run.session, agent_run_id: run.run };
    let done = waiting::wait(&run.runtime, &request, |view| view.turn.state.terminal()).await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }));
    let queued = crate::r03_tests::success(
        &run.runtime,
        "agent.submit",
        json!(AgentSubmitRequest {
            session_id: run.session,
            text: "Continue from the recorded results.".into(),
            destination: AgentInputLane::NextTurn,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    assert!(!queued.is_null());
    let next_run = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let view = latest_turn_view(&run.runtime, run.session).await;
            if view.turn.turn_task_id != run.turn_task_id {
                break view.turn.agent_run_id;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("new human turn");
    let next = waiting::wait(
        &run.runtime,
        &AgentRunRequest { session_id: run.session, agent_run_id: next_run },
        |view| view.turn.state.terminal(),
    )
    .await;
    assert!(matches!(next.turn.state, TurnTaskState::Completed { .. }));
    assert_eq!(binding_facts(&run.runtime).execution_attempts, 1);
    let requests = run.provider.requests.lock().expect("requests");
    let input =
        requests.last().expect("continuation request")["input"].as_array().expect("history");
    for call_id in [RESPONSES_WRITE_CALL_ID, "call_illegal"] {
        assert!(
            input
                .iter()
                .any(|item| item["type"] == "function_call_output" && item["call_id"] == call_id)
        );
    }
    assert_eq!(
        std::fs::read_to_string(run.workspace_root.join("agent-result.txt")).expect("write"),
        RESPONSES_WRITE_TOKEN
    );
}

#[tokio::test]
async fn an_unknown_first_call_continues_to_one_known_write() {
    let run = start_binding_run_with(Scenario::ResponsesUnknownThenWrite).await;
    let done = waiting::wait(
        &run.runtime,
        &AgentRunRequest { session_id: run.session, agent_run_id: run.run },
        |view| view.turn.state.terminal(),
    )
    .await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }), "{:?}", done.turn.state);
    assert_eq!(done.turn.turn_task_id, run.turn_task_id);
    let facts = binding_facts(&run.runtime);
    assert_eq!(facts.execution_attempts, 1);
    assert!(facts.model_failure_payloads.is_empty());
    assert!(facts.application_error_payloads.is_empty());
    assert_eq!(
        std::fs::read_to_string(run.workspace_root.join("agent-result.txt")).expect("write"),
        RESPONSES_WRITE_TOKEN
    );
    let requests = run.provider.requests.lock().expect("requests");
    let input = requests[1]["input"].as_array().expect("second request history");
    let result = input
        .iter()
        .find(|item| item["type"] == "function_call_output" && item["call_id"] == "call_illegal")
        .expect("refusal precedes the known write");
    assert!(result["output"].as_str().expect("result").contains("tool_not_found"));
}
