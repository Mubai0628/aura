//! S04-A counterexample probes: the three key doubts of the brief, driven against
//! the **pre-fix** behaviour of the current tree.
//!
//! These are the probes whose *first* run is the recorded counterexample. They assert the
//! defect, so after the fix they must FAIL: an `#[ignore]`d probe that keeps asserting the
//! old behaviour is the honest form - it is the pre-fix harness, and the fixed behaviour is
//! pinned by the real regressions in `supervision.rs` and `store_health.rs`. Nothing in the
//! product compiles this module.
//!
//! PROBE 1 (panic residue) uses one injected panic placed **inside the real spawned run task**
//! that `AgentApplication::launch` creates. The panic source is injected; everything the panic
//! then passes through - the teardown that does not run, the `active` entry that stays, the
//! `stopped` signal that never fires - is the unmodified production path.
//!
//! PROBE 2 (one run's record failure poisons every session) is fully real: a real run of a real
//! Provider fixture, a real first-event timeout, and a real second SQLite connection holding the
//! V2 write lock so that the run's own failure record really fails. The fault text the probe
//! reads back is the text of that real SQLite error, which is what makes it evidence of the
//! source chain rather than of a manual assignment.
//!
//! PROBE 3 (a real journal wait occupies the reactor) holds the real `BrokerJournal` mutex - the
//! one every agent journal step takes - from a plain thread while the single reactor worker of
//! its runtime is inside the production `agent.status` entry.
use super::*;
use crate::{AsyncPriorityQueue, WireFrame};
use provider_fixture::{Provider, Scenario};
use rusqlite::{Connection, OpenFlags};
use std::time::{Duration, Instant};

/// PROBE 1: pre-fix, a panic in the real run task leaves the run in `active` forever and never
/// fires `stopped`.
#[tokio::test]
#[ignore = "pre-fix counterexample probe: asserts the defect, so it must fail after the S04 fix"]
async fn prefix_a_real_run_panic_leaves_an_active_entry_and_never_signals_stopped() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.agent_required().probe_arm_drive_panic();
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let run = started.run.agent_run_id;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let residue = services.agent_required().probe_active_runs();
    let stopped =
        services.agent_required().probe_await_stopped(run, Duration::from_millis(600)).await;
    println!("PROBE1 run={run} active_after_panic={residue:?} stopped_signalled={stopped}");
    assert!(
        residue.contains(&run),
        "pre-fix: the panicked run's active entry is never removed: {residue:?}"
    );
    assert!(!stopped, "pre-fix: a panicked run never signals stopped");
}

/// PROBE 2: pre-fix, one run's failed failure-record poisons **another** session's `status()`
/// with an opaque string that never clears - not even after the store is healthy again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "pre-fix counterexample probe: asserts the defect, so it must fail after the S04 fix"]
async fn prefix_one_runs_store_failure_poisons_another_sessions_status_permanently() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let root = host.root.path().join("s04-probe-workspace");
    std::fs::create_dir(&root).expect("probe workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"S04 probe"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace summary");

    // B: a session whose own run parks in AwaitingQuery and needs nothing from the writer.
    let provider_b = Provider::scenario(Scenario::Query).await;
    let model_b = profile_after_existing(&runtime, &provider_b.url).await;
    let session_b =
        create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let started_b = waiting::start(&runtime, session_b, model_b, AgentTaskMode::Normal).await;
    let request_b =
        AgentRunRequest { session_id: session_b, agent_run_id: started_b.run.agent_run_id };
    waiting::wait(&runtime, &request_b, |p| {
        matches!(p.turn.state, TurnTaskState::AwaitingQuery { .. })
    })
    .await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let before = services.agent_required().status(&request_b).await;
    assert!(before.is_ok(), "B's own status must be readable before anything fails");

    // A: a session whose run really fails, with the V2 write lock held from a second real
    // connection so that the *record* of that failure really fails too.
    let provider_a = Provider::scenario(Scenario::Hold).await;
    let model_a = profile_after_existing(&runtime, &provider_a.url).await;
    let session_a =
        create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let started_a = waiting::start(&runtime, session_a, model_a, AgentTaskMode::Normal).await;
    let request_a =
        AgentRunRequest { session_id: session_a, agent_run_id: started_a.run.agent_run_id };
    let parked = tokio::time::timeout(Duration::from_secs(30), async {
        while provider_a.requests.lock().expect("fixture requests").is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(parked.is_ok(), "A's real provider request must be parked before the lock is taken");
    let database = host.paths.data_dir.join("v2-execution").join("aura-v2.db");
    let holder = Connection::open_with_flags(
        &database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .expect("second real connection to the V2 store");
    holder.busy_timeout(Duration::from_millis(0)).expect("holder has no wait budget");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match holder.execute_batch("BEGIN IMMEDIATE") {
            Ok(()) => break,
            Err(error) => {
                assert!(Instant::now() < deadline, "the holder never took the write lock: {error}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    println!("PROBE2 write lock held by a second real connection at {:?}", Instant::now());

    let mut faulted_at = None;
    let started_waiting = Instant::now();
    while started_waiting.elapsed() < Duration::from_secs(45) {
        if let Err(error) = services.agent_required().status(&request_b).await {
            faulted_at = Some((started_waiting.elapsed(), format!("{error:#}")));
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (elapsed, text) = faulted_at.expect("A's real failure must poison B's status");
    println!("PROBE2 B_status_refused_after={elapsed:?} text={text}");
    assert!(
        text.contains("agent.persistence_unavailable"),
        "the global fault string is used: {text}"
    );
    assert!(
        text.contains("SQLite") || text.contains("locked"),
        "the fault must carry the real SQLite error of the failed record: {text}"
    );
    assert!(
        services.agent_required().status(&request_a).await.is_err(),
        "A's own status is refused as well"
    );

    // Release the store: the shared dependency is healthy again.
    holder.execute_batch("COMMIT").expect("release the write lock");
    drop(holder);
    tokio::time::sleep(Duration::from_secs(7)).await;
    let after_recovery = services.agent_required().status(&request_b).await;
    println!("PROBE2 B_status_after_store_recovered={after_recovery:?}");
    assert!(
        after_recovery.is_err(),
        "pre-fix: the historical fault string outlives the fault it named, with no recovery entry"
    );
}

/// PROBE 3: pre-fix, a real agent journal wait parks the only reactor worker, so the production
/// heartbeat cannot be delivered while it waits.
///
/// The run is parked in `AwaitingQuery` first and its own task really ends, so the only journal
/// wait left when the heartbeat window opens is the production `agent.status` entry: the
/// heartbeat's absence is attributed to that call and not to a background run.
#[test]
#[ignore = "pre-fix counterexample probe: asserts the defect, so it must fail after the S04 fix"]
fn prefix_a_real_agent_journal_wait_occupies_the_only_reactor_worker() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("one-worker runtime");
    let (host, state, started) = runtime.block_on(async {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(Scenario::Query).await;
        let (runtime_state, session, model) = waiting::prepare(&host, &provider).await;
        let started = waiting::start(&runtime_state, session, model, AgentTaskMode::Normal).await;
        let request = AgentRunRequest {
            session_id: started.turn.session_id,
            agent_run_id: started.run.agent_run_id,
        };
        waiting::wait(&runtime_state, &request, |p| {
            matches!(p.turn.state, TurnTaskState::AwaitingQuery { .. })
        })
        .await;
        if let crate::StartupMode::Ready(services) = &runtime_state.startup_mode {
            services
                .agent_required()
                .idle(request.agent_run_id)
                .await
                .expect("the run's own task ended");
        }
        (host, runtime_state, started)
    });
    let state = Arc::new(state);
    let request = AgentRunRequest {
        session_id: started.turn.session_id,
        agent_run_id: started.run.agent_run_id,
    };
    let crate::StartupMode::Ready(services) = &state.startup_mode else { panic!("Core ready") };
    assert!(
        services.agent_required().probe_active_runs().is_empty(),
        "no run is left active before the probe"
    );
    // The real journal mutex, held by a plain thread: this is the wait every agent journal step
    // takes when another step is inside the store.
    let (release, held) = std::sync::mpsc::channel::<()>();
    let broker = Arc::clone(&services.broker);
    let holder = std::thread::spawn(move || {
        let _guard = broker.lock().expect("the real journal mutex");
        let _ = held.recv_timeout(Duration::from_secs(30));
    });
    std::thread::sleep(Duration::from_millis(150));
    // The production status entry reaches its journal read and parks the only worker.
    let status_agent = services.agent_required();
    let status_request = request.clone();
    let status = runtime.spawn(async move { status_agent.status(&status_request).await });
    println!(
        "PROBE3 reader_absent={:?} counters_before={:?}",
        match &state.startup_mode {
            crate::StartupMode::Ready(services) =>
                services.agent_required().store().reader_absent(),
            _ => None,
        },
        match &state.startup_mode {
            crate::StartupMode::Ready(services) => services.agent_required().store().counters(),
            _ => Default::default(),
        }
    );
    std::thread::sleep(Duration::from_millis(300));
    // The production heartbeat, on the same one-worker runtime.
    let queue = AsyncPriorityQueue::new();
    let delivered = Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
    let recorder = Arc::clone(&delivered);
    let reader_queue = queue.clone();
    let started_at = Instant::now();
    let reader = runtime.spawn(async move {
        while let Ok(frame) = reader_queue.recv().await {
            let WireFrame::Control(bytes) = frame.frame else { continue };
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { continue };
            if value.get("method").and_then(Value::as_str)
                == Some(aura_ipc::method::RUNTIME_HEARTBEAT)
            {
                recorder.store(
                    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
                    std::sync::atomic::Ordering::Release,
                );
                return;
            }
        }
    });
    let heartbeat_state = Arc::clone(&state);
    let _heartbeat = runtime.spawn(async move {
        let _ = crate::spawn_heartbeat(heartbeat_state, queue).await;
    });
    // A plain timer task on the same one-worker runtime: if even this cannot run, the worker
    // itself is parked and the heartbeat's absence is not about the heartbeat path.
    let ticked = Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
    let ticked_recorder = Arc::clone(&ticked);
    let ticked_started = Instant::now();
    let _ticker = runtime.spawn(async move {
        tokio::time::sleep(Duration::from_millis(1_000)).await;
        ticked_recorder.store(
            u64::try_from(ticked_started.elapsed().as_millis()).unwrap_or(u64::MAX),
            std::sync::atomic::Ordering::Release,
        );
    });
    // The production status entry, on the same worker.
    let window = Duration::from_millis(9_000);
    let until = Instant::now() + window;
    while Instant::now() < until && delivered.load(std::sync::atomic::Ordering::Acquire) == u64::MAX
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let heartbeat_ms = delivered.load(std::sync::atomic::Ordering::Acquire);
    println!(
        "PROBE3 run={} heartbeat_ms_while_journal_held={heartbeat_ms} window_ms={} \
         plain_timer_ticked_ms={}",
        request.agent_run_id,
        window.as_millis(),
        ticked.load(std::sync::atomic::Ordering::Acquire)
    );
    let _ = release.send(());
    let _ = holder.join();
    let answered = runtime.block_on(status).expect("status task");
    let _ = runtime.block_on(reader);
    println!("PROBE3 status_answered={answered:?} heartbeat_ms={heartbeat_ms}");
    drop(host);
    assert_eq!(
        heartbeat_ms,
        u64::MAX,
        "pre-fix: the heartbeat cannot be delivered while a real journal wait parks the worker"
    );
}
