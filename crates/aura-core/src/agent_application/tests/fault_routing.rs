//! S04-D: Provider, tool and local-record faults, routed separately through the real entries.
//!
//! WHAT IS REAL HERE
//! Only the external Provider answer is substituted (the existing loopback fixture). The Agent
//! loop, the admission of tool calls, the tool broker, the permission/mode decision, the V2
//! journal, the projection and the run supervisor are the production ones, reached through the
//! production `agent.*` entries. Every case therefore keeps apart the facts that used to be one
//! error string: which class the fault is, which stage it happened in, what the run recorded, and
//! whether anything was dispatched.
//!
//! CLASSIFICATION IS BY TYPE
//! No case asserts a stage by matching message text, and no case depends on the product deriving a
//! stage from text: the Provider classes carry the transport's own failure value, the binding class
//! carries the transport's `ProviderBindingViolationDiagnostic`, the tool-Unknown class carries its
//! batch and Attempt identities, and a refusal before dispatch carries the contract's
//! `ToolNotExecutedReasonV1`.
//!
//! READING THE STORE
//! The durable facts below are decoded from the same journal rows the projection reads, and they
//! are filtered by the Tool's own `tool_id`, because the discovery Tools (`tools.search`,
//! `tools.load`) are real tool calls too: a global count would silently mix them in.
use super::*;
use aura_core_broker::agent_store;
use provider_fixture::{Provider, Scenario};
use std::time::{Duration, Instant};

/// Endpoint budgets for the fault cases: a real first-event timeout has to happen inside a test,
/// so it is configured on the endpoint rather than slept for. No product default is touched.
const FAULT_TIMEOUTS: ModelEndpointTimeouts = ModelEndpointTimeouts {
    connect_ms: 2_000,
    first_event_ms: 1_200,
    stream_idle_ms: 1_200,
    total_ms: Some(8_000),
};

const DEFAULT_TIMEOUTS: ModelEndpointTimeouts = ModelEndpointTimeouts {
    connect_ms: 5_000,
    first_event_ms: 10_000,
    stream_idle_ms: 10_000,
    total_ms: Some(30_000),
};

/// One session on a real runtime, with its own real Provider fixture.
struct Session {
    /// The fixture host owns the temporary root, so it has to outlive the run.
    _host: crate::r03_tests::Fixture,
    runtime: crate::RuntimeState,
    provider: Provider,
    model: ModelProfileId,
    session: SessionId,
    mode: PermissionMode,
    workspace_root: std::path::PathBuf,
}

impl Session {
    async fn new(
        scenario: Scenario,
        mode: PermissionMode,
        api_format: ModelApiFormat,
        timeouts: ModelEndpointTimeouts,
        root: &str,
    ) -> Self {
        let host = crate::r03_tests::Fixture::new();
        let runtime = host.start().await;
        let provider = Provider::scenario(scenario).await;
        let model = profile_for_with(&runtime, &provider.url, api_format, timeouts).await;
        let workspace_root = host.root.path().join(root);
        std::fs::create_dir_all(&workspace_root).expect("S04-D workspace");
        let created = crate::r03_tests::success(
            &runtime,
            "workspace.create",
            json!({"selected_root":workspace_root,"display_name":"S04-D faults"}),
        )
        .await;
        let workspace: aura_workspace_domain::WorkspaceSummary =
            serde_json::from_value(created["workspace"].clone()).expect("workspace summary");
        let session = create_session(&runtime, workspace.workspace_id, mode).await;
        Self { _host: host, runtime, provider, model, session, mode, workspace_root }
    }

    fn services(&self) -> &crate::RuntimeServices {
        match &self.runtime.startup_mode {
            crate::StartupMode::Ready(services) => services,
            _ => panic!("Core must be ready"),
        }
    }

    fn agent(&self) -> std::sync::Arc<crate::agent_application::AgentApplication> {
        self.services().agent_required()
    }

    async fn start(&self, mode: AgentTaskMode) -> AgentRunProjection {
        waiting::start(&self.runtime, self.session, self.model, mode).await
    }

    /// A second real session on the same runtime: it exists to be *unaffected*.
    async fn second_session(
        &self,
        scenario: Scenario,
        root: &str,
    ) -> (Provider, SessionId, ModelProfileId) {
        let provider = Provider::scenario(scenario).await;
        let profile = profile_after_existing(&self.runtime, &provider.url).await;
        let workspace = self.workspace_root.parent().expect("fixture root").join(root);
        std::fs::create_dir_all(&workspace).expect("second workspace");
        let created = crate::r03_tests::success(
            &self.runtime,
            "workspace.create",
            json!({"selected_root":workspace,"display_name":"S04-D second"}),
        )
        .await;
        let summary: aura_workspace_domain::WorkspaceSummary =
            serde_json::from_value(created["workspace"].clone()).expect("workspace summary");
        let session = create_session(&self.runtime, summary.workspace_id, self.mode).await;
        // The queue starts from the session binding, so a later continuation inherits the model.
        crate::r03_tests::success(
            &self.runtime,
            "models.session.set_binding",
            json!({"expected_revision":"0","session_id":session,"model_profile_id":profile}),
        )
        .await;
        (provider, session, profile)
    }

    async fn start_with(
        &self,
        session: SessionId,
        model: ModelProfileId,
        mode: AgentTaskMode,
    ) -> AgentRunProjection {
        waiting::start(&self.runtime, session, model, mode).await
    }
}

/// Wait for a terminal turn through the production status entry.
async fn wait_terminal(
    runtime: &crate::RuntimeState,
    request: &AgentRunRequest,
    budget: Duration,
) -> AgentRunProjection {
    let deadline = Instant::now() + budget;
    loop {
        let projection: AgentRunProjection = serde_json::from_value(
            crate::r03_tests::success(runtime, "agent.status", json!(request)).await,
        )
        .expect("Agent status");
        if projection.turn.state.terminal() {
            return projection;
        }
        assert!(
            Instant::now() < deadline,
            "the run must reach a terminal state: {:?}",
            projection.turn.state
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Waits until the turn is terminal **or** waiting for the user's answer.
///
/// A mode that does not authorize an operation by default now produces an approval request
/// rather than a refusal, so a test asserting "nothing happened yet" has to stop on either
/// outcome. Waiting only for a terminal state would hang on the honest one.
async fn wait_terminal_or_approval(
    runtime: &crate::RuntimeState,
    request: &AgentRunRequest,
    budget: Duration,
) -> AgentRunProjection {
    let deadline = Instant::now() + budget;
    loop {
        let projection: AgentRunProjection = serde_json::from_value(
            crate::r03_tests::success(runtime, "agent.status", json!(request)).await,
        )
        .expect("Agent status");
        if projection.turn.state.terminal()
            || matches!(projection.turn.state, TurnTaskState::AwaitingApproval)
        {
            return projection;
        }
        assert!(
            Instant::now() < deadline,
            "the run must reach a terminal state or ask for approval: {:?}",
            projection.turn.state
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// A blocking wait, for the cases driven from a `#[test]` with a multi-thread runtime.
fn wait_until(deadline: Duration, predicate: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    predicate()
}

/// The same wait for a `#[tokio::test]`: a blocking sleep would park the runtime the run needs.
async fn wait_until_async(deadline: Duration, predicate: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if predicate() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    predicate()
}

/// Run `work` against the journal connection, holding the one Store lock for its duration.
///
/// The guard's own type is not nameable from here, and the point of the helper is only that every
/// durable read in this module goes through the same authority the projection uses.
macro_rules! with_db {
    ($runtime:expr, |$db:ident| $body:expr) => {{
        let crate::StartupMode::Ready(services) = &$runtime.startup_mode else {
            panic!("Core ready")
        };
        let store = services.broker.lock().expect("S04-D journal");
        let $db = store.database.connection();
        $body
    }};
}

/// Every admitted Tool call's own decoded record.
fn tool_calls(runtime: &crate::RuntimeState) -> Vec<Value> {
    let rows: Vec<String> = with_db!(runtime, |db| {
        let mut q = db.prepare("SELECT payload FROM tool_calls ORDER BY rowid").expect("query");
        q.query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("query")
    });
    rows.iter().map(|text| serde_json::from_str(text).expect("tool call record")).collect()
}

/// The calls whose own record carries `needle`.
///
/// A `ToolId` is a `tol_` ULID and does not spell the Tool's canonical name, so the call is
/// identified by an argument the model was told to send - a property of that request, never of
/// whichever row happened to come first.
fn calls_containing<'a>(calls: &'a [Value], needle: &str) -> Vec<&'a Value> {
    calls.iter().filter(|call| call.to_string().contains(needle)).collect()
}

fn tool_batches(runtime: &crate::RuntimeState) -> i64 {
    with_db!(runtime, |db| {
        db.query_row("SELECT COUNT(*) FROM tool_batches", [], |row| row.get(0)).expect("count")
    })
}

/// The one `agent.application_error` a run recorded, decoded.
fn recorded_error(runtime: &crate::RuntimeState, run: AgentRunId) -> Value {
    let rows: Vec<String> = with_db!(runtime, |db| {
        let mut q = db
            .prepare("SELECT payload FROM events WHERE kind='agent.application_error' AND owner=?1")
            .expect("query");
        q.query_map([run.to_string()], |row| row.get::<_, String>(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("query")
    });
    assert_eq!(rows.len(), 1, "exactly one application error for this run: {rows:?}");
    serde_json::from_str(&rows[0]).expect("application error payload")
}

fn application_error_count(runtime: &crate::RuntimeState) -> usize {
    with_db!(runtime, |db| {
        let mut q = db
            .prepare("SELECT payload FROM events WHERE kind='agent.application_error'")
            .expect("query");
        q.query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("query")
            .len()
    })
}

/// `<ordinal>:<state>` for every persisted model step of one run.
fn model_steps(runtime: &crate::RuntimeState, run: AgentRunId) -> Vec<String> {
    let steps = with_db!(runtime, |db| {
        aura_persistence_v2::agent::model_context::steps(db, run).expect("model steps")
    });
    steps
        .into_iter()
        .map(|step| {
            let state = match step.state {
                ModelStepState::Preparing => "preparing",
                ModelStepState::Running { .. } => "running",
                ModelStepState::Completed { .. } => "completed",
                ModelStepState::Failed { .. } => "failed",
                ModelStepState::Cancelled => "cancelled",
            };
            format!("{}:{state}", step.ordinal)
        })
        .collect()
}

/// The execution attempt one admitted call really claimed, decoded.
///
/// The attempt is bound to the call's Operation, which the store keeps in its own column, so the
/// lookup is the real binding and not a payload guess.
fn execution_attempt(runtime: &crate::RuntimeState, tool_call_id: &str) -> Option<Value> {
    let text: Option<String> = with_db!(runtime, |db| {
        db.query_row(
            "SELECT attempts.payload FROM execution_attempts AS attempts \
             JOIN tool_calls AS calls ON calls.operation_id = attempts.operation_id \
             WHERE calls.id=?1",
            [tool_call_id],
            |row| row.get(0),
        )
        .ok()
    });
    text.map(|text| serde_json::from_str(&text).expect("execution attempt record"))
}

fn run_id(projection: &AgentRunProjection) -> AgentRunId {
    projection.run.agent_run_id
}

fn request(projection: &AgentRunProjection) -> AgentRunRequest {
    AgentRunRequest { session_id: projection.turn.session_id, agent_run_id: run_id(projection) }
}

/// D1 - a **definite** Provider failure is a Provider fault at the Provider stage, and nothing is
/// admitted or dispatched for it.
#[tokio::test]
async fn d1_a_definite_provider_failure_is_routed_as_a_provider_fault_and_dispatches_nothing() {
    let run = Session::new(
        Scenario::ProviderUnavailable,
        PermissionMode::ReadOnly,
        ModelApiFormat::OpenAiChatCompletions,
        DEFAULT_TIMEOUTS,
        "s04-d1",
    )
    .await;
    let started = run.start(AgentTaskMode::Normal).await;
    let stopped = wait_terminal(&run.runtime, &request(&started), Duration::from_secs(45)).await;
    let payload = recorded_error(&run.runtime, run_id(&started));
    let steps = model_steps(&run.runtime, run_id(&started));
    println!(
        "D1 turn={:?} error={payload} steps={steps:?} batches={} calls={} requests={}",
        stopped.turn.state,
        tool_batches(&run.runtime),
        tool_calls(&run.runtime).len(),
        run.provider.requests.lock().expect("requests").len()
    );
    // The failure is the Provider's own answer, and it is reported as a Provider fault.
    assert!(
        payload["error"].as_str().expect("stored").starts_with("agent.provider_failed:"),
        "{payload}"
    );
    assert!(
        matches!(stopped.turn.state, TurnTaskState::RecoveryRequired { .. }),
        "{:?}",
        stopped.turn.state
    );
    // Nothing was admitted, nothing was dispatched, no result was invented.
    assert_eq!(tool_batches(&run.runtime), 0, "a refused Provider answer admits no tool batch");
    assert_eq!(tool_calls(&run.runtime).len(), 0);
    // The step failed, and the store says so for this run only.
    assert_eq!(steps, vec!["1:failed".to_owned()]);
    assert!(
        !run.provider.requests.lock().expect("requests").is_empty(),
        "the request really reached the endpoint"
    );
    // The shared dependency was never blamed: this is not a store fault.
    assert!(
        run.agent().store_health(agent_store::StoreDependency::Journal).available,
        "a Provider fault is not a store fact"
    );
}

/// D2 - a Provider answer that arrives **after** the caller already failed the step cannot revive
/// it, and the failed step is not called again.
///
/// The endpoint really parks every attempt and really answers after the release. The three
/// requests the transport makes are its own retry policy for one failed step, which is why the
/// assertion is on the persisted step count and not on the request count: a retry of an *attempt*
/// is not a re-call of a finished model step.
#[test]
fn d2_a_late_provider_answer_cannot_revive_a_step_the_caller_already_failed() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    let run = runtime.block_on(Session::new(
        Scenario::LateAnswer,
        PermissionMode::ReadOnly,
        ModelApiFormat::OpenAiChatCompletions,
        FAULT_TIMEOUTS,
        "s04-d2",
    ));
    let started = runtime.block_on(run.start(AgentTaskMode::Normal));
    let parked = wait_until(Duration::from_secs(20), || {
        !run.provider.requests.lock().expect("requests").is_empty()
    });
    let stopped =
        runtime.block_on(wait_terminal(&run.runtime, &request(&started), Duration::from_secs(45)));
    let before = model_steps(&run.runtime, run_id(&started));
    let payload = recorded_error(&run.runtime, run_id(&started));
    // Now the Provider really answers, late, into sockets nothing is reading.
    run.provider.release_window.notify_waiters();
    runtime.block_on(async { tokio::time::sleep(Duration::from_millis(400)).await });
    let after = model_steps(&run.runtime, run_id(&started));
    let requests = run.provider.requests.lock().expect("requests").len();
    println!(
        "D2 parked={parked} turn={:?} error={payload} steps_before={before:?} \
         steps_after={after:?} transport_requests={requests}",
        stopped.turn.state
    );
    assert!(parked, "the fixture must really have received the request");
    assert!(
        matches!(stopped.turn.state, TurnTaskState::RecoveryRequired { .. }),
        "{:?}",
        stopped.turn.state
    );
    assert!(
        payload["error"].as_str().expect("stored").starts_with("agent.provider_failed:"),
        "a real first-event timeout is a Provider failure: {payload}"
    );
    assert_eq!(after, before, "the late answer revived or changed no step");
    assert_eq!(after, vec!["1:failed".to_owned()], "the step failed exactly once");
    assert!(!after.iter().any(|step| step.ends_with("completed")));
    assert_eq!(tool_calls(&run.runtime).len(), 0);
    assert!(requests >= 1, "the request really reached the endpoint {requests} time(s)");
}

/// D3 - a broken connection is a transport fault, not a Provider error document.
#[tokio::test]
async fn d3_a_broken_connection_is_routed_as_a_transport_fault() {
    let run = Session::new(
        Scenario::BrokenConnection,
        PermissionMode::ReadOnly,
        ModelApiFormat::OpenAiChatCompletions,
        DEFAULT_TIMEOUTS,
        "s04-d3",
    )
    .await;
    let started = run.start(AgentTaskMode::Normal).await;
    let stopped = wait_terminal(&run.runtime, &request(&started), Duration::from_secs(45)).await;
    let payload = recorded_error(&run.runtime, run_id(&started));
    let steps = model_steps(&run.runtime, run_id(&started));
    println!("D3 turn={:?} error={payload} steps={steps:?}", stopped.turn.state);
    assert!(
        matches!(stopped.turn.state, TurnTaskState::RecoveryRequired { .. }),
        "{:?}",
        stopped.turn.state
    );
    assert_eq!(steps, vec!["1:failed".to_owned()]);
    assert_eq!(tool_batches(&run.runtime), 0);
    assert_eq!(tool_calls(&run.runtime).len(), 0);
    // The class is the transport's own: the fixture never sent a Provider error document, so a
    // structured binding diagnostic must not appear.
    let stored = payload["error"].as_str().expect("stored");
    assert!(stored.starts_with("agent.provider_failed:"), "{stored}");
    assert_eq!(payload.get("diagnostic"), None, "a truncated stream is not a refusal: {payload}");
}

/// D4 - an unknown wire name is a call-local refusal with a paired result.
#[tokio::test]
async fn d4_an_unknown_wire_name_does_not_become_an_application_failure() {
    let run = Session::new(
        Scenario::ResponsesWriteThenIllegalToolName,
        PermissionMode::WorkspaceReadWrite,
        ModelApiFormat::OpenAiResponses,
        DEFAULT_TIMEOUTS,
        "s10-unknown-tool",
    )
    .await;
    let started = run.start(AgentTaskMode::Normal).await;
    let done = wait_terminal(&run.runtime, &request(&started), Duration::from_secs(60)).await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }), "{:?}", done.turn.state);
    let calls = tool_calls(&run.runtime);
    assert_eq!(calls.len(), 2);
    let unknown =
        calls.iter().find(|call| call["call"]["definition_ref"].is_null()).expect("unbound call");
    assert!(unknown["attempt_id"].is_null());
    assert_eq!(unknown["result"]["outcome"]["reason"], "tool_not_found");
    assert_eq!(
        std::fs::read_to_string(run.workspace_root.join("agent-result.txt")).expect("write"),
        provider_fixture::RESPONSES_WRITE_TOKEN
    );
}

/// D5 - a tool the session may not run without an answer is not executed, leaves no Attempt,
/// and produces no side effect.
///
/// The migrated half: under the confirmed semantics a mode is a **default**, so ReadOnly
/// `fs.write` is no longer `permission_denied` — it is an approval request and the run waits.
/// The invariant this test exists for is unchanged and is asserted either way: the call never
/// acquires an Attempt and the target file is never created. What moved is which of the two
/// honest outcomes the user gets — a request they can answer instead of a flat refusal.
#[tokio::test]
async fn d5_a_tool_refused_before_dispatch_is_not_executed_and_leaves_no_attempt() {
    let run = Session::new(
        Scenario::Write,
        PermissionMode::ReadOnly,
        ModelApiFormat::OpenAiChatCompletions,
        DEFAULT_TIMEOUTS,
        "s04-d5",
    )
    .await;
    let started = run.start(AgentTaskMode::Normal).await;
    // The run waits for the user's answer rather than finishing, so the wait is bounded and a
    // still-pending run is the expected outcome here.
    let stopped =
        wait_terminal_or_approval(&run.runtime, &request(&started), Duration::from_secs(60)).await;
    let all = tool_calls(&run.runtime);
    let writes = calls_containing(&all, "agent-result.txt");
    println!(
        "D5 turn={:?} calls={} writes={} write={}",
        stopped.turn.state,
        all.len(),
        writes.len(),
        writes.first().map(ToString::to_string).unwrap_or_default()
    );
    assert_eq!(writes.len(), 1, "the call was admitted");
    let write = writes[0];
    assert!(write["attempt_id"].is_null(), "and it never acquired an Attempt: {write}");
    assert!(
        !run.workspace_root.join("agent-result.txt").exists(),
        "a call that waits for its answer has no side effect"
    );
    assert!(
        matches!(stopped.turn.state, TurnTaskState::AwaitingApproval),
        "ReadOnly asks for a precise approval instead of refusing outright: {:?}",
        stopped.turn.state
    );
}

/// D6 - a tool that really ran and really failed is a definite failure, and it is neither a
/// refusal nor an unknown outcome.
#[tokio::test]
async fn d6_a_tool_that_really_ran_and_failed_is_a_definite_failure_not_an_unknown() {
    let run = Session::new(
        Scenario::ReadMissing,
        PermissionMode::ReadOnly,
        ModelApiFormat::OpenAiChatCompletions,
        DEFAULT_TIMEOUTS,
        "s04-d6",
    )
    .await;
    let started = run.start(AgentTaskMode::Normal).await;
    let stopped = wait_terminal(&run.runtime, &request(&started), Duration::from_secs(60)).await;
    let all = tool_calls(&run.runtime);
    let reads = calls_containing(&all, "definitely-missing-agent-file.txt");
    assert_eq!(reads.len(), 1, "the model's read was admitted: {all:?}");
    let read = reads[0];
    let attempt =
        read["call"]["tool_call_id"].as_str().and_then(|id| execution_attempt(&run.runtime, id));
    let steps = model_steps(&run.runtime, run_id(&started));
    println!("D6 turn={:?} read={read} attempt={attempt:?} steps={steps:?}", stopped.turn.state);
    assert!(
        matches!(stopped.turn.state, TurnTaskState::Completed { .. }),
        "{:?}",
        stopped.turn.state
    );
    assert!(!read["attempt_id"].is_null(), "the call really reached a backend");
    // The backend really ran and really answered: this is a definite answer about the call, not a
    // refusal before dispatch and not an unknown outcome.
    let class = read["result"]["outcome"]["type"].as_str().expect("outcome class");
    assert!(
        matches!(class, "failed" | "operation_rejected"),
        "a real, definite answer is the definite class: {read}"
    );
    assert_ne!(class, "not_executed", "the call did reach a backend: {read}");
    assert_ne!(class, "indeterminate", "the outcome is established: {read}");
    assert_eq!(
        attempt.expect("an Attempt")["state"]["state"],
        "terminal",
        "the attempt is closed, so this is not an unknown"
    );
    assert_eq!(
        application_error_count(&run.runtime),
        0,
        "a failed tool is a result, not a run fault"
    );
}

/// D7 - a call that was really dispatched and has no terminal receipt routes as Unknown, keeps its
/// Attempt identities, and is never dispatched again.
///
/// WHAT IS PRODUCTION HERE: the class, its structured evidence, the recording entry
/// (`persist_drive_failure`), the store write, the projection and the fact that no dispatch is
/// attempted for it. WHAT IS CONSTRUCTED: the `ToolOutcomeUnknown` value itself. Producing it from
/// the drive loop needs a real interruption between a tool call's reservation and its terminal
/// receipt - a window with no hold point inside `execute_batch` - so the value is built with the
/// same type and the same inputs the loop builds it from, and the end-to-end interruption is
/// exercised by the S04-E IPC gate (a Core that really dies mid-tool). This case therefore pins
/// the routing; it does not claim the interruption.
#[tokio::test]
async fn d7_a_dispatched_call_without_a_terminal_receipt_routes_as_unknown_and_is_never_redispatched()
 {
    let run = Session::new(
        Scenario::Text,
        PermissionMode::ReadOnly,
        ModelApiFormat::OpenAiChatCompletions,
        DEFAULT_TIMEOUTS,
        "s04-d7",
    )
    .await;
    let started = run.start(AgentTaskMode::Normal).await;
    let id = run_id(&started);
    let request = request(&started);
    let agent = run.agent();
    let settled = wait_until_async(Duration::from_secs(20), || {
        agent.supervisor().ledger().iter().any(|fact| fact.run == id)
    })
    .await;
    assert!(settled, "the first run must settle");
    let generation = agent.supervisor().generation_of(id).unwrap_or(0);
    let before_calls = tool_calls(&run.runtime).len();
    let before_requests = run.provider.requests.lock().expect("requests").len();
    let before: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(&run.runtime, "agent.status", json!(request)).await,
    )
    .expect("status");
    // The same type and the same inputs the drive loop builds this class from.
    let unknown = crate::agent_application::fault::ToolOutcomeUnknown {
        run: id,
        batch: ToolBatchId::new(),
        attempts: vec![aura_execution_contract::AttemptRef {
            operation_id: SecurityOperationId::new(),
            ordinal: std::num::NonZeroU32::MIN,
        }],
    };
    agent.persist_drive_failure(id, generation, &anyhow::Error::new(unknown), false).await;
    let payload = recorded_error(&run.runtime, id);
    let after: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(&run.runtime, "agent.status", json!(request)).await,
    )
    .expect("status");
    println!("D7 payload={payload} before={:?} after={:?}", before.turn.state, after.turn.state);
    // The class is the tool Unknown, with its own evidence - not a Provider fault, not a store
    // fault, and not a line that had to be parsed to find out which.
    assert_eq!(payload["diagnostic"]["code"], "agent.tool_outcome_unknown");
    assert_eq!(payload["diagnostic"]["stage"], "tools");
    assert_eq!(payload["diagnostic"]["replay"], "never");
    assert_eq!(payload["diagnostic"]["attempt_count"], 1);
    assert!(
        payload["diagnostic"]["attempts"][0]["operation_id"].as_str().is_some(),
        "the real Attempt identity is retained: {payload}"
    );
    // Recording the fact dispatched nothing and re-called no model step.
    assert_eq!(tool_calls(&run.runtime).len(), before_calls);
    assert_eq!(run.provider.requests.lock().expect("requests").len(), before_requests);
    // The projection is what the recording left, not something the recorder invented.
    assert_eq!(after.turn.revision, before.turn.revision, "no projection was fabricated");
    assert!(after.turn.state.terminal(), "the run stays terminal: {:?}", after.turn.state);
}

/// D7b - the recorded Unknown is queryable from the store exactly once.
#[tokio::test]
async fn d7b_the_unknown_fact_is_queryable_and_committed_exactly_once() {
    let run = Session::new(
        Scenario::Text,
        PermissionMode::ReadOnly,
        ModelApiFormat::OpenAiChatCompletions,
        DEFAULT_TIMEOUTS,
        "s04-d7b",
    )
    .await;
    let started = run.start(AgentTaskMode::Normal).await;
    let id = run_id(&started);
    let agent = run.agent();
    assert!(
        wait_until_async(Duration::from_secs(20), || {
            agent.supervisor().ledger().iter().any(|fact| fact.run == id)
        })
        .await
    );
    let generation = agent.supervisor().generation_of(id).unwrap_or(0);
    let unknown = crate::agent_application::fault::ToolOutcomeUnknown {
        run: id,
        batch: ToolBatchId::new(),
        attempts: vec![aura_execution_contract::AttemptRef {
            operation_id: SecurityOperationId::new(),
            ordinal: std::num::NonZeroU32::MIN,
        }],
    };
    agent.persist_drive_failure(id, generation, &anyhow::Error::new(unknown), false).await;
    // The same fact is readable again, and it is the only one.
    let first = recorded_error(&run.runtime, id);
    let second = recorded_error(&run.runtime, id);
    assert_eq!(first, second);
    assert_eq!(application_error_count(&run.runtime), 1);
    assert!(
        agent.store().receipts().iter().any(|receipt| receipt.operation
            == "agent.drive_failure_record"
            && receipt.state == agent_store::ReceiptState::Settled { committed: true, code: None }),
        "the record's own receipt reports the real terminal fact"
    );
}

/// D8 - a result-record failure stays local to its run and the other session keeps working.
#[test]
fn d8_a_result_record_failure_stays_local_and_the_other_session_keeps_working() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    let run = runtime.block_on(Session::new(
        Scenario::ProviderUnavailable,
        PermissionMode::ReadOnly,
        ModelApiFormat::OpenAiChatCompletions,
        DEFAULT_TIMEOUTS,
        "s04-d8",
    ));
    let (provider_b, session_b, model_b) =
        runtime.block_on(run.second_session(Scenario::Text, "s04-d8-b"));
    let started_b = runtime.block_on(run.start_with(session_b, model_b, AgentTaskMode::Normal));
    let done_b = runtime.block_on(wait_terminal(
        &run.runtime,
        &request(&started_b),
        Duration::from_secs(45),
    ));
    assert!(matches!(done_b.turn.state, TurnTaskState::Completed { .. }));
    // A's own record write is parked inside the production call, so its caller ends bounded and
    // the run's unrecorded fact stays local.
    run.runtime.sync.set_budget(Duration::from_millis(400));
    let hold = run.agent().store().arm_hold("agent.drive_failure_record", 4_000);
    let started_a = runtime.block_on(run.start(AgentTaskMode::Normal));
    let id_a = run_id(&started_a);
    let reached = wait_until(Duration::from_secs(25), || hold.was_reached());
    let ended = wait_until(Duration::from_secs(20), || !run.agent().supervisor().is_active(id_a));
    let diagnostics = run.agent().store().diagnostics().of(id_a);
    let b_after = runtime.block_on(wait_terminal(
        &run.runtime,
        &request(&started_b),
        Duration::from_secs(20),
    ));
    println!(
        "D8 hold_reached={reached} run_ended={ended} diagnostics={diagnostics:?} \
         b_turn={:?} b_requests={}",
        b_after.turn.state,
        provider_b.requests.lock().expect("requests").len()
    );
    assert!(reached, "the run's own record write must really reach the production call");
    assert!(ended, "a failing record must not block the run's resource reclamation");
    assert!(
        diagnostics.iter().any(|entry| entry.stage == "recording"),
        "the unrecorded fact stays visible on that run: {diagnostics:?}"
    );
    assert!(
        matches!(b_after.turn.state, TurnTaskState::Completed { .. }),
        "the other session is unaffected: {:?}",
        b_after.turn.state
    );
    assert!(
        run.agent().store_health(agent_store::StoreDependency::Journal).available,
        "the shared Store is still available"
    );
    // The parked record is the other real path: it commits exactly once, late, with its value.
    hold.release_all();
    let settled =
        wait_until(Duration::from_secs(15), || application_error_count(&run.runtime) == 1);
    assert!(settled, "the parked record commits once the release arrives");
    assert_eq!(application_error_count(&run.runtime), 1, "exactly once");
}

/// D9 - A's fault does not mark B failed or cancelled, and B really executes.
#[test]
fn d9_a_provider_fault_does_not_mark_the_other_session_failed_or_cancelled() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    let run = runtime.block_on(Session::new(
        Scenario::ProviderUnavailable,
        PermissionMode::ReadOnly,
        ModelApiFormat::OpenAiChatCompletions,
        DEFAULT_TIMEOUTS,
        "s04-d9",
    ));
    let (provider_b, session_b, model_b) =
        runtime.block_on(run.second_session(Scenario::Text, "s04-d9-b"));
    // Both sessions really run at the same time.
    let started_b = runtime.block_on(run.start_with(session_b, model_b, AgentTaskMode::Normal));
    let started_a = runtime.block_on(run.start(AgentTaskMode::Normal));
    let stopped_a = runtime.block_on(wait_terminal(
        &run.runtime,
        &request(&started_a),
        Duration::from_secs(45),
    ));
    let done_b = runtime.block_on(wait_terminal(
        &run.runtime,
        &request(&started_b),
        Duration::from_secs(45),
    ));
    let ledger = run.agent().supervisor().ledger();
    let fact_a = ledger.iter().find(|fact| fact.run == run_id(&started_a)).cloned();
    let fact_b = ledger.iter().find(|fact| fact.run == run_id(&started_b)).cloned();
    println!(
        "D9 a={:?} b={:?} fact_a={fact_a:?} fact_b={fact_b:?} b_requests={} \
         run_a={} session_a={} turn_a={} run_b={} session_b={} turn_b={}",
        stopped_a.turn.state,
        done_b.turn.state,
        provider_b.requests.lock().expect("requests").len(),
        run_id(&started_a),
        started_a.turn.session_id,
        started_a.turn.turn_task_id,
        run_id(&started_b),
        started_b.turn.session_id,
        started_b.turn.turn_task_id,
    );
    assert!(matches!(stopped_a.turn.state, TurnTaskState::RecoveryRequired { .. }));
    // B not only "can be read": it really asked its own Provider and really completed.
    assert!(
        !provider_b.requests.lock().expect("requests").is_empty(),
        "B's own Provider request really happened"
    );
    assert!(
        matches!(done_b.turn.state, TurnTaskState::Completed { .. }),
        "A's fault must not mark B failed or cancelled: {:?}",
        done_b.turn.state
    );
    assert_eq!(fact_a.map(|fact| fact.end), Some("failed"));
    assert_eq!(fact_b.map(|fact| fact.end), Some("completed"));
    assert!(run.agent().supervisor().active_runs().is_empty(), "both runs released their entries");
}

/// D10 - after the fault clears, a new legitimate turn consumes only real committed boundaries:
/// the failed model step is not re-called and nothing is replayed.
#[tokio::test]
async fn d10_recovery_after_a_fault_consumes_only_committed_boundaries() {
    let run = Session::new(
        Scenario::ProviderUnavailable,
        PermissionMode::ReadOnly,
        ModelApiFormat::OpenAiChatCompletions,
        DEFAULT_TIMEOUTS,
        "s04-d10",
    )
    .await;
    let failed = run.start(AgentTaskMode::Normal).await;
    let failed_id = run_id(&failed);
    let stopped = wait_terminal(&run.runtime, &request(&failed), Duration::from_secs(45)).await;
    assert!(matches!(stopped.turn.state, TurnTaskState::RecoveryRequired { .. }));
    let after_fault = model_steps(&run.runtime, failed_id);
    // A genuinely different Provider: the fault is gone, so this turn can really complete.
    let provider_next = Provider::scenario(Scenario::Text).await;
    let model_next = profile_after_existing(&run.runtime, &provider_next.url).await;
    let next = run.start_with(run.session, model_next, AgentTaskMode::Normal).await;
    let next_id = run_id(&next);
    assert_ne!(next_id, failed_id, "recovery is a new run, never a replay of the failed one");
    let done = wait_terminal(&run.runtime, &request(&next), Duration::from_secs(45)).await;
    let after_recovery = model_steps(&run.runtime, failed_id);
    let next_steps = model_steps(&run.runtime, next_id);
    println!(
        "D10 turn={:?} next={:?} failed_steps={after_recovery:?} next_steps={next_steps:?} \
         next_requests={}",
        stopped.turn.state,
        done.turn.state,
        provider_next.requests.lock().expect("requests").len()
    );
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }), "{:?}", done.turn.state);
    assert_eq!(after_recovery, after_fault, "the failed step's own record is untouched");
    assert_eq!(after_recovery, vec!["1:failed".to_owned()]);
    assert!(
        !provider_next.requests.lock().expect("requests").is_empty(),
        "the recovered turn really asked its own Provider"
    );
    // The recovered turn has one committed step, and nothing was dispatched on the run's behalf.
    assert_eq!(next_steps, vec!["1:completed".to_owned()]);
    assert_eq!(tool_calls(&run.runtime).len(), 0);
    assert!(
        run.agent().store_health(agent_store::StoreDependency::Journal).available,
        "no permanent fault marker is left behind"
    );
}

/// D11 - Normal, Plan and Goal each keep their own semantics across a fault and a recovery.
#[tokio::test]
async fn d11_normal_plan_and_goal_each_recover_with_their_own_semantics() {
    for (mode, scenario, requests) in [
        (AgentTaskMode::Normal, Scenario::Text, 1usize),
        (AgentTaskMode::Plan, Scenario::Plan, 1),
        (AgentTaskMode::Goal, Scenario::Goal, 2),
    ] {
        let run = Session::new(
            Scenario::ProviderUnavailable,
            PermissionMode::ReadOnly,
            ModelApiFormat::OpenAiChatCompletions,
            DEFAULT_TIMEOUTS,
            &format!("s04-d11-{mode:?}"),
        )
        .await;
        // The fault turn: the Provider really refuses, in the mode under test.
        let failed = run.start(mode).await;
        let stopped = wait_terminal(&run.runtime, &request(&failed), Duration::from_secs(45)).await;
        assert!(
            matches!(stopped.turn.state, TurnTaskState::RecoveryRequired { .. }),
            "{mode:?}: {:?}",
            stopped.turn.state
        );
        assert_eq!(tool_batches(&run.runtime), 0, "{mode:?}");
        // The same session accepts a new legitimate turn in the same mode, and the mode's own
        // committed semantics still hold.
        let provider_next = Provider::scenario(scenario).await;
        let model_next = profile_after_existing(&run.runtime, &provider_next.url).await;
        let next = run.start_with(run.session, model_next, mode).await;
        let done = wait_terminal(&run.runtime, &request(&next), Duration::from_secs(60)).await;
        let calls = provider_next.requests.lock().expect("requests").len();
        println!("D11 mode={mode:?} next={:?} requests={calls}", done.turn.state);
        assert!(
            matches!(done.turn.state, TurnTaskState::Completed { .. }),
            "{mode:?}: {:?}",
            done.turn.state
        );
        assert_eq!(calls, requests, "{mode:?}: the mode's own model step count");
        if mode == AgentTaskMode::Goal {
            let goal = with_db!(run.runtime, |db| {
                aura_persistence_v2::agent::work::goal(db, done.turn.turn_task_id)
                    .expect("the goal of the recovered turn")
            });
            assert!(goal.completed, "Goal keeps its own completion semantics");
            assert_eq!(goal.consumed_turns, 2);
        }
    }
}
