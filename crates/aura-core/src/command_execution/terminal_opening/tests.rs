use super::*;
use aura_core_base::ipc_session;
use aura_domain_common::SessionId;

fn progress(
    runtime: RuntimeInstanceId,
    request: CorrelationId,
    session: SessionId,
) -> TerminalOpeningProgressV1 {
    TerminalOpeningProgressV1 {
        contract_version: ToolWireVersionV1,
        runtime_instance_id: runtime,
        request_id: request,
        session_id: session,
        phase: Phase::WaitingLaunch,
    }
}

#[tokio::test]
async fn runtime_completion_reports_waiting_launch_before_the_actual_launch_gate() {
    let registry = OpeningRegistry::default();
    let queue = AsyncPriorityQueue::new();
    let guard = registry
        .begin(
            TerminalOpeningProgressV1 {
                phase: Phase::WaitingRuntime,
                ..progress(RuntimeInstanceId::new(), CorrelationId::new(), SessionId::new())
            },
            queue.clone(),
            false,
            Weak::new(),
        )
        .unwrap();
    assert!(guard.0.before_launch().is_err(), "an unverified runtime cannot launch");
    guard.0.runtime_ready().unwrap();
    guard.0.runtime_ready().unwrap(); // Already-ready runtimes do not duplicate progress.
    guard.0.before_launch().unwrap();
    assert!(guard.0.runtime_ready().is_err(), "the launch gate is irreversible");
    for phase in [Phase::WaitingRuntime, Phase::WaitingLaunch, Phase::Launching] {
        let WireFrame::Control(bytes) = queue.recv().await.unwrap().frame else {
            panic!("control")
        };
        let event: JsonRpcNotification = serde_json::from_slice(&bytes).unwrap();
        let value: TerminalOpeningProgressV1 = serde_json::from_value(event.params).unwrap();
        assert_eq!(value.phase, phase);
    }
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), queue.recv()).await.is_err(),
        "duplicate readiness must not emit another state"
    );
}

#[test]
fn cancellation_wins_in_both_waiting_phases_and_runtime_completion_cannot_revive_it() {
    for phase in [Phase::WaitingRuntime, Phase::WaitingLaunch] {
        let registry = OpeningRegistry::default();
        let source = TerminalOpeningProgressV1 {
            phase,
            ..progress(RuntimeInstanceId::new(), CorrelationId::new(), SessionId::new())
        };
        let guard = registry.begin(source, AsyncPriorityQueue::new(), false, Weak::new()).unwrap();
        assert_eq!(guard.0.cancel().unwrap(), CancelDisposition::Accepted);
        assert!(guard.0.runtime_ready().is_err());
        assert!(guard.0.before_launch().is_err());
        assert!(guard.0.is_cancelled());
    }
}

#[test]
fn cancellation_and_launch_have_exactly_one_winner_and_keep_event_identity() {
    for _ in 0..24 {
        let registry = Arc::new(OpeningRegistry::default());
        let runtime = RuntimeInstanceId::new();
        let request = CorrelationId::new();
        let session = SessionId::new();
        let queue = AsyncPriorityQueue::new();
        let connection = Arc::new(Mutex::new(RequestLedger::new()));
        let guard = registry
            .begin(
                progress(runtime, request, session),
                queue.clone(),
                false,
                Arc::downgrade(&connection),
            )
            .unwrap();
        let launch = guard.0.clone();
        let started = std::thread::spawn(move || launch.before_launch().is_ok());
        let canceller = registry.clone();
        let cancel_connection = connection.clone();
        let disposition = std::thread::spawn(move || {
            canceller.cancel(request, false, &cancel_connection).unwrap().unwrap()
        });
        let started = started.join().unwrap();
        let disposition = disposition.join().unwrap();
        assert_eq!(started, disposition == CancelDisposition::TooLateToCancel);
        assert_eq!(guard.0.is_cancelled(), !started);
        let notification = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(queue.recv())
            .unwrap();
        let WireFrame::Control(bytes) = notification.frame else { panic!("control") };
        let event: JsonRpcNotification = serde_json::from_slice(&bytes).unwrap();
        let progress: TerminalOpeningProgressV1 = serde_json::from_value(event.params).unwrap();
        assert_eq!(event.method, "terminal.opening.v1");
        assert_eq!(
            (progress.runtime_instance_id, progress.request_id, progress.session_id),
            (runtime, request, session)
        );
        assert_eq!(progress.phase, Phase::WaitingLaunch);
        assert_eq!(registry.cancel(CorrelationId::new(), false, &connection).unwrap(), None);
        registry.finish(request).unwrap();
        assert_eq!(registry.cancel(request, false, &connection).unwrap(), None);
    }
}

#[test]
fn an_intent_received_before_registration_cannot_pass_the_launch_gate() {
    let registry = OpeningRegistry::default();
    let request = CorrelationId::new();
    let mut ledger = RequestLedger::new();
    ledger.begin(request).unwrap();
    ledger.cancel(request);
    ledger.mark_started(request).unwrap();
    let guard = registry
        .begin(
            progress(RuntimeInstanceId::new(), request, SessionId::new()),
            AsyncPriorityQueue::new(),
            ledger.cancellation_requested(request),
            Weak::new(),
        )
        .unwrap();
    assert!(guard.0.is_cancelled());
    assert!(guard.0.before_launch().is_err());
}

#[tokio::test]
async fn launching_user_open_returns_too_late_without_recording_cancel_intent() {
    let host = crate::r03_tests::Fixture::new();
    let state = host.start().await;
    let target = CorrelationId::new();
    let ledger = Arc::new(Mutex::new(RequestLedger::new()));
    ledger.lock().unwrap().begin(target).unwrap();
    ledger.lock().unwrap().mark_started(target).unwrap();
    let guard = state
        .terminal_openings
        .begin(
            progress(state.options.runtime_instance_id, target, SessionId::new()),
            AsyncPriorityQueue::new(),
            false,
            Arc::downgrade(&ledger),
        )
        .unwrap();
    guard.0.before_launch().unwrap();
    let request = JsonRpcRequest {
        jsonrpc: crate::JSON_RPC_VERSION.into(),
        id: CorrelationId::new(),
        command_invocation_id: Some(aura_domain_common::CommandInvocationId::new()),
        method: "aura.cancel".into(),
        params: serde_json::json!({"request_id": target}),
    };
    let queue = AsyncPriorityQueue::new();
    let claim = match crate::reserve_command_invocation(&state, &queue, &request).await.unwrap() {
        crate::CommandReservation::Granted(claim) => claim,
        crate::CommandReservation::Refused => panic!("own cancel invocation"),
    };
    crate::handle_request(
        &state,
        &queue,
        &crate::ProjectionSubscriptions::default(),
        &ledger,
        &request,
        claim.as_ref(),
    )
    .await
    .unwrap();
    let WireFrame::Control(bytes) = queue.recv().await.unwrap().frame else { panic!("control") };
    let response: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(response["result"]["value"], "too_late_to_cancel", "{response}");
    assert!(!ledger.lock().unwrap().cancellation_requested(target));
}

#[tokio::test]
async fn a_foreign_connection_cannot_cancel_another_connections_opening() {
    let host = crate::r03_tests::Fixture::new();
    let state = host.start().await;
    let target = CorrelationId::new();
    let owner = Arc::new(Mutex::new(RequestLedger::new()));
    owner.lock().unwrap().begin(target).unwrap();
    owner.lock().unwrap().mark_started(target).unwrap();
    let guard = state
        .terminal_openings
        .begin(
            progress(state.options.runtime_instance_id, target, SessionId::new()),
            AsyncPriorityQueue::new(),
            false,
            Arc::downgrade(&owner),
        )
        .unwrap();
    let foreign = Arc::new(Mutex::new(RequestLedger::new()));
    for (connection, expected) in [(&foreign, "unknown_request"), (&owner, "accepted")] {
        let request = JsonRpcRequest {
            jsonrpc: crate::JSON_RPC_VERSION.into(),
            id: CorrelationId::new(),
            command_invocation_id: Some(aura_domain_common::CommandInvocationId::new()),
            method: "aura.cancel".into(),
            params: serde_json::json!({"request_id":target}),
        };
        let queue = AsyncPriorityQueue::new();
        let claim = match crate::reserve_command_invocation(&state, &queue, &request).await.unwrap()
        {
            crate::CommandReservation::Granted(claim) => claim,
            crate::CommandReservation::Refused => panic!("own cancel invocation"),
        };
        let terminal = crate::handle_request(
            &state,
            &queue,
            &crate::ProjectionSubscriptions::default(),
            connection,
            &request,
            claim.as_ref(),
        )
        .await
        .unwrap();
        let WireFrame::Control(bytes) = queue.recv().await.unwrap().frame else {
            panic!("control")
        };
        let response: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(terminal, aura_runtime_contract::IpcRequestTerminal::Succeeded, "{response}");
        assert_eq!(response["result"]["value"], expected, "{response}");
        assert_eq!(guard.0.is_cancelled(), expected == "accepted");
        assert_eq!(owner.lock().unwrap().cancellation_requested(target), expected == "accepted");
        assert!(!foreign.lock().unwrap().cancellation_requested(target));
    }
}

#[tokio::test]
async fn disconnect_reclaims_only_its_openings_and_reconnect_reuses_capacity() {
    let host = crate::r03_tests::Fixture::new();
    let state = Arc::new(host.start().await);
    let registry = &state.terminal_openings;
    let other = Arc::new(Mutex::new(RequestLedger::new()));
    let still_waiting = registry
        .begin(
            progress(RuntimeInstanceId::new(), CorrelationId::new(), SessionId::new()),
            AsyncPriorityQueue::new(),
            false,
            Arc::downgrade(&other),
        )
        .unwrap();
    for _ in 0..140 {
        let first = Arc::new(Mutex::new(RequestLedger::new()));
        let preparing = registry
            .begin(
                progress(RuntimeInstanceId::new(), CorrelationId::new(), SessionId::new()),
                AsyncPriorityQueue::new(),
                false,
                Arc::downgrade(&first),
            )
            .unwrap();
        let launching = registry
            .begin(
                progress(RuntimeInstanceId::new(), CorrelationId::new(), SessionId::new()),
                AsyncPriorityQueue::new(),
                false,
                Arc::downgrade(&first),
            )
            .unwrap();
        launching.0.before_launch().unwrap();
        let mut lanes = ipc_session::IpcSession::new();
        crate::reclaim_connection_work(
            &state,
            &AsyncPriorityQueue::new(),
            &first,
            &mut lanes,
            &mut Default::default(),
            &mut Default::default(),
        )
        .await;
        assert_eq!(registry.cancel(preparing.0.progress.request_id, false, &first).unwrap(), None);
        assert_eq!(registry.cancel(launching.0.progress.request_id, false, &first).unwrap(), None);
        assert!(preparing.0.is_cancelled());
        assert!(
            preparing.0.before_launch().is_err(),
            "a queued closure cannot launch after disconnect"
        );
        assert!(!launching.0.is_cancelled(), "already-started work keeps its native owner");
        assert!(!still_waiting.0.is_cancelled(), "another connection stays independent");
    }
    assert_eq!(registry.reclaim(&other).unwrap(), 1);
}
