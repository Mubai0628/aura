use super::*;
use aura_core_base::{command_settlement, ipc_session};

fn start_owned_handler(
    runtime: &tokio::runtime::Runtime,
    state: Arc<RuntimeState>,
    call: SessionCall,
) -> ipc_session::IpcSession {
    let mut session = ipc_session::IpcSession::new();
    let _ = session.admit(call.request.clone());
    let ready = session.take_ready();
    assert_eq!(ready.len(), 1, "the real execution lane admitted this request");
    let (ticket, _) = ready.into_iter().next().expect("admitted request").into_parts();
    let handle = runtime.spawn(async move {
        let _ = drive_session(state, call).await;
        ipc_session::HandlerTerminal::Answered(IpcRequestTerminal::Succeeded)
    });
    session.attach(ticket, handle);
    session
}

fn ledger_after_work(
    runtime: &tokio::runtime::Runtime,
    state: &Arc<RuntimeState>,
    id: CommandInvocationId,
) -> Value {
    let _ = wait_until(Duration::from_millis(700), || {
        let answer = runtime.block_on(query_command_outcome(Arc::clone(state), id));
        answer.pointer("/result/value/outcome").and_then(Value::as_str) == Some("confirmed")
    });
    runtime.block_on(query_command_outcome(Arc::clone(state), id))
}

#[test]
fn an_expired_caller_keeps_the_original_receipt_until_the_writer_commits() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    persistence.set_sync_deadline_for_test(Duration::from_millis(300));
    persistence.set_settled_receipt_budget_for_test(Duration::from_millis(150));
    let hold = persistence.arm_writer_hold_once(
        aura_persistence::barrier::Point::WriterExecution,
        Some("sync"),
        3000,
    );
    let title = "Codex successful commit after the receipt window";
    let call = SessionCall::new("session.create", harness.workspace, title);
    let invocation = call.invocation;
    let identity = aura_persistence::PersistenceCoordinator::session_write_identity(invocation);
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call));
    assert!(hold.wait_for_arrivals(1, Duration::from_secs(3)), "real writer reached the hold");
    let response = runtime.block_on(driver).expect("caller response");
    assert_eq!(
        failure_envelope(&response).pointer("/code").and_then(Value::as_str),
        Some("command.outcome_unknown")
    );
    assert!(
        wait_until(Duration::from_secs(2), || state
            .sync
            .counters(SyncSection::SessionCommand)
            .in_flight
            == 0),
        "the synchronous admission section has ended before release"
    );
    assert!(persistence.writer_health().running >= 1, "the real writer still owns the job");
    let pending_before_release = state.settlements.pending();
    assert_eq!(state.settlements.pending(), 1, "the admitted work still owns its result");
    hold.release_all();
    assert!(
        wait_until(Duration::from_secs(3), || persistence
            .job_settlement(&identity)
            .is_some_and(|s| s.succeeded)),
        "the original job committed successfully"
    );
    let projection = serde_json::to_string(
        &persistence.workspace_session_projection().expect("actual projection"),
    )
    .expect("json");
    assert_eq!(occurrences(&projection, title), 1, "one actual session exists");
    let answer = ledger_after_work(&runtime, &state, invocation);
    println!(
        "CODEX_RECEIPT {}",
        json!({"invocation": invocation, "pending_before_release": pending_before_release, "pending_after": state.settlements.pending(), "actual_session_count": 1, "settlement": format!("{:?}", persistence.job_settlement(&identity)), "answer": answer})
    );
    assert_eq!(
        answer.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("confirmed"),
        "a healthy late commit still owes its original result"
    );
}

#[test]
fn disconnect_before_evidence_wait_expiry_keeps_the_completion_owner() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(2500));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    let root = harness._fixture.root.path().join("ws-a");
    let relative = "codex-disconnect-evidence.txt";
    let content = "effect survives its connection\n";
    let before = recorded_completions(&state);
    let call = SessionCall::with_params(
        method::FS_WRITE_FILE,
        json!({"workspace_id":harness.workspace,"session_id":harness.session,"target":{"path":relative},"content":content,"expected":{"expected_kind":"absent"},"encoding":"utf8","line_ending":"lf"}),
    );
    let invocation = call.invocation;
    let hold = persistence.arm_admission_hold_selected(
        Some("sync"),
        &aura_persistence::PersistenceCoordinator::broker_evidence_identity(call.request.id),
        5000,
    );
    let mut session = start_owned_handler(&runtime, Arc::clone(&state), call);
    assert!(
        hold.wait_for_arrivals(1, Duration::from_secs(3)),
        "the actual post-execution evidence is held"
    );
    assert_eq!(std::fs::read(root.join(relative)).expect("handler wrote file"), content.as_bytes());
    assert_eq!(state.sync.counters(SyncSection::Evidence).in_flight, 1);
    let handoffs = runtime.block_on(session.shutdown_within(Duration::from_millis(200)));
    assert!(
        handoffs.is_empty(),
        "async handler was aborted by the actual connection shutdown path"
    );
    hold.release_all();
    assert!(
        wait_until(Duration::from_secs(3), || state.sync.counters(SyncSection::Evidence).in_flight
            == 0),
        "real evidence completed"
    );
    let answer = ledger_after_work(&runtime, &state, invocation);
    let completion_delta = recorded_completions(&state) - before;
    println!(
        "CODEX_DISCONNECT_EVIDENCE {}",
        json!({"invocation":invocation,"pending":state.settlements.pending(),"completion_delta":completion_delta,"answer":answer})
    );
    assert_eq!(
        answer.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("confirmed"),
        "disconnect must not destroy post-execution continuation"
    );
    assert_eq!(completion_delta, 1);
}

#[test]
fn disconnect_before_session_wait_expiry_delivers_the_workers_real_value() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(2500));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    let hold = persistence.arm_writer_hold_once(
        aura_persistence::barrier::Point::WriterExecution,
        Some("sync"),
        5000,
    );
    let title = "Codex connection closes while writer runs";
    let call = SessionCall::new("session.create", harness.workspace, title);
    let invocation = call.invocation;
    let identity = aura_persistence::PersistenceCoordinator::session_write_identity(invocation);
    let mut session = start_owned_handler(&runtime, Arc::clone(&state), call);
    assert!(hold.wait_for_arrivals(1, Duration::from_secs(3)));
    let handoffs = runtime.block_on(session.shutdown_within(Duration::from_millis(200)));
    assert!(handoffs.is_empty(), "the actual session shutdown aborted the async waiter");
    assert_eq!(state.settlements.pending(), 1, "the admitted work still owns its result");
    hold.release_all();
    assert!(wait_until(Duration::from_secs(3), || persistence
        .job_settlement(&identity)
        .is_some_and(|s| s.succeeded)));
    assert!(wait_until(Duration::from_secs(3), || state
        .sync
        .counters(SyncSection::SessionCommand)
        .in_flight
        == 0));
    let projection = serde_json::to_string(
        &persistence.workspace_session_projection().expect("actual projection"),
    )
    .expect("json");
    assert_eq!(occurrences(&projection, title), 1);
    let answer = ledger_after_work(&runtime, &state, invocation);
    println!(
        "CODEX_DISCONNECT_SESSION {}",
        json!({"invocation":invocation,"pending":state.settlements.pending(),"actual_session_count":1,"answer":answer})
    );
    assert_eq!(
        answer.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("confirmed"),
        "task_finished must not wait forever for an aborted caller to say it departed"
    );
}

#[test]
fn a_late_timeout_response_cannot_replace_the_original_confirmed_result() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let call = SessionCall::new("session.create", harness.workspace, "retain the real value");
    let response = runtime.block_on(drive_session(Arc::clone(&state), call.clone()));
    assert!(response.get("error").is_none(), "the original write succeeded: {response}");
    let original = runtime.block_on(query_command_outcome(Arc::clone(&state), call.invocation));
    let late_timeout = aura_ipc::JsonRpcResponse::failure(
        call.request.id,
        command_settlement::outcome_unknown_envelope(
            call.request.id,
            Some(call.invocation),
            "this caller stopped waiting before receiving the value".into(),
        ),
    );
    crate::record_command_outcome(&state, &call.request, &late_timeout).expect("late reply");
    let after = runtime.block_on(query_command_outcome(Arc::clone(&state), call.invocation));
    assert_eq!(after["result"]["value"], original["result"]["value"]);
}

#[test]
fn a_publication_receipt_keeps_the_already_committed_write_owned() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    let title = "publication finishes after the caller";
    let call = SessionCall::new("session.create", harness.workspace, title);
    let invocation = call.invocation;
    let hold = persistence.arm_admission_hold_selected(
        Some("sync"),
        &format!("session-publication:{invocation}"),
        4000,
    );
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call));
    assert!(hold.wait_for_arrivals(1, Duration::from_secs(3)));
    let projection =
        serde_json::to_string(&persistence.workspace_session_projection().expect("projection"))
            .expect("projection json");
    assert_eq!(occurrences(&projection, title), 1, "the transaction has already committed");
    let response = runtime.block_on(driver).expect("caller response");
    assert_eq!(failure_envelope(&response)["code"], "command.outcome_unknown");
    assert_eq!(state.settlements.pending(), 1);
    hold.release_all();
    let answer = ledger_after_work(&runtime, &state, invocation);
    assert_eq!(answer["result"]["value"]["outcome"], "confirmed");
    assert_eq!(answer["result"]["value"]["terminal"], "succeeded");
    assert_eq!(state.settlements.pending(), 0);
}

#[test]
fn a_post_commit_publication_failure_never_reports_the_write_as_not_run() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    let injecting =
        rusqlite::Connection::open(&harness._fixture.paths.database_path).expect("fixture DB");
    injecting
        .execute_batch(
            "CREATE TRIGGER stop_publication BEFORE UPDATE OF published_at ON projection_outbox
         BEGIN SELECT RAISE(FAIL, 'injected publication failure'); END;",
        )
        .expect("install fixture-only failure");
    let title = "a committed session is not a clean refusal";
    let call = SessionCall::new("session.create", harness.workspace, title);
    let response = runtime.block_on(drive_session(Arc::clone(&state), call.clone()));
    let envelope = failure_envelope(&response);
    assert_eq!(envelope["code"], "command.outcome_unknown");
    assert_eq!(envelope["retryable"], false);
    assert!(envelope["details"]["outcome_recovery"].is_string());
    let projection =
        serde_json::to_string(&persistence.workspace_session_projection().expect("projection"))
            .expect("projection json");
    assert_eq!(occurrences(&projection, title), 1, "the original effect exists exactly once");
    let answer = runtime.block_on(query_command_outcome(Arc::clone(&state), call.invocation));
    assert_eq!(answer["result"]["value"]["outcome"], "unknown");
    assert_eq!(state.settlements.pending(), 0, "the real failed publication has ended");
}
