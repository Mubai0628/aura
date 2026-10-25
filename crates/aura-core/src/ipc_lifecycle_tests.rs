//! Response ownership, idempotency claims and cleanup for one IPC connection.
//!
//! These cover the lifecycle rules that are easy to get subtly wrong and
//! expensive to discover in the field: a failed handler that leaves the peer
//! waiting, a reader that parks on the outbound queue, a replay of a command
//! whose effect is unknown, and registrations that outlive their scan.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use aura_domain_common::{CommandInvocationId, CorrelationId, ErrorCategory};
use aura_ipc::{
    AsyncPriorityQueue, CancellationOutcome, CancellationSettlement, LedgerError, OutboundFrame,
    QueuePriority, WireFrame,
};
use aura_runtime_contract::{
    CancelDisposition, CommandOutcome, CommandOutcomeUnknown, IpcRequestTerminal,
};

use crate::ipc_session::{Admission, HandlerTerminal, Lane};
use crate::r03_tests::Fixture;
use crate::{
    CommandSettlement, RequestLedger, finish_handled_requests, settle_command_claim, try_send_json,
};

/// The search flight registry is process-global, so tests that create and
/// release registrations must not run beside each other: one test's orphan is
/// every test's orphan.
fn flight_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::r03_tests::search_flight_lock()
}

fn control_payload(frame: OutboundFrame) -> serde_json::Value {
    match frame.frame {
        WireFrame::Control(payload) => {
            serde_json::from_slice(&payload).expect("control frame is JSON")
        }
        other => panic!("expected a control frame, got {other:?}"),
    }
}

async fn drain(queue: &AsyncPriorityQueue) -> Vec<serde_json::Value> {
    let mut frames = Vec::new();
    while let Ok(frame) = queue.recv().await {
        frames.push(control_payload(frame));
    }
    frames
}

#[tokio::test]
async fn an_unreplied_handler_gets_exactly_one_typed_error() {
    // A handler that failed before queueing anything still owes the peer one
    // reply. Reporting its terminal without sending one is what left Main
    // waiting for its own timeout to discover an internal failure.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let queue = AsyncPriorityQueue::new();
    let ledger = Arc::new(std::sync::Mutex::new(RequestLedger::new()));
    let request_id = CorrelationId::new();
    ledger.lock().expect("ledger").begin(request_id).expect("begin");

    let mut search_ids = BTreeSet::new();
    let mut command_flights = BTreeMap::new();
    let undeliverable = finish_handled_requests(
        &state,
        &queue,
        &ledger,
        &mut search_ids,
        &mut command_flights,
        vec![(request_id, HandlerTerminal::Unreplied)],
    )
    .expect("finishing one handler must succeed");
    assert!(undeliverable.is_empty(), "an application failure is still answerable");

    queue.close();
    let frames = drain(&queue).await;
    assert_eq!(frames.len(), 1, "exactly one reply, never two: {frames:?}");
    let frame = &frames[0];
    assert_eq!(frame["id"], serde_json::to_value(request_id).expect("id"));
    let error = &frame["error"];
    assert_eq!(
        error["data"]["code"], "internal.unexpected",
        "the typed code travels in the envelope data: {frame}"
    );
    assert_eq!(error["data"]["category"], "internal");
    assert_eq!(error["data"]["terminal"], "failed");
    assert_eq!(
        ledger.lock().expect("ledger").terminal(request_id),
        Some(IpcRequestTerminal::Failed),
        "the request terminal matches the reply that was sent"
    );
}

#[tokio::test]
async fn a_failed_channel_or_a_panic_retires_instead_of_answering() {
    // Both cases have no trustworthy reply to send. The reader must be told to
    // retire the session rather than leaving a request unanswered or inventing a
    // second response.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let ledger = Arc::new(std::sync::Mutex::new(RequestLedger::new()));
    let transport_id = CorrelationId::new();
    let panic_id = CorrelationId::new();
    {
        let mut ledger = ledger.lock().expect("ledger");
        ledger.begin(transport_id).expect("begin");
        ledger.begin(panic_id).expect("begin");
    }
    let queue = AsyncPriorityQueue::new();
    let mut search_ids = BTreeSet::new();
    let mut command_flights = BTreeMap::new();
    let undeliverable = finish_handled_requests(
        &state,
        &queue,
        &ledger,
        &mut search_ids,
        &mut command_flights,
        vec![
            (transport_id, HandlerTerminal::TransportFailed),
            (panic_id, HandlerTerminal::Panicked),
        ],
    )
    .expect("finishing must succeed");
    assert_eq!(
        undeliverable,
        vec![transport_id, panic_id],
        "both must be reported as undeliverable, not silently swallowed"
    );
    queue.close();
    assert!(
        drain(&queue).await.is_empty(),
        "a retired session must not also answer: a second response would be a protocol error"
    );
    assert_eq!(ledger.lock().expect("ledger").terminal(transport_id), None);
}

#[tokio::test]
async fn a_full_priority_queue_is_reported_instead_of_parking_the_reader() {
    // The reader answers refusals itself. If that answer cannot be queued, the
    // reader must learn it immediately — parking would starve cancellation and
    // connection control.
    let queue = AsyncPriorityQueue::new();
    let mut sent = 0;
    let outcome = loop {
        match try_send_json(&queue, QueuePriority::P0, &serde_json::json!({ "probe": sent })) {
            Ok(()) => sent += 1,
            Err(error) => break error,
        }
    };
    assert!(sent > 0, "the reserved partition must accept at least one answer");
    assert!(
        outcome.to_string().contains("could not be answered"),
        "a full queue must be an explicit connection failure: {outcome}"
    );
}

#[tokio::test]
async fn a_started_command_without_a_recorded_outcome_stays_unknown() {
    // The command ran, so its effect may exist. Releasing the claim would let a
    // replay execute it a second time; recording `Unknown` both answers
    // `runtime.command_outcome` honestly and makes the replay a conflict.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let invocation = CommandInvocationId::new();
    let claim = state
        .command_outcomes
        .lock()
        .expect("outcomes")
        .begin(invocation, CorrelationId::new())
        .expect("the claim starts free");

    settle_command_claim(&state, &claim, CommandSettlement::Ran);

    let outcomes = state.command_outcomes.lock().expect("outcomes");
    assert_eq!(
        outcomes.query(invocation),
        CommandOutcome::Unknown {
            command_invocation_id: invocation,
            reason: CommandOutcomeUnknown::NotRecordedOrIncomplete,
        },
        "an unrecorded result must be recorded as unknown, never as success or failure"
    );
    assert!(!outcomes.is_in_flight(invocation), "the attempt itself is over");
    assert!(
        outcomes.recorded_outcome_is_unknown(invocation),
        "the refusal can then tell the caller to query instead of replaying"
    );
}

#[tokio::test]
async fn a_side_effecting_command_is_not_refused_by_the_claim_its_own_connection_took() {
    // The reader claims a side-effecting command's CommandInvocationId before the
    // handler can run, so a duplicate check inside the handler that ignores *who*
    // took the claim reads that very claim as "someone else is executing this" and
    // refuses the request that owns it. Everything real side-effecting work does
    // goes through this pairing, and no test covered it: the reader's claim and the
    // handler's check were only ever tested apart.
    let fixture = Fixture::new();
    let state = Arc::new(fixture.start().await);
    let queue = AsyncPriorityQueue::new();
    let ledger = Arc::new(std::sync::Mutex::new(RequestLedger::new()));
    let request = aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: CorrelationId::new(),
        command_invocation_id: Some(CommandInvocationId::new()),
        method: "security.host_preparation.prepare".to_owned(),
        params: serde_json::json!({ "expected_revision": 0 }),
    };
    let mut command_flights = BTreeMap::new();

    // Reader step 0: the request ledger owns exactly one terminal per request.
    ledger.lock().expect("ledger").begin(request.id).expect("begin");

    // Reader step 1: claim, exactly as the read loop does.
    let reservation = crate::reserve_command_invocation(&state, &queue, &request)
        .await
        .expect("the reader's claim is an ordinary step");
    let crate::CommandReservation::Granted(Some(claimed)) = reservation else {
        panic!("a fresh invocation id must be granted");
    };
    command_flights.insert(request.id, claimed);

    // Reader step 2: admit and start the handler on its lane.
    let mut session = crate::ipc_session::IpcSession::new();
    assert_eq!(session.admit(request.clone()), crate::ipc_session::Admission::Accepted);
    crate::spawn_ready_handlers(
        &state,
        &queue,
        &Arc::new(crate::ProjectionSubscriptions::default()),
        &ledger,
        &command_flights,
        &mut session,
    )
    .expect("the handler starts on its lane");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while session.is_busy() && tokio::time::Instant::now() < deadline {
        let _ = crate::finish_handled_requests(
            &state,
            &queue,
            &ledger,
            &mut BTreeSet::new(),
            &mut command_flights,
            session.poll_completions(),
        )
        .expect("handler terminals close cleanly");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(!session.is_busy(), "the handler must finish");

    queue.close();
    let frames = drain(&queue).await;
    let error = frames
        .first()
        .expect("the handler owes the peer exactly one answer")
        .get("error")
        .expect("this fixture has no probed Host Preparation state, so it fails");
    assert_ne!(
        error["data"]["code"], "ipc.command_invocation_in_flight",
        "the handler must not read its own connection's claim as a replay: {frames:?}"
    );
    let code = error["data"]["code"].as_str().unwrap_or_default();
    assert!(
        code.starts_with("sandbox.host_preparation_") || code == "persistence.internal",
        "the request must reach its own handler logic, not the claim check: {frames:?}"
    );
}

#[tokio::test]
async fn a_direct_caller_with_a_claimed_id_is_still_refused() {
    // The other half of the rule: ownership exempts *this* request, not duplicate
    // ids in general. A caller that did not claim the id — or a genuine replay —
    // still gets the frozen refusal.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let queue = AsyncPriorityQueue::new();
    let invocation = CommandInvocationId::new();
    let claim = state
        .command_outcomes
        .lock()
        .expect("outcomes")
        .begin(invocation, CorrelationId::new())
        .expect("the first attempt owns the id");
    let request = aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: CorrelationId::new(),
        command_invocation_id: Some(invocation),
        method: "security.host_preparation.prepare".to_owned(),
        params: serde_json::json!({ "expected_revision": 0 }),
    };
    let terminal = crate::handle_request(
        &state,
        &queue,
        &crate::ProjectionSubscriptions::default(),
        &Arc::new(std::sync::Mutex::new(RequestLedger::new())),
        &request,
        None,
    )
    .await
    .expect("the handler answers this request");
    assert_eq!(terminal, IpcRequestTerminal::Failed);
    queue.close();
    let frames = drain(&queue).await;
    assert_eq!(
        frames[0]["error"]["data"]["code"], "ipc.command_invocation_in_flight",
        "a duplicate that does not own the claim is still refused: {frames:?}"
    );
    // The original attempt is untouched: still claimed, and no outcome invented.
    let outcomes = state.command_outcomes.lock().expect("outcomes");
    assert!(outcomes.owns(&claim), "a duplicate's refusal must not release the claim");
    assert!(
        !outcomes.entries.contains_key(&invocation),
        "a duplicate's refusal must not write an outcome for the original"
    );
}

#[tokio::test]
async fn a_borrowed_or_absent_claim_cannot_authorise_another_request() {
    // Credentials are not bearer tokens. A second request that presents the first
    // request's credential is refused, and a request that reaches the handler with
    // no credential at all cannot execute — every execution path takes its claim.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let subscriptions = crate::ProjectionSubscriptions::default();
    let request_ledger = Arc::new(std::sync::Mutex::new(RequestLedger::new()));
    let invocation = CommandInvocationId::new();
    let owner = CorrelationId::new();
    let claim = state
        .command_outcomes
        .lock()
        .expect("outcomes")
        .begin(invocation, owner)
        .expect("the owner claims the id");
    let duplicate = aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: CorrelationId::new(),
        command_invocation_id: Some(invocation),
        method: "security.host_preparation.prepare".to_owned(),
        params: serde_json::json!({ "expected_revision": 0 }),
    };

    // 1. Borrowed: another request presents the owner's credential.
    let borrowed_queue = AsyncPriorityQueue::new();
    crate::handle_request(
        &state,
        &borrowed_queue,
        &subscriptions,
        &request_ledger,
        &duplicate,
        Some(&claim),
    )
    .await
    .expect("answered");
    borrowed_queue.close();
    let borrowed_frames = drain(&borrowed_queue).await;
    assert_eq!(
        borrowed_frames[0]["error"]["data"]["code"], "ipc.command_invocation_in_flight",
        "a credential issued to another request is not authority: {borrowed_frames:?}"
    );

    // 2. Absent: a request whose id nobody claimed at all.
    let orphan = aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: CorrelationId::new(),
        command_invocation_id: Some(CommandInvocationId::new()),
        method: "security.host_preparation.prepare".to_owned(),
        params: serde_json::json!({ "expected_revision": 0 }),
    };
    let orphan_queue = AsyncPriorityQueue::new();
    crate::handle_request(&state, &orphan_queue, &subscriptions, &request_ledger, &orphan, None)
        .await
        .expect("answered");
    orphan_queue.close();
    let orphan_frames = drain(&orphan_queue).await;
    assert_eq!(
        orphan_frames[0]["error"]["data"]["code"], "internal.unexpected",
        "a command that arrives without its claim is not executed: {orphan_frames:?}"
    );
    assert!(
        state.command_outcomes.lock().expect("outcomes").owns(&claim),
        "neither refusal may disturb the owner's claim"
    );
}

#[tokio::test]
async fn a_stale_claim_from_an_earlier_attempt_is_not_authority() {
    // After a claim is released and the id is claimed again, the earlier
    // credential names a generation that is no longer live.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let queue = AsyncPriorityQueue::new();
    let request_ledger = Arc::new(std::sync::Mutex::new(RequestLedger::new()));
    let invocation = CommandInvocationId::new();
    let owner = CorrelationId::new();
    let (stale, live) = {
        let mut outcomes = state.command_outcomes.lock().expect("outcomes");
        let stale = outcomes.begin(invocation, owner).expect("first generation");
        outcomes.abandon(&stale);
        let live = outcomes.begin(invocation, owner).expect("second generation");
        (stale, live)
    };
    let request = aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: owner,
        command_invocation_id: Some(invocation),
        method: "security.host_preparation.prepare".to_owned(),
        params: serde_json::json!({ "expected_revision": 0 }),
    };
    crate::handle_request(
        &state,
        &queue,
        &crate::ProjectionSubscriptions::default(),
        &request_ledger,
        &request,
        Some(&stale),
    )
    .await
    .expect("answered");
    queue.close();
    let frames = drain(&queue).await;
    assert_eq!(
        frames[0]["error"]["data"]["code"], "ipc.command_invocation_in_flight",
        "the previous generation's credential authorises nothing: {frames:?}"
    );
    let outcomes = state.command_outcomes.lock().expect("outcomes");
    assert!(outcomes.owns(&live), "the live claim is untouched by the stale credential");
}

#[tokio::test]
async fn only_one_of_two_connections_can_execute_the_same_invocation_id() {
    // Two connections race for one id. The ledger is process-global and the
    // decision is atomic, so exactly one request is granted and the other is
    // refused — and the refusal goes to the loser's own queue.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let invocation = CommandInvocationId::new();
    let request_for = || aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: CorrelationId::new(),
        command_invocation_id: Some(invocation),
        method: "workspace.create".to_owned(),
        params: serde_json::json!({}),
    };
    let first_queue = AsyncPriorityQueue::new();
    let second_queue = AsyncPriorityQueue::new();
    let granted = crate::reserve_command_invocation(&state, &first_queue, &request_for())
        .await
        .expect("the first claim is an ordinary step");
    let crate::CommandReservation::Granted(Some(claim)) = granted else {
        panic!("the first attempt must be granted");
    };
    let refused = crate::reserve_command_invocation(&state, &second_queue, &request_for())
        .await
        .expect("a refusal is answered, not an error");
    assert!(
        matches!(refused, crate::CommandReservation::Refused),
        "the second attempt must not be granted"
    );
    second_queue.close();
    let frames = drain(&second_queue).await;
    assert_eq!(
        frames[0]["error"]["data"]["code"], "ipc.command_invocation_in_flight",
        "the loser learns why, on its own queue: {frames:?}"
    );
    // Closing is what ends `drain`: the winner's queue holds no frame at all.
    first_queue.close();
    assert!(drain(&first_queue).await.is_empty(), "the winner's queue carries no refusal");
    assert!(state.command_outcomes.lock().expect("outcomes").owns(&claim));
}

#[tokio::test]
async fn a_claim_released_before_it_started_leaves_the_id_free() {
    // A request refused for capacity, or dropped while still queued, never reached
    // a handler: nothing could have happened, so its claim is released and the id
    // can be used again. This is the decision the read loop makes in that branch.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let invocation = CommandInvocationId::new();
    let claim = state
        .command_outcomes
        .lock()
        .expect("outcomes")
        .begin(invocation, CorrelationId::new())
        .expect("claim");
    settle_command_claim(&state, &claim, CommandSettlement::NeverRan);
    let outcomes = state.command_outcomes.lock().expect("outcomes");
    assert!(!outcomes.is_in_flight(invocation), "the never-started claim is gone");
    assert!(
        !outcomes.entries.contains_key(&invocation),
        "and no outcome was invented for work that never ran"
    );
}

#[tokio::test]
async fn a_command_that_never_ran_is_released_and_a_recorded_one_is_kept() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let never_ran = CommandInvocationId::new();
    let recorded = CommandInvocationId::new();
    let (never_ran_claim, recorded_claim) = {
        let mut outcomes = state.command_outcomes.lock().expect("outcomes");
        let never_ran_claim =
            outcomes.begin(never_ran, CorrelationId::new()).expect("the refused request's claim");
        let recorded_claim =
            outcomes.begin(recorded, CorrelationId::new()).expect("the recorded command's claim");
        outcomes.insert(
            recorded,
            CommandOutcome::Confirmed {
                command_invocation_id: recorded,
                terminal: IpcRequestTerminal::Succeeded,
                response: serde_json::json!({ "terminal": "succeeded" }),
            },
        );
        (never_ran_claim, recorded_claim)
    };

    settle_command_claim(&state, &never_ran_claim, CommandSettlement::NeverRan);
    settle_command_claim(&state, &recorded_claim, CommandSettlement::Ran);

    let mut outcomes = state.command_outcomes.lock().expect("outcomes");
    assert!(
        outcomes.begin(never_ran, CorrelationId::new()).is_some(),
        "a request refused for capacity left no effect, so its id is usable again"
    );
    assert!(
        outcomes.begin(recorded, CorrelationId::new()).is_none(),
        "a recorded outcome is never overwritten by cleanup"
    );
    assert!(
        matches!(outcomes.query(recorded), CommandOutcome::Confirmed { .. }),
        "the recorded result stays exactly as the handler wrote it"
    );
}

#[tokio::test]
async fn only_real_completion_releases_a_search_registration() {
    let _serial = flight_lock();
    // The old shape of this test asserted that a bounded wait expiring releases
    // the registration and counts the scan as abandoned. That fixed the wrong
    // behaviour: an expired wait is not reclamation. The rules are now:
    //
    // * a coordinator that returned is *not* enough while work is admitted;
    // * a wait that expires releases nothing and keeps the entry;
    // * only "coordinator returned and admitted work ended" releases it;
    // * a request that never started has nothing running, so it is released.

    // 1. Never started: nothing was admitted and no coordinator ran.
    let unstarted = CorrelationId::new();
    aura_fs_tools::register_search(unstarted, aura_fs_tools::SearchCancel::new());
    assert!(
        aura_fs_tools::release_unstarted_search(unstarted, aura_fs_tools::NeverStarted),
        "a request that never started has no work to wait for"
    );
    assert!(aura_fs_tools::search_flight(unstarted).is_none());

    // 2. Coordinator returned while one piece of work is still running.
    let running = CorrelationId::new();
    let cancel = aura_fs_tools::SearchCancel::new();
    let work = cancel.admit_work();
    aura_fs_tools::register_search(running, cancel.clone());
    cancel.mark_coordinator_done();
    assert!(!cancel.is_finished(), "admitted work is still running");
    assert!(
        !aura_fs_tools::release_search_flight(running),
        "a returned coordinator is not a finished scan"
    );
    assert!(
        aura_fs_tools::search_flight(running).is_some(),
        "the registration must survive a wait that cannot succeed yet"
    );

    // 3. A bounded wait expiring keeps the work tracked and reports it.
    aura_fs_tools::orphan_search([running]);
    let still_running = aura_fs_tools::wait_for_search_orphans(Duration::from_millis(60));
    assert_eq!(
        still_running,
        vec![running],
        "the expired wait must report the scan as still running"
    );
    assert!(
        aura_fs_tools::search_flight(running).is_some(),
        "an expired wait must not release or drop the real work"
    );
    assert!(aura_fs_tools::search_orphan_count() >= 1, "it stays an orphan");

    // 4. The work ends: now the registration goes. Either this call releases it,
    // or the registry's own sweeper already did when it observed the same
    // completion — the entry is orphaned at this point, and both are "released
    // because the work really ended". What must never happen is the registration
    // outliving the work.
    drop(work);
    assert!(cancel.is_finished(), "no admitted work left and the coordinator returned");
    let released_here = aura_fs_tools::release_search_flight(running);
    assert!(
        released_here || aura_fs_tools::search_flight(running).is_none(),
        "the registration must not survive the work that really ended"
    );
    assert!(aura_fs_tools::search_flight(running).is_none(), "and it is gone");
    // Deliberately not a global count: the registry is process-global, and other
    // tests in this binary run real searches through their own paths. This test's
    // claim is about the search it registered, and that is what it checks.
}

#[tokio::test]
async fn a_handoff_with_unfinished_work_keeps_the_registration_until_it_really_ends() {
    let _serial = flight_lock();
    // Registry-level test: the hand-off rules are exercised directly, not through
    // a real connection. Production-path evidence for the disconnect is the
    // Electron Named Pipe disconnect reconnects to the same Core instance E2E
    // plus the real search worker test in ura-fs-tools; this test pins the
    // ownership rule: a worker is admitted and still running, the connection has
    // cancelled and given up waiting, and the scan must stay tracked until its
    // work really ends.
    let harness = aura_fs_tools::SearchCancel::new();
    let request_id = CorrelationId::new();
    let work = harness.admit_work();
    aura_fs_tools::register_search(request_id, harness.clone());

    // Disconnect: cancel, then hand the search to the registry because the
    // bounded wait could not prove reclamation.
    aura_fs_tools::cancel_search(request_id);
    aura_fs_tools::orphan_search([request_id]);
    let still_running = aura_fs_tools::wait_for_search_orphans(Duration::from_millis(50));
    assert_eq!(still_running, vec![request_id], "the parked worker is not reclaimed");
    assert!(
        aura_fs_tools::search_flight(request_id).is_some(),
        "cancel requested and coordinator gone still say nothing about the scan"
    );

    // The worker ends (the gate opens): the completion is received and released.
    harness.mark_coordinator_done();
    drop(work);
    assert_eq!(
        aura_fs_tools::wait_for_search_orphans(Duration::from_secs(5)),
        Vec::<CorrelationId>::new(),
        "the sweeper must observe the real completion"
    );
    assert!(
        aura_fs_tools::search_flight(request_id).is_none(),
        "the registration is released once the work really ended"
    );
}

#[tokio::test]
async fn another_connections_search_is_untouched_by_this_ones_cleanup() {
    let _serial = flight_lock();
    let mine = CorrelationId::new();
    let theirs = CorrelationId::new();
    let my_cancel = aura_fs_tools::SearchCancel::new();
    let their_cancel = aura_fs_tools::SearchCancel::new();
    let my_work = my_cancel.admit_work();
    let their_work = their_cancel.admit_work();
    aura_fs_tools::register_search(mine, my_cancel.clone());
    aura_fs_tools::register_search(theirs, their_cancel.clone());

    aura_fs_tools::cancel_search(mine);
    aura_fs_tools::orphan_search([mine]);
    my_cancel.mark_coordinator_done();
    drop(my_work);
    assert!(aura_fs_tools::release_search_flight(mine), "this connection's search finished");
    assert!(
        aura_fs_tools::search_flight(theirs).is_some(),
        "another connection's running search keeps its registration"
    );
    assert!(
        !aura_fs_tools::release_search_flight(theirs),
        "and it is not released by this connection's cleanup"
    );

    their_cancel.mark_coordinator_done();
    drop(their_work);
    assert!(aura_fs_tools::release_search_flight(theirs));
}

#[tokio::test]
async fn a_saturated_queue_fails_every_reader_reply_path_without_waiting() {
    // The case that matters is the combination: a full P0 partition while a
    // repeated invocation, a request with no invocation id, and a cancellation
    // answer all arrive. None of them may park the reader — each must report the
    // connection failure so cleanup runs instead.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let repeated = CommandInvocationId::new();
    state
        .command_outcomes
        .lock()
        .expect("outcomes")
        .begin(repeated, CorrelationId::new())
        .expect("the original command is already executing");

    // Saturate the reserved P0 partition with reader answers.
    let queue = AsyncPriorityQueue::new();
    while try_send_json(&queue, QueuePriority::P0, &serde_json::json!({ "keep": true })).is_ok() {}

    let request = |command_invocation_id: Option<CommandInvocationId>| aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: CorrelationId::new(),
        command_invocation_id,
        method: "workspace.create".to_owned(),
        params: serde_json::json!({}),
    };

    // 1. A repeated invocation id: the claim is refused, and the refusal cannot
    //    be queued, so the reader learns the connection is gone.
    assert!(
        crate::reserve_command_invocation(&state, &queue, &request(Some(repeated))).await.is_err(),
        "a refusal that cannot be queued must be reported, not retried forever"
    );

    // 2. A request with no invocation id takes the same path.
    assert!(
        crate::reserve_command_invocation(&state, &queue, &request(None)).await.is_err(),
        "the missing-id refusal must not wait for queue space either"
    );

    // 3. A cancellation answer is written by the reader as well.
    let id = CorrelationId::new();
    assert!(
        crate::send_control_answer(
            &queue,
            &aura_ipc::JsonRpcResponse::failure(
                id,
                crate::handler_failed_error(id).expect("envelope"),
            ),
        )
        .is_err(),
        "the reader's own answers must fail fast when the partition is full"
    );
    assert!(
        state.command_outcomes.lock().expect("outcomes").is_in_flight(repeated),
        "the original command's claim is untouched by refused duplicates"
    );
}

#[tokio::test]
async fn one_connections_cleanup_leaves_another_connections_claim_alone() {
    // The claim ledger is process-global, so cleanup must be per request id.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let mine = CommandInvocationId::new();
    let theirs = CommandInvocationId::new();
    let (mine_claim, theirs_claim) = {
        let mut outcomes = state.command_outcomes.lock().expect("outcomes");
        let mine_claim = outcomes.begin(mine, CorrelationId::new()).expect("this connection's");
        let theirs_claim =
            outcomes.begin(theirs, CorrelationId::new()).expect("another connection's");
        (mine_claim, theirs_claim)
    };
    settle_command_claim(&state, &mine_claim, CommandSettlement::Ran);
    let outcomes = state.command_outcomes.lock().expect("outcomes");
    assert!(outcomes.owns(&theirs_claim), "another connection's running command must stay claimed");
    assert!(
        !outcomes.recorded_outcome_is_unknown(theirs),
        "cleanup must not write an outcome for work it does not own"
    );
}

/// One connection's real search, started exactly as `serve_connection` starts it.
struct SearchConnection {
    queue: AsyncPriorityQueue,
    ledger: Arc<std::sync::Mutex<RequestLedger>>,
    session: crate::ipc_session::IpcSession,
    search_ids: BTreeSet<CorrelationId>,
    command_flights: crate::CommandFlights,
}

impl SearchConnection {
    fn new() -> Self {
        Self {
            queue: AsyncPriorityQueue::new(),
            ledger: Arc::new(std::sync::Mutex::new(RequestLedger::new())),
            session: crate::ipc_session::IpcSession::new(),
            search_ids: BTreeSet::new(),
            command_flights: crate::CommandFlights::new(),
        }
    }

    /// Admit a real `fs.search.text` request and start the real handler on its
    /// lane, registering the search flight the way the read loop does.
    fn start_search(
        &mut self,
        state: &Arc<crate::RuntimeState>,
        request: aura_ipc::JsonRpcRequest,
    ) -> CorrelationId {
        let request_id = request.id;
        self.ledger.lock().expect("ledger").begin(request_id).expect("begin");
        aura_fs_tools::register_search(request_id, aura_fs_tools::SearchCancel::new());
        self.search_ids.insert(request_id);
        assert_eq!(
            self.session.admit(request),
            crate::ipc_session::Admission::Accepted,
            "the read-only lane has room for one search"
        );
        crate::spawn_ready_handlers(
            state,
            &self.queue,
            &Arc::new(crate::ProjectionSubscriptions::default()),
            &self.ledger,
            &self.command_flights,
            &mut self.session,
        )
        .expect("the handler starts on its lane");
        assert_eq!(self.session.running_count(), 1, "the handler really took the lane");
        request_id
    }

    async fn disconnect(&mut self, state: &Arc<crate::RuntimeState>) -> crate::ConnectionReclaim {
        crate::reclaim_connection_work(
            state,
            &self.queue,
            &self.ledger,
            &mut self.session,
            &mut self.search_ids,
            &mut self.command_flights,
        )
        .await
    }
}

fn search_request(
    workspace: &aura_workspace_domain::WorkspaceSummary,
    session: aura_domain_common::SessionId,
    request_id: CorrelationId,
    pattern: &str,
) -> aura_ipc::JsonRpcRequest {
    aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: request_id,
        command_invocation_id: None,
        method: aura_ipc::method::FS_SEARCH_TEXT.to_owned(),
        params: serde_json::json!({
            "workspace_id": workspace.workspace_id,
            "session_id": session,
            "pattern": {"kind": "literal", "text": pattern, "case_sensitive": true},
            "root": ".",
            "path_glob": null,
            "include_ignored": false,
            "include_generated": false,
            "limit": 50
        }),
    }
}

#[test]
fn a_real_search_worker_outlives_its_disconnected_connection() {
    // The flight registry is process-global, so this test holds the shared lock for
    // its whole run. That lock is deliberately taken in a *synchronous* test body:
    // an async test would hold a std guard across await points, and the production
    // rule under test needs awaits (bounded waits, a real disconnect).
    let _serial = flight_lock();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(real_search_worker_outlives_its_disconnected_connection());
}

async fn real_search_worker_outlives_its_disconnected_connection() {
    // The whole chain, with nothing standing in for the parts that matter: a real
    // Core fixture, a real workspace, a real search handler on a real lane, the
    // engine's own worker parked inside one file's scan, and the production
    // disconnect cleanup. The claim under test is the three-facts rule — the
    // connection is gone, but the scan is not, so nothing may be reported as
    // reclaimed until the worker really ends.
    use aura_domain_common::SessionId;
    use aura_workspace_domain::{SessionSummary, WorkspaceSummary};

    let fixture = Fixture::new();
    let state = Arc::new(fixture.start().await);
    let root = fixture.root.path().join("workspace");
    std::fs::create_dir_all(&root).expect("workspace");
    std::fs::write(root.join("gated.txt"), "blocked_worker_token\n").expect("gated file");
    std::fs::write(root.join("plain.txt"), "quick_worker_token\n").expect("plain file");
    let created = crate::r03_tests::success(
        &state,
        aura_ipc::method::WORKSPACE_CREATE,
        serde_json::json!({"selected_root": root, "display_name": "IPC lifecycle"}),
    )
    .await;
    let workspace: WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session = SessionId::new();
    let crate::StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must be ready");
    };
    services
        .persistence
        .upsert_workspace_session(
            workspace.clone(),
            SessionSummary {
                session_id: session,
                workspace_id: workspace.workspace_id,
                title: "IPC lifecycle".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("session");

    // The gate is armed before the search starts and models a worker that is
    // already inside one blocking OS call: the disconnect's cancel cannot end it.
    let gate = aura_fs_tools::test_faults::arm_blocking_gate("gated.txt");

    let mut blocked = SearchConnection::new();
    let blocked_id = blocked.start_search(
        &state,
        search_request(&workspace, session, CorrelationId::new(), "blocked_worker_token"),
    );
    assert!(
        gate.wait_for_parked(1, Duration::from_secs(20)),
        "a real engine worker must be inside the gated file's scan"
    );
    assert_eq!(
        blocked.session.running_count(),
        1,
        "the real handler is still on its lane while its worker is parked"
    );
    assert!(
        aura_fs_tools::search_flight(blocked_id).is_some(),
        "the search is registered while its worker is parked"
    );
    assert_eq!(
        aura_fs_tools::search_finished(blocked_id),
        Some(false),
        "a parked worker means the scan is not finished, even if its coordinator returned"
    );

    // A second connection runs a real search as well, so "this connection's
    // cleanup" has something it must not touch.
    let mut quick = SearchConnection::new();
    let quick_id = quick.start_search(
        &state,
        search_request(&workspace, session, CorrelationId::new(), "quick_worker_token"),
    );

    // The gated connection disconnects. Its worker is inside one blocking OS
    // call, so it ends only after the bounded reclaim wait has already expired —
    // the timeout path is therefore the one taken, and the facts must say so.
    // The release is deliberately far behind the 2 s budget: on a loaded machine
    // the test thread must still reach the wait before the worker ends, or the
    // scenario silently becomes "the scan finished in time".
    let release_after_timeout = {
        let gate = Arc::clone(&gate);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(8));
            gate.release();
        })
    };
    let reclaim = blocked.disconnect(&state).await;
    assert_eq!(
        reclaim.still_running,
        vec![blocked_id],
        "the expired wait must hand this search to the registry, not reclaim it"
    );
    assert!(
        reclaim.started.contains(&blocked_id),
        "the blocked request really took a lane, so it is not a never-started release"
    );
    release_after_timeout.join().expect("gate releaser");

    // The worker ended, and only then was the registration released: real
    // completion is what reclaims the search, and nothing is left behind.
    //
    // The reclaim's own wait is bounded, so a coordinator that is still draining when
    // the wait expires is handed over rather than waited for. Its completion still runs
    // to the end — that is the point of the hand-over — so this waits for the real
    // completion instead of asserting on a fixed deadline.
    //
    // What this test does *not* claim: the sweeper keeping a live orphan across a
    // coordinator that returned early. Here the coordinator waits for its worker
    // (the engine's drain), so the hand-over and the completion meet inside the
    // same teardown. That window — coordinator gone while a worker still runs —
    // is covered by the registry test above, which is labelled as registry-level
    // for exactly this reason.
    let mut released = false;
    for _ in 0..2_000 {
        if aura_fs_tools::search_flight(blocked_id).is_none() {
            released = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        released,
        "the registry must release the search once its work really ended, even when the \
         connection's own wait expired first"
    );
    assert_eq!(
        aura_fs_tools::search_orphan_count(),
        0,
        "no orphan is left behind after the real completion"
    );

    // The second connection finishes normally: its own teardown has nothing left
    // to hand over, which is what distinguishes a real completion from a timeout.
    let quick_reclaim = quick.disconnect(&state).await;
    assert!(
        quick_reclaim.still_running.is_empty(),
        "a search that finished inside the wait is released, not orphaned"
    );
    assert!(
        aura_fs_tools::search_flight(quick_id).is_none(),
        "its registration is gone because the scan really finished"
    );
}

/// A handler parked in synchronous work outlives its connection inside a budget.
///
/// `abort()` only takes effect at a task's next await point. A handler that is inside
/// a blocking call — a host API, a persistence write — never reaches one, so awaiting
/// it is an unbounded wait that no budget can interrupt. That is how a connection
/// teardown outlived its two-second reclaim budget while the read loop waited for it.
///
/// Nothing here stands in for the parts that matter: the work is a real synchronous
/// blocking call on a real thread, the handler runs on a real `IpcSession` lane, and
/// the teardown is the production `reclaim_connection_work`.
#[test]
fn a_handler_in_synchronous_work_outlives_its_connection_inside_the_budget() {
    // The reclaim path hands work to process-global owners, so this test does not run
    // beside the search-flight tests.
    let _serial = flight_lock();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(handler_in_synchronous_work());
}

async fn handler_in_synchronous_work() {
    let fixture = Fixture::new();
    let state = Arc::new(fixture.start().await);

    // A real synchronous block: the handler waits on this condition with no await point
    // in between, so `abort()` cannot end it and the task cannot finish before the test
    // releases it.
    let gate: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)> =
        Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    // The real work starts here, before the handler is even admitted. Its "I am running"
    // handshake is a channel with a bounded wait, not a fixed number of `yield_now`
    // calls: a yield count is a race on a loaded machine, and this test has to observe that
    // the work is running *before* it measures the teardown.
    let (worker_started_tx, worker_started_rx) = std::sync::mpsc::channel::<()>();
    let worker_finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let finished_flag = Arc::clone(&worker_finished);
        let worker_gate = Arc::clone(&gate);
        std::thread::spawn(move || {
            let _ = worker_started_tx.send(());
            let (lock, signal) = &*worker_gate;
            let mut released = lock.lock().expect("work gate");
            while !*released {
                let Ok(next) = signal.wait_timeout(released, Duration::from_secs(30)) else {
                    return;
                };
                released = next.0;
                if next.1.timed_out() {
                    return;
                }
            }
            finished_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
    }
    assert!(
        worker_started_rx.recv_timeout(Duration::from_secs(10)).is_ok(),
        "the blocking work must really be running before the teardown is measured"
    );

    let mut connection = SearchConnection::new();
    let request_id = read_only_request_id();
    connection.ledger.lock().expect("ledger").begin(request_id).expect("begin");
    assert_eq!(
        connection.session.admit(read_only_request_named_value(request_id)),
        crate::ipc_session::Admission::Accepted
    );
    let handler_gate = Arc::clone(&gate);
    crate::spawn_ready_handlers_with(&mut connection.session, move || {
        let handler_gate = Arc::clone(&handler_gate);
        async move {
            // The handler is inside a real blocking call: it waits, with no await point
            // in between, for work running on another thread. An abort cannot end that
            // work, and the task cannot finish before the work does.
            tokio::task::block_in_place(move || {
                let (lock, signal) = &*handler_gate;
                let mut released = lock.lock().expect("work gate");
                while !*released {
                    let Ok(next) = signal.wait_timeout(released, Duration::from_secs(30)) else {
                        return;
                    };
                    released = next.0;
                    if next.1.timed_out() {
                        return;
                    }
                }
            });
            terminal_succeeded()
        }
    });
    assert_eq!(connection.session.running_count(), 1, "the handler took its lane");
    assert_eq!(connection.session.queued_count(), 0);

    // The teardown must return inside its budget even though the handler cannot stop.
    let started = std::time::Instant::now();
    let reclaim = connection.disconnect(&state).await;
    let elapsed = started.elapsed();
    assert!(
        reclaim.started.contains(&request_id),
        "the request really took a lane, so its effect cannot be called a no-op"
    );
    assert!(
        elapsed < crate::HANDLER_RECLAIM_TIMEOUT + Duration::from_secs(3),
        "the teardown waited {elapsed:?}, which means it joined unbounded work"
    );
    assert_eq!(
        connection.session.running_count(),
        0,
        "the lane is released even though the work is not"
    );
    assert_eq!(connection.session.queued_count(), 0, "queued work is dropped, never promoted");
    assert!(
        crate::detached_handlers::live() >= 1,
        "the work keeps an owner instead of being dropped with its task"
    );
    assert!(
        !worker_finished.load(std::sync::atomic::Ordering::SeqCst),
        "the blocking work has to be unfinished at the moment the teardown reports it"
    );

    // A new connection to the same Core is not blocked by the unfinished work, and it
    // answers an ordinary read-only request.
    let mut replacement = SearchConnection::new();
    let replacement_id = read_only_request_id();
    replacement.ledger.lock().expect("ledger").begin(replacement_id).expect("begin");
    assert_eq!(
        replacement.session.admit(read_only_request_named_value(replacement_id)),
        crate::ipc_session::Admission::Accepted,
        "the new connection must be admitted while the old work still runs"
    );
    let replacement_queue = replacement.queue.clone();
    crate::spawn_ready_handlers_with(&mut replacement.session, || async { terminal_succeeded() });
    let completed = drain_completions(&mut replacement.session, 1).await;
    assert_eq!(completed.len(), 1, "the new connection answers its own request");
    assert_eq!(replacement.session.running_count(), 0);

    // Release the blocked work. Only its real completion releases the owner, and no
    // late result reaches the replacement connection.
    {
        let (lock, signal) = &*gate;
        let mut released = lock.lock().expect("work gate");
        *released = true;
        signal.notify_all();
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !worker_finished.load(std::sync::atomic::Ordering::SeqCst)
        && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(worker_finished.load(std::sync::atomic::Ordering::SeqCst), "the work really ended");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while crate::detached_handlers::live() != 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        crate::detached_handlers::live(),
        0,
        "the owner is released once the work really finishes"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), replacement_queue.recv()).await.is_err(),
        "a late result must not be delivered anywhere"
    );
}

fn read_only_request_id() -> CorrelationId {
    CorrelationId::new()
}

/// A read-only request, named so the lanes classify it exactly as they do in
/// production: the method is the one the registry marks `ReadOnly`.
fn read_only_request_named_value(id: CorrelationId) -> aura_ipc::JsonRpcRequest {
    aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id,
        command_invocation_id: None,
        method: "runtime.readiness".to_owned(),
        params: serde_json::json!({}),
    }
}

fn terminal_succeeded() -> HandlerTerminal {
    HandlerTerminal::Answered(IpcRequestTerminal::Succeeded)
}

async fn drain_completions(
    session: &mut crate::ipc_session::IpcSession,
    expected: usize,
) -> Vec<(CorrelationId, HandlerTerminal)> {
    let mut completed = Vec::new();
    for _ in 0..500 {
        completed.extend(session.poll_completions());
        if completed.len() >= expected {
            break;
        }
        tokio::task::yield_now().await;
    }
    completed
}

// ---------------------------------------------------------------------------
// Scheduling, cancellation and result ownership.
//
// These tests drive the lanes, the request ledger and the claims through the same
// steps the read loop takes — claim, admit, start, close the terminal — because
// the facts under test are about *which* of those steps saw what, and a stand-in
// would answer that itself.
// ---------------------------------------------------------------------------

/// An ordered business request. Its method keeps it on the serial lane, and it is
/// never started in these tests: it is what a cancellation finds waiting.
fn queued_business_request() -> aura_ipc::JsonRpcRequest {
    named_business_request(CorrelationId::new(), "workspace.create")
}

fn named_business_request(id: CorrelationId, method: &str) -> aura_ipc::JsonRpcRequest {
    aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id,
        command_invocation_id: None,
        method: method.to_owned(),
        params: serde_json::json!({}),
    }
}

/// A `projection.subscribe`/`projection.unsubscribe` request for one projection.
/// Both routes are read-only in the table but mutate this connection's own
/// subscription set, so their arrival order is observable in the end state.
fn projection_request(method: &str, id: CorrelationId) -> aura_ipc::JsonRpcRequest {
    aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id,
        command_invocation_id: None,
        method: method.to_owned(),
        params: serde_json::json!({
            "projection_kind": "core_health",
            "projection_scope": {"type": "global"},
        }),
    }
}

/// The cancellation Electron Main sends, carrying the invocation id its route
/// declares, so the handler's claim check is the production one.
fn cancellation_request(target: CorrelationId) -> aura_ipc::JsonRpcRequest {
    aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: CorrelationId::new(),
        command_invocation_id: Some(CommandInvocationId::new()),
        method: aura_ipc::method::RUNTIME_CANCEL.to_owned(),
        params: serde_json::json!({ "request_id": target }),
    }
}

/// The read loop's own steps for a side-effecting request: begin its ledger entry,
/// claim its CommandInvocationId, then admit it to a lane. Returns the claim the
/// connection now holds.
async fn reader_admit(
    state: &Arc<crate::RuntimeState>,
    connection: &mut SearchConnection,
    request: &aura_ipc::JsonRpcRequest,
) -> crate::CommandClaim {
    connection.ledger.lock().expect("ledger").begin(request.id).expect("begin");
    let reservation = crate::reserve_command_invocation(state, &connection.queue, request)
        .await
        .expect("the reader's claim is an ordinary step");
    let crate::CommandReservation::Granted(Some(claim)) = reservation else {
        panic!("a fresh invocation id must be granted");
    };
    connection.command_flights.insert(request.id, claim.clone());
    assert_eq!(
        connection.session.admit(request.clone()),
        Admission::Accepted,
        "the lane must have room for this request"
    );
    claim
}

/// Pump the read loop's drain step until `settled` holds, and report whether it
/// did. The handler bodies are real; only the loop that would otherwise run
/// forever inside `serve_connection` is driven by the test.
async fn pump_until(
    state: &Arc<crate::RuntimeState>,
    connection: &mut SearchConnection,
    subscriptions: &Arc<crate::ProjectionSubscriptions>,
    mut settled: impl FnMut() -> bool,
) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    // The same wait channel the real read loop builds, so a handler that enters the common
    // admission reports its wait exactly as production does.
    let (wait_handlers, mut wait_signals) = crate::ipc_session::ResourceWaitSignals::channel();
    while tokio::time::Instant::now() < deadline {
        crate::spawn_ready_handlers_reporting(
            state,
            &connection.queue,
            subscriptions,
            &connection.ledger,
            &connection.command_flights,
            &mut connection.session,
            &wait_handlers,
        )
        .expect("a queued request starts when its lane frees up");
        connection.session.apply_wait_signals(wait_signals.drain());
        let _ = finish_handled_requests(
            state,
            &connection.queue,
            &connection.ledger,
            &mut BTreeSet::new(),
            &mut connection.command_flights,
            connection.session.poll_completions(),
        )
        .expect("handler terminals close cleanly");
        if settled() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    settled()
}

#[tokio::test]
async fn a_blocked_ordered_handler_does_not_hold_back_a_cancellation() {
    // The defect this pins: `aura.cancel` is declared a `CommandInvocation` route,
    // so the lanes used to put it on the ordered lane behind business work. One long
    // write then held every cancellation on the connection until it finished — even
    // a cancellation for a request that write had nothing to do with.
    let fixture = Fixture::new();
    let state = Arc::new(fixture.start().await);
    let mut connection = SearchConnection::new();
    let subscriptions = Arc::new(crate::ProjectionSubscriptions::default());

    // The ordered lane is occupied by a handler parked on an async gate: it holds
    // the lane for as long as this test keeps the gate shut.
    let blocked = queued_business_request();
    connection.ledger.lock().expect("ledger").begin(blocked.id).expect("begin");
    assert_eq!(connection.session.admit(blocked.clone()), Admission::Accepted);
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let parked = Arc::clone(&gate);
    crate::spawn_ready_handlers_with(&mut connection.session, move || {
        let parked = Arc::clone(&parked);
        async move {
            let _permit = parked.acquire().await.expect("the gate stays open");
            terminal_succeeded()
        }
    });
    assert_eq!(connection.session.running_count(), 1, "the ordered lane is occupied");

    // A second request waits behind it and never starts. This is the target.
    let target = queued_business_request();
    connection.ledger.lock().expect("ledger").begin(target.id).expect("begin");
    assert_eq!(connection.session.admit(target.clone()), Admission::Accepted);
    assert_eq!(connection.session.queued_count(), 1, "the target is waiting for its lane");
    assert!(
        !connection.ledger.lock().expect("ledger").is_executing(target.id),
        "the target has not started"
    );

    // The cancellation arrives for that waiting request while the parked handler
    // still holds the ordered lane.
    let cancel = cancellation_request(target.id);
    let _cancel_claim = reader_admit(&state, &mut connection, &cancel).await;
    let ledger = Arc::clone(&connection.ledger);
    let cancel_id = cancel.id;
    assert!(
        pump_until(&state, &mut connection, &subscriptions, || {
            ledger.lock().expect("ledger").terminal(cancel_id).is_some()
        })
        .await,
        "the cancellation must be handled while the ordered handler is still parked"
    );

    // The parked handler is untouched and unfinished: this was a second lane, not a
    // reordering of the ordered one.
    assert_eq!(connection.session.running_count(), 1, "the parked handler still holds its lane");
    assert!(
        ledger.lock().expect("ledger").terminal(blocked.id).is_none(),
        "the blocked handler has not finished either"
    );

    // The reply is the contract's answer, and it only ever means the intent was
    // received: the target is still queued, and it is still in the ledger.
    connection.queue.close();
    let frames = drain(&connection.queue).await;
    let reply = frames
        .iter()
        .find(|frame| frame["id"] == serde_json::to_value(cancel.id).expect("id"))
        .expect("the cancellation owes the peer exactly one answer");
    assert_eq!(reply["result"]["value"], "accepted", "a waiting request is cancellable: {reply}");
    assert!(
        ledger.lock().expect("ledger").cancellation_requested(target.id),
        "the accepted intent is recorded against the target"
    );
    assert_eq!(
        ledger.lock().expect("ledger").terminal(target.id),
        None,
        "an accepted cancellation does not retire the target on its own"
    );

    // Let the parked handler go, so nothing here outlives the test.
    gate.add_permits(1);
    connection.session.shutdown().await;
}

#[test]
fn cancellation_classifies_queued_executing_completed_and_unknown_distinctly() {
    // `cancel()` used to answer from a single `Running` state, so "queued, never
    // started" and "executing right now" were the same fact. They are not: only the
    // second one describes work that may already have had an effect.
    let mut ledger = RequestLedger::new();
    let queued = CorrelationId::new();
    let executing = CorrelationId::new();
    let completed = CorrelationId::new();
    let absent = CorrelationId::new();
    ledger.begin(queued).expect("begin");
    ledger.begin(executing).expect("begin");
    ledger.mark_started(executing).expect("the handler started");
    ledger.begin(completed).expect("begin");
    ledger.mark_started(completed).expect("the handler started");
    ledger.finish(completed, IpcRequestTerminal::Succeeded).expect("one terminal");

    // 1. Queued, never started. 2. Executing. 3. Already completed. 4. No record.
    assert_eq!(ledger.cancel(queued), CancellationOutcome::QueuedNotStarted);
    assert_eq!(ledger.cancel(executing), CancellationOutcome::Executing);
    assert_eq!(ledger.cancel(completed), CancellationOutcome::AlreadyCompleted);
    assert_eq!(ledger.cancel(absent), CancellationOutcome::Unknown);

    // Recorded as well as returned: the two live states stay distinguishable, and
    // neither is confused with a finished request.
    assert!(ledger.cancellation_requested(queued));
    assert!(!ledger.is_executing(queued));
    assert!(ledger.cancellation_requested(executing));
    assert!(ledger.is_executing(executing));
    assert!(!ledger.cancellation_requested(completed), "a finished request has no intent");
    assert_eq!(ledger.terminal(completed), Some(IpcRequestTerminal::Succeeded));
    assert!(!ledger.cancellation_requested(absent));
    assert_eq!(
        ledger.terminal(absent),
        None,
        "no record is recorded as nothing, never as a result"
    );

    // The wire enum cannot carry four states, so the mapping is explicit and the two
    // live states share one answer on purpose.
    assert_eq!(CancellationOutcome::QueuedNotStarted.disposition(), CancelDisposition::Accepted);
    assert_eq!(CancellationOutcome::Executing.disposition(), CancelDisposition::Accepted);
    assert_eq!(
        CancellationOutcome::AlreadyCompleted.disposition(),
        CancelDisposition::TooLateToCancel
    );
    assert_eq!(CancellationOutcome::Unknown.disposition(), CancelDisposition::UnknownRequest);
    assert_ne!(CancellationOutcome::QueuedNotStarted, CancellationOutcome::Executing);

    // An intent recorded while the request waited survives its start, and the next
    // cancellation names the running state.
    let late = CorrelationId::new();
    ledger.begin(late).expect("begin");
    assert_eq!(ledger.cancel(late), CancellationOutcome::QueuedNotStarted);
    ledger.mark_started(late).expect("the handler started");
    assert!(ledger.is_executing(late) && ledger.cancellation_requested(late));
    assert_eq!(ledger.cancel(late), CancellationOutcome::Executing);

    // The request's one terminal is a separate fact from the cancellation. A handler
    // that was already running reaches its own terminal and never answers
    // `Cancelled`, so the intent is recorded as acknowledged, not enforced.
    assert_eq!(
        ledger.finish(executing, IpcRequestTerminal::Succeeded).expect("one terminal"),
        CancellationSettlement::NotHonoured
    );
    assert_eq!(
        ledger.cancellation_settlement(executing),
        Some(CancellationSettlement::NotHonoured)
    );
    assert_eq!(
        ledger.terminal(executing),
        Some(IpcRequestTerminal::Succeeded),
        "the business terminal is not rewritten as cancelled"
    );
    // Queued work that never started was not ignored by a handler: there was none.
    assert_eq!(
        ledger.finish(queued, IpcRequestTerminal::TransportLost).expect("one terminal"),
        CancellationSettlement::NeverStarted
    );
    // A cooperative handler is its own state.
    let cooperative = CorrelationId::new();
    ledger.begin(cooperative).expect("begin");
    ledger.mark_started(cooperative).expect("the handler started");
    assert_eq!(ledger.cancel(cooperative), CancellationOutcome::Executing);
    assert_eq!(
        ledger.finish(cooperative, IpcRequestTerminal::Cancelled).expect("one terminal"),
        CancellationSettlement::Honoured
    );
    // And a request nobody cancelled has no cancellation fact at all.
    let plain = CorrelationId::new();
    ledger.begin(plain).expect("begin");
    assert_eq!(
        ledger.finish(plain, IpcRequestTerminal::Succeeded).expect("one terminal"),
        CancellationSettlement::NotRequested
    );
    assert_eq!(ledger.cancellation_settlement(plain), Some(CancellationSettlement::NotRequested));

    // One start and one terminal per request, and neither can be invented for a
    // request the ledger never saw.
    assert_eq!(ledger.mark_started(absent), Err(LedgerError::UnknownRequest));
    assert_eq!(
        ledger.finish(completed, IpcRequestTerminal::Failed),
        Err(LedgerError::AlreadyTerminal)
    );
}

#[tokio::test]
async fn same_resource_ordering_is_preserved_and_unknown_methods_stay_ordered() {
    let fixture = Fixture::new();
    let state = Arc::new(fixture.start().await);
    let subscriptions = Arc::new(crate::ProjectionSubscriptions::default());
    let health = crate::current_core_health_projection(&state);

    // Both routes are read-only in the table, so an idempotency- or name-based
    // classification would run them beside each other and let them race. They
    // mutate the *same* connection-local resource, so their arrival order has to be
    // their execution order.
    for method in ["projection.subscribe", "projection.unsubscribe"] {
        let request = projection_request(method, CorrelationId::new());
        assert_eq!(
            crate::ipc_session::lane_for(&request),
            Lane::Serial,
            "{method} must keep arrival order"
        );
    }
    // The route table is the only source of independence: a method it does not know
    // is not guessed to be safe beside anything.
    let unknown = named_business_request(CorrelationId::new(), "mystery.method");
    assert!(
        aura_ipc::routes::core_route("mystery.method").is_none(),
        "this method really is unrecognised"
    );
    assert_eq!(crate::ipc_session::lane_for(&unknown), Lane::Serial);

    // Both arrival orders, on one connection: whichever ran last decides the state,
    // so a reversed result would prove the order was not preserved.
    let mut connection = SearchConnection::new();
    let mut arrival = Vec::new();
    for (expected, methods) in [
        (false, ["projection.subscribe", "projection.unsubscribe"]),
        (true, ["projection.unsubscribe", "projection.subscribe"]),
    ] {
        let requests: Vec<aura_ipc::JsonRpcRequest> =
            methods.iter().map(|method| projection_request(method, CorrelationId::new())).collect();
        for request in &requests {
            connection.ledger.lock().expect("ledger").begin(request.id).expect("begin");
            assert_eq!(connection.session.admit(request.clone()), Admission::Accepted);
            arrival.push(request.id);
        }
        crate::spawn_ready_handlers(
            &state,
            &connection.queue,
            &subscriptions,
            &connection.ledger,
            &connection.command_flights,
            &mut connection.session,
        )
        .expect("the first request starts on its lane");
        assert_eq!(connection.session.running_count(), 1, "one ordered handler at a time");
        assert_eq!(connection.session.queued_count(), 1, "the second waits its turn");

        let ledger = Arc::clone(&connection.ledger);
        let last = requests[1].id;
        assert!(
            pump_until(&state, &mut connection, &subscriptions, || {
                ledger.lock().expect("ledger").terminal(last).is_some()
            })
            .await,
            "both ordered requests must run"
        );
        assert_eq!(
            subscriptions.contains(&health),
            expected,
            "{methods:?} must run in arrival order"
        );
        for request in &requests {
            assert_eq!(
                connection.ledger.lock().expect("ledger").terminal(request.id),
                Some(IpcRequestTerminal::Succeeded),
                "each request closes its own terminal"
            );
        }
    }

    // And the replies follow that same order, which is what the peer observes.
    connection.queue.close();
    let frames = drain(&connection.queue).await;
    let answered: Vec<CorrelationId> = frames
        .iter()
        .filter_map(|frame| serde_json::from_value(frame["id"].clone()).ok())
        .collect();
    assert_eq!(answered, arrival, "the replies must follow the arrival order: {frames:?}");
}

#[tokio::test]
async fn an_overloaded_lane_answers_locally_and_still_serves_a_cancellation() {
    let fixture = Fixture::new();
    let state = Arc::new(fixture.start().await);
    let mut connection = SearchConnection::new();
    let subscriptions = Arc::new(crate::ProjectionSubscriptions::default());

    // A parked handler holds the ordered lane, and one waiting request is the
    // cancellation target.
    let blocked = queued_business_request();
    connection.ledger.lock().expect("ledger").begin(blocked.id).expect("begin");
    assert_eq!(connection.session.admit(blocked.clone()), Admission::Accepted);
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let parked = Arc::clone(&gate);
    crate::spawn_ready_handlers_with(&mut connection.session, move || {
        let parked = Arc::clone(&parked);
        async move {
            let _permit = parked.acquire().await.expect("the gate stays open");
            terminal_succeeded()
        }
    });
    let target = queued_business_request();
    connection.ledger.lock().expect("ledger").begin(target.id).expect("begin");
    assert_eq!(connection.session.admit(target.clone()), Admission::Accepted);

    // Fill the ordered queue to its bound, then overflow it.
    for _ in 1..crate::ipc_session::MAX_QUEUED_SERIAL {
        let waiting = queued_business_request();
        connection.ledger.lock().expect("ledger").begin(waiting.id).expect("begin");
        assert_eq!(
            connection.session.admit(waiting),
            Admission::Accepted,
            "the ordered queue has room up to its bound"
        );
    }
    let refused = queued_business_request();
    connection.ledger.lock().expect("ledger").begin(refused.id).expect("begin");
    assert_eq!(
        connection.session.admit(refused.clone()),
        Admission::Overloaded { lane: Lane::Serial },
        "an ordered burst is refused, never queued without bound"
    );

    // The refusal is answered by the reader itself, with a local typed error and
    // without waiting for the lane, the handler or queue space.
    let error = Admission::overloaded_error(refused.id, Lane::Serial);
    assert_eq!(error.code.as_str(), "ipc.handler_overloaded");
    assert_eq!(error.category, ErrorCategory::Unavailable);
    assert_eq!(error.terminal, IpcRequestTerminal::Failed);
    assert!(error.retryable, "the caller can retry once the current work finishes");
    try_send_json(
        &connection.queue,
        QueuePriority::P0,
        &aura_ipc::JsonRpcResponse::failure(refused.id, error),
    )
    .expect("the reader answers a refused request without waiting");
    let frame = control_payload(connection.queue.recv().await.expect("the refusal is queued"));
    assert_eq!(frame["id"], serde_json::to_value(refused.id).expect("id"));
    assert_eq!(
        frame["error"]["data"]["code"], "ipc.handler_overloaded",
        "one local typed overload: {frame}"
    );

    // The reader is not blocked and no lane leaked: the cancellation is admitted on
    // the control lane while the ordered lane is saturated and parked.
    let cancel = cancellation_request(target.id);
    let _cancel_claim = reader_admit(&state, &mut connection, &cancel).await;
    let ledger = Arc::clone(&connection.ledger);
    let cancel_id = cancel.id;
    assert!(
        pump_until(&state, &mut connection, &subscriptions, || {
            ledger.lock().expect("ledger").terminal(cancel_id).is_some()
        })
        .await,
        "an overflowed ordered lane must not stop the control lane"
    );
    assert_eq!(
        ledger.lock().expect("ledger").terminal(cancel_id),
        Some(IpcRequestTerminal::Succeeded)
    );
    assert_eq!(
        connection.session.running_count(),
        1,
        "the ordered lane still holds exactly its one parked handler"
    );

    gate.add_permits(1);
    connection.session.shutdown().await;
}

#[test]
fn a_disconnect_never_releases_a_started_commands_claim() {
    // The claim ledger is process-global and the parked work keeps a global owner,
    // so this test does not run beside the other teardown tests.
    let _serial = flight_lock();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(disconnect_keeps_a_started_commands_claim());
}

async fn disconnect_keeps_a_started_commands_claim() {
    let fixture = Fixture::new();
    let state = Arc::new(fixture.start().await);
    let mut connection = SearchConnection::new();

    let invocation = CommandInvocationId::new();
    let request = aura_ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: CorrelationId::new(),
        command_invocation_id: Some(invocation),
        method: "workspace.create".to_owned(),
        params: serde_json::json!({}),
    };
    let claim = reader_admit(&state, &mut connection, &request).await;
    assert!(state.command_outcomes.lock().expect("outcomes").owns(&claim));

    // The handler is parked inside a real blocking call, so `abort()` cannot end it
    // and the teardown has to hand the work over instead of reporting it stopped.
    let gate: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)> =
        Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let handler_gate = Arc::clone(&gate);
    crate::spawn_ready_handlers_with(&mut connection.session, move || {
        let handler_gate = Arc::clone(&handler_gate);
        async move {
            tokio::task::block_in_place(move || {
                let (lock, signal) = &*handler_gate;
                let mut released = lock.lock().expect("work gate");
                while !*released {
                    let Ok(next) = signal.wait_timeout(released, Duration::from_secs(30)) else {
                        return;
                    };
                    released = next.0;
                    if next.1.timed_out() {
                        return;
                    }
                }
            });
            terminal_succeeded()
        }
    });
    assert_eq!(connection.session.running_count(), 1, "the handler took its lane");

    // The waiter goes away while the work is still running.
    let reclaim = connection.disconnect(&state).await;
    assert!(reclaim.started.contains(&request.id), "the handler really took a lane");
    assert!(
        crate::detached_handlers::live() >= 1,
        "work that could not stop keeps an owner instead of being dropped"
    );

    // `serve_connection` settles this connection's claims immediately after the
    // reclaim: a request that started is `Ran`, which records the effect as unknown
    // rather than releasing the id.
    for (request_id, claim) in std::mem::take(&mut connection.command_flights) {
        let settlement = if reclaim.started.contains(&request_id) {
            CommandSettlement::Ran
        } else {
            CommandSettlement::NeverRan
        };
        settle_command_claim(&state, &claim, settlement);
    }

    // The claim is gone but the id is not free: the effect is undetermined, and the
    // record is what a replay conflicts with.
    {
        let mut outcomes = state.command_outcomes.lock().expect("outcomes");
        assert!(!outcomes.is_in_flight(invocation), "the attempt itself is over");
        assert!(outcomes.recorded_outcome_is_unknown(invocation));
        assert_eq!(
            outcomes.query(invocation),
            CommandOutcome::Unknown {
                command_invocation_id: invocation,
                reason: CommandOutcomeUnknown::NotRecordedOrIncomplete,
            },
            "a missing record reads as unknown, never as 'did not execute'"
        );
        assert!(
            outcomes.begin(invocation, CorrelationId::new()).is_none(),
            "a replay must conflict instead of executing the same command twice"
        );
    }

    // A Core that never had the record answers identically: absence of a record is
    // not evidence that nothing ran.
    let restarted = crate::command_claims::CommandOutcomeLedger::default();
    assert_eq!(
        restarted.query(invocation),
        CommandOutcome::Unknown {
            command_invocation_id: invocation,
            reason: CommandOutcomeUnknown::NotRecordedOrIncomplete,
        }
    );

    // The handed-over work really ends, and only then is the owner released.
    {
        let (lock, signal) = &*gate;
        let mut released = lock.lock().expect("work gate");
        *released = true;
        signal.notify_all();
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while crate::detached_handlers::live() != 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        crate::detached_handlers::live(),
        0,
        "the owner is released once the work really finishes"
    );
}
