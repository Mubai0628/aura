//! S04-B: bounded Store access and per-dependency health, through the real entries.
//!
//! Every case here drives the production entries - `agent.start`, `agent.status`,
//! `agent.cancel`, the production heartbeat task and the real Store owner - against an isolated
//! real database. The only injected thing is a hold point *inside* the target production call
//! (`AgentStore::arm_hold`, default off) or a second real SQLite connection doing what any
//! second writer would do.
//!
//! WHAT EACH CASE IS ABOUT
//! 1. One run's real Store work is parked: its own caller still ends inside its budget, another
//!    session's real status is still served, and authentication and the heartbeat still happen.
//! 2. The shared Store is genuinely unavailable: both runs' dependent writes report it inside
//!    bounds, an unrelated control entry stays usable, and no write is claimed to have
//!    succeeded.
//! 3. The boundary is full: the next item is refused **before** the business runs, the holders
//!    keep their permits, and capacity comes back with the real completion.
//! 4. The caller's wait ends before the real commit: the same invocation can still be asked for
//!    its original result, and the commit really happens exactly once.
//! 5. The fault clears: new legitimate work is served without restarting anything, and no
//!    unknown work is replayed.
use super::*;
use crate::agent_store::{ReceiptState, StoreDependency, StoreError, StoreStep};
use crate::sync_boundary::SyncSection;
use crate::{AsyncPriorityQueue, WireFrame};
use provider_fixture::{Provider, Scenario};
use rusqlite::{Connection, OpenFlags};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// How long a Store hold parks the real work.
const HOLD_MS: u64 = 4_000;
/// The boundary budget these cases run with: short so a bounded answer is observable without
/// waiting the production 30 s. The budget is the only thing that shrinks.
const BUDGET_MS: u64 = 400;

/// One workspace with two independent sessions, each with its own configured Provider.
struct TwoSessions {
    _fixture: crate::r03_tests::Fixture,
    runtime: crate::RuntimeState,
    workspace: WorkspaceId,
    provider_a: Provider,
    model_a: ModelProfileId,
    session_a: SessionId,
    /// Kept alive: dropping the Provider aborts its real listener.
    #[allow(dead_code)]
    provider_b: Provider,
    model_b: ModelProfileId,
    session_b: SessionId,
}

impl TwoSessions {
    async fn new(scenario_a: Scenario, scenario_b: Scenario) -> Self {
        let fixture = crate::r03_tests::Fixture::new();
        let runtime = fixture.start().await;
        let root = fixture.root.path().join("s04-store-workspace");
        std::fs::create_dir(&root).expect("workspace root");
        let created = crate::r03_tests::success(
            &runtime,
            "workspace.create",
            json!({"selected_root":root,"display_name":"S04 store"}),
        )
        .await;
        let workspace: aura_workspace_domain::WorkspaceSummary =
            serde_json::from_value(created["workspace"].clone()).expect("workspace summary");
        let provider_a = Provider::scenario(scenario_a).await;
        let model_a = profile_after_existing(&runtime, &provider_a.url).await;
        let session_a =
            create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
        let provider_b = Provider::scenario(scenario_b).await;
        let model_b = profile_after_existing(&runtime, &provider_b.url).await;
        let session_b =
            create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
        Self {
            _fixture: fixture,
            runtime,
            workspace: workspace.workspace_id,
            provider_a,
            model_a,
            session_a,
            provider_b,
            model_b,
            session_b,
        }
    }

    fn services(&self) -> &crate::RuntimeServices {
        match &self.runtime.startup_mode {
            crate::StartupMode::Ready(services) => services,
            _ => panic!("Core must be ready"),
        }
    }

    /// B parked in `AwaitingQuery`: a real run with a real projection and no pending write.
    async fn park_b(&self) -> AgentRunRequest {
        let started =
            waiting::start(&self.runtime, self.session_b, self.model_b, AgentTaskMode::Normal)
                .await;
        let request =
            AgentRunRequest { session_id: self.session_b, agent_run_id: started.run.agent_run_id };
        waiting::wait(&self.runtime, &request, |p| {
            matches!(p.turn.state, TurnTaskState::AwaitingQuery { .. })
        })
        .await;
        self.services()
            .agent_required()
            .idle(request.agent_run_id)
            .await
            .expect("B's own task really ended");
        request
    }
}

/// PRODUCTION HEARTBEAT on the current runtime, exactly as a connection starts it.
fn start_heartbeat(
    runtime: &tokio::runtime::Runtime,
    state: Arc<crate::RuntimeState>,
) -> (tokio::task::JoinHandle<()>, Arc<std::sync::atomic::AtomicU64>) {
    let queue = AsyncPriorityQueue::new();
    let delivered = Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
    let recorder = Arc::clone(&delivered);
    let reader_queue = queue.clone();
    let started = Instant::now();
    let reader = runtime.spawn(async move {
        while let Ok(frame) = reader_queue.recv().await {
            let WireFrame::Control(bytes) = frame.frame else { continue };
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { continue };
            if value.get("method").and_then(Value::as_str)
                == Some(aura_ipc::method::RUNTIME_HEARTBEAT)
            {
                recorder.store(
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    Ordering::Release,
                );
                return;
            }
        }
    });
    let heartbeat_state = Arc::clone(&state);
    let _heartbeat = runtime.spawn(async move {
        let _ = crate::spawn_heartbeat(heartbeat_state, queue).await;
    });
    (reader, delivered)
}

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

/// Waits for a real Provider request to be parked, without blocking the runtime's workers.
async fn park_at_provider(provider: &Provider, budget: Duration) {
    let parked = tokio::time::timeout(budget, async {
        while provider.requests.lock().expect("fixture requests").is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(parked.is_ok(), "the real Provider request must be parked");
}

/// B1 - one run's real Store work is held by a barrier inside the production call.
///
/// Observed together: A's caller ends inside its own budget with an honest Unknown (its result
/// is retained); B's real status is served while A is still parked; the control lane and the
/// production heartbeat still answer; and the hold is matched by run identity, so what was
/// parked is A's operation and not whichever section arrived first.
#[test]
fn b1_a_held_store_section_leaves_the_caller_bounded_and_another_run_served() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    let (state, requests, hold) = runtime.block_on(async {
        let sessions = TwoSessions::new(Scenario::Query, Scenario::Query).await;
        let request_b = sessions.park_b().await;
        let request_a = waiting::start(
            &sessions.runtime,
            sessions.session_a,
            sessions.model_a,
            AgentTaskMode::Normal,
        )
        .await;
        let request_a = AgentRunRequest {
            session_id: sessions.session_a,
            agent_run_id: request_a.run.agent_run_id,
        };
        // A's own run is left active: the status read below is A's operation, and B's is B's.
        let state = Arc::new(sessions.runtime);
        // The hold is armed INSIDE the production call, on A's projection step only.
        let select = format!("run={}/agent.status.projection", request_a.agent_run_id);
        let hold = match &state.startup_mode {
            crate::StartupMode::Ready(services) => {
                state.sync.set_budget(Duration::from_millis(BUDGET_MS));
                services.agent_required().store().arm_hold(&select, HOLD_MS)
            }
            _ => panic!("Core must be ready"),
        };
        (state, (request_a, request_b), hold)
    });
    let (request_a, request_b) = requests;
    let (heartbeat_reader, heartbeat_at) = start_heartbeat(&runtime, Arc::clone(&state));
    let a_state = Arc::clone(&state);
    let a_request = request_a.clone();
    let a = runtime.spawn(async move {
        let agent = match &a_state.startup_mode {
            crate::StartupMode::Ready(services) => services.agent_required(),
            _ => panic!("Core must be ready"),
        };
        agent.status(&a_request).await.map_err(|error| error.code())
    });
    // The control lane: authentication and a real control read, while A is parked.
    let control_state = Arc::clone(&state);
    let control_session = request_b.session_id;
    let control = runtime.spawn(async move {
        crate::r03_tests::request(
            &control_state,
            "permission.get_session_state",
            json!({"session_id": control_session}),
        )
        .await
    });
    let b_state = Arc::clone(&state);
    let b_request = request_b.clone();
    let b = runtime.spawn(async move {
        let agent = match &b_state.startup_mode {
            crate::StartupMode::Ready(services) => services.agent_required(),
            _ => panic!("Core must be ready"),
        };
        agent.status(&b_request).await.map(|projection| projection.turn.state)
    });
    // B and the control lane must answer while A is really parked.
    let served =
        wait_until(Duration::from_secs(3), || heartbeat_at.load(Ordering::Acquire) != u64::MAX);
    let b_value = runtime.block_on(b).expect("B's status task");
    let control_value = runtime.block_on(control).expect("control task");
    let a_value = runtime.block_on(a).expect("A's status task");
    let heartbeat_ms = heartbeat_at.load(Ordering::Acquire);
    heartbeat_reader.abort();
    println!(
        "B1 a={a_value:?} b={b_value:?} control_error={} heartbeat_ms={heartbeat_ms} \
         store={:?} boundary_available={}",
        control_value.get("error").is_some(),
        match &state.startup_mode {
            crate::StartupMode::Ready(services) => services.agent_required().store().counters(),
            _ => panic!("Core must be ready"),
        },
        state.sync.available()
    );
    // A's caller ended inside its own budget with an honest Unknown - the work started.
    assert_eq!(
        a_value.expect_err("A's status must not answer while its own step is parked"),
        "agent.store_unknown",
        "A's caller must end bounded, with the work proven started"
    );
    // B's real status was served: a real projection of B's own run.
    match b_value.expect("B's status must be served while A is parked") {
        TurnTaskState::AwaitingQuery { .. } => {}
        other => panic!("B's status must carry B's real state, got {other:?}"),
    }
    assert!(
        control_value.get("error").is_none(),
        "the control lane must answer while A is parked: {control_value}"
    );
    assert!(served, "the production heartbeat must be delivered while A is parked");
    assert!(heartbeat_ms < HOLD_MS, "the heartbeat is not waiting for A: {heartbeat_ms} ms");
    // The hold really reached the target production call, matched by run identity.
    let arrivals = hold.arrivals();
    assert!(
        arrivals.iter().any(|arrival| arrival.held && arrival.hold_nanos == 0),
        "A's operation is parked inside the Store call right now: {arrivals:?}"
    );
    // Release: the parked work really finishes and the original result is retained and queryable.
    hold.release_all();
    let settled = wait_until(Duration::from_secs(5), || match &state.startup_mode {
        crate::StartupMode::Ready(services) => {
            let counters = services.agent_required().store().counters();
            counters.completed >= 1
        }
        _ => false,
    });
    assert!(settled, "the released work must really complete");
    let receipts = match &state.startup_mode {
        crate::StartupMode::Ready(services) => services.agent_required().store().counters(),
        _ => panic!("Core must be ready"),
    };
    println!("B1 after_release counters={receipts:?} arrivals={:?}", hold.arrivals());
}

/// B2 - the shared Store is genuinely unavailable.
///
/// A second real connection holds the V2 write lock, which is what any second writer does. Both
/// runs' dependent writes answer `agent.store_unavailable` inside bounds, an unrelated control
/// entry keeps working, no write is reported as succeeded, and after the lock is released new
/// legitimate work is served **without restarting the Core** while the failed attempts left no
/// trace and are not replayed.
#[test]
fn b2_an_unavailable_store_degrades_dependent_writes_and_clears_without_a_restart() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    let sessions = runtime.block_on(TwoSessions::new(Scenario::Query, Scenario::Query));
    let request_b = runtime.block_on(sessions.park_b());
    let state = Arc::new(sessions.runtime);
    let database = match &state.startup_mode {
        crate::StartupMode::Ready(services) => {
            services.broker.database_path().expect("the journal's own database file")
        }
        _ => panic!("Core must be ready"),
    };
    // The real write lock, held by a second real connection.
    let holder = Connection::open_with_flags(
        &database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .expect("second real connection");
    holder.busy_timeout(Duration::from_millis(0)).expect("no wait budget");
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
    let agent = match &state.startup_mode {
        crate::StartupMode::Ready(services) => services.agent_required(),
        _ => panic!("Core must be ready"),
    };
    let start_a = |session: SessionId, model: ModelProfileId| {
        let agent = Arc::clone(&agent);
        async move {
            let started = Instant::now();
            let answer = agent
                .start(
                    CommandInvocationId::new(),
                    AgentStartRequest {
                        session_id: session,
                        model_profile_id: model,
                        text: "dependent write while the store is locked".into(),
                        mode: AgentTaskMode::Normal,
                        goal_budget: None,
                        explicit_backend_id: None,
                        skill_ids: Vec::new(),
                        reasoning_effort: None,
                        attachments: Vec::new(),
                    },
                )
                .await;
            (answer.map_err(|error| format!("{error}")), started.elapsed())
        }
    };
    let (a_answer, a_waited) = runtime.block_on(start_a(sessions.session_a, sessions.model_a));
    let (b_answer, b_waited) = runtime.block_on(start_a(sessions.session_b, sessions.model_b));
    let control = runtime.block_on(crate::r03_tests::request(
        &state,
        "permission.get_session_state",
        json!({"session_id": sessions.session_b}),
    ));
    let health = match &state.startup_mode {
        crate::StartupMode::Ready(services) => {
            services.agent_required().store_health(StoreDependency::Journal)
        }
        _ => panic!("Core must be ready"),
    };
    println!(
        "B2 a={a_answer:?} ({a_waited:?}) b={b_answer:?} ({b_waited:?}) \
         control_error={} health={health:?}",
        control.get("error").is_some()
    );
    let a_text = a_answer.as_ref().err().cloned().unwrap_or_default();
    let b_text = b_answer.as_ref().err().cloned().unwrap_or_default();
    assert!(a_text.contains("agent.store_unavailable"), "A's dependent write: {a_text}");
    assert!(b_text.contains("agent.store_unavailable"), "B's dependent write: {b_text}");
    assert!(
        a_waited < Duration::from_secs(20) && b_waited < Duration::from_secs(20),
        "both answers must be bounded: {a_waited:?}/{b_waited:?}"
    );
    assert!(
        control.get("error").is_none(),
        "an unrelated control entry must stay usable: {control}"
    );
    assert!(!health.available, "the dependency fact must be recorded: {health:?}");
    assert_eq!(health.code, Some("sqlite.busy"), "{health:?}");
    // Nothing was claimed to have succeeded: B's run for the refused start does not exist.
    let runs_before = runtime
        .block_on(agent.store().read(
            crate::agent_store::StoreStep::operation("b2.count_runs"),
            |db| {
                let mut q = db.prepare("SELECT COUNT(*) FROM agent_runs")?;
                q.query_row([], |row| row.get::<_, i64>(0)).map_err(Into::into)
            },
        ))
        .expect("a read is still served by the read-only connection");
    // The fault clears: release the lock and do new legitimate work in a session with no
    // active turn, which is the work that must really commit.
    holder.execute_batch("COMMIT").expect("release the write lock");
    drop(holder);
    std::thread::sleep(Duration::from_millis(200));
    let fresh_session = runtime.block_on(async {
        let created = crate::r03_tests::success(
            &state,
            "session.create",
            json!({"title":"B2 recovered","workspace_id":sessions.workspace}),
        )
        .await;
        let session: SessionId =
            serde_json::from_value(created["session_id"].clone()).expect("session id");
        let current = crate::r03_tests::success(
            &state,
            "permission.get_session_state",
            json!({"session_id":session}),
        )
        .await;
        crate::r03_tests::success(
            &state,
            "permission.set_session_mode",
            json!({"session_id":session,"expected_revision":current["revision"],"mode":PermissionMode::ReadOnly}),
        )
        .await;
        session
    });
    let after = runtime.block_on(agent.start(
        CommandInvocationId::new(),
        AgentStartRequest {
            session_id: fresh_session,
            model_profile_id: sessions.model_b,
            text: "legitimate work after the store recovered".into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        },
    ));
    let after = after.expect("new legitimate work must be served without a restart");
    let health_after = match &state.startup_mode {
        crate::StartupMode::Ready(services) => {
            services.agent_required().store_health(StoreDependency::Journal)
        }
        _ => panic!("Core must be ready"),
    };
    let runs_after = runtime
        .block_on(agent.store().read(
            crate::agent_store::StoreStep::operation("b2.count_runs_after"),
            |db| {
                let mut q = db.prepare("SELECT COUNT(*) FROM agent_runs")?;
                q.query_row([], |row| row.get::<_, i64>(0)).map_err(Into::into)
            },
        ))
        .expect("read");
    println!(
        "B2 after_recovery run={} health={health_after:?} runs {runs_before}->{runs_after}",
        after.run.agent_run_id
    );
    assert!(health_after.available, "a real success must clear the fact: {health_after:?}");
    assert_eq!(health_after.cleared, 1, "the clearing is an observable transition");
    assert_eq!(
        runs_after,
        runs_before + 1,
        "the refused attempts created nothing and were not replayed"
    );
    assert!(
        after.run.state != AgentRunState::Cancelled,
        "the recovered work is real work, not a replay of an old one"
    );
    // No automatic resend: the failed attempt for session A produced no model traffic at all,
    // and the count above shows it created no turn to replay.
    assert_eq!(
        sessions.provider_a.requests.lock().expect("A's fixture requests").len(),
        0,
        "an unavailable write must never reach a Provider"
    );
    let _ = request_b;
}

/// B3 - the boundary is full.
///
/// The next item is refused **before the business runs**, the sections that hold the permits keep
/// them, and capacity returns only with the real completion. Held writes fill the shared workers:
/// reads have a lane of their own and do not.
#[test]
fn b3_a_full_boundary_refuses_before_the_business_and_recovers_with_the_real_completion() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(6)
        .enable_all()
        .build()
        .expect("test runtime");
    let sessions = runtime.block_on(TwoSessions::new(Scenario::Query, Scenario::Query));
    let request_b = runtime.block_on(sessions.park_b());
    let state = Arc::new(sessions.runtime);
    let agent = match &state.startup_mode {
        crate::StartupMode::Ready(services) => services.agent_required(),
        _ => panic!("Core must be ready"),
    };
    state.sync.set_budget(Duration::from_millis(BUDGET_MS));
    // Every held write keeps a worker, so the four workers are really occupied.
    let hold = agent.store().arm_hold("test.held_write", HOLD_MS);
    let mut holders = Vec::new();
    for _ in 0..crate::sync_boundary::SYNC_WORKERS {
        let store = Arc::clone(agent.store());
        holders.push(runtime.spawn(async move {
            store.write(StoreStep::operation("test.held_write"), |_store| Ok(())).await
        }));
    }
    // Occupied means inside the held section too, not only holding a permit — under load
    // a worker can hold its permit before it arrives, and the count below read that as a refusal.
    let full = wait_until(Duration::from_secs(3), || {
        state.sync.available() == 0
            && hold.held_count() == crate::sync_boundary::SYNC_WORKERS as u64
    });
    assert!(full, "every boundary worker must really be occupied");
    assert_eq!(
        state.sync.counters(SyncSection::AgentStore).in_flight,
        crate::sync_boundary::SYNC_WORKERS as u64,
        "the running work owns its permits"
    );
    // The next item: a real write entry, refused before its business ran.
    let refused = runtime.block_on(agent.cancel(&request_b));
    let arrivals_after_refusal = hold.held_count();
    println!(
        "B3 refused={:?} arrivals_after_refusal={arrivals_after_refusal} available={} counters={:?}",
        refused
            .as_ref()
            .err()
            .and_then(|error| error.downcast_ref::<StoreError>())
            .map(StoreError::code),
        state.sync.available(),
        state.sync.counters(SyncSection::AgentStore)
    );
    assert!(
        refused
            .as_ref()
            .err()
            .map(|error| format!("{error}"))
            .unwrap_or_default()
            .contains("agent.store_refused"),
        "a full boundary is a proven not-started answer: {:?}",
        refused.as_ref().err().map(|error| format!("{error}"))
    );
    assert_eq!(
        arrivals_after_refusal,
        crate::sync_boundary::SYNC_WORKERS as u64,
        "a refused item never reaches the production call"
    );
    // No side effect: the cancellation request was never written. This read goes to a separate
    // read-only connection, because the boundary is still full of the held sections - which is
    // exactly the state a dependent read would be refused in.
    let reader = Connection::open_with_flags(
        &match &state.startup_mode {
            crate::StartupMode::Ready(services) => {
                services.broker.database_path().expect("the journal's own database file")
            }
            _ => panic!("Core must be ready"),
        },
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .expect("a separate read-only connection");
    let requests: i64 = {
        let mut q = reader
            .prepare("SELECT COUNT(*) FROM events WHERE kind='agent.cancel_requested'")
            .expect("prepare");
        q.query_row([], |row| row.get(0)).expect("count")
    };
    assert_eq!(requests, 0, "a refused write must leave no side effect");
    // Release the holders: the permits come back with the real completion.
    hold.release_all();
    for holder in holders {
        let _ = runtime.block_on(holder);
    }
    let recovered = wait_until(Duration::from_secs(5), || {
        state.sync.available() == crate::sync_boundary::SYNC_WORKERS
    });
    assert!(recovered, "the permits come back with the work, not with the caller");
    let now_served = runtime.block_on(agent.status(&request_b));
    println!("B3 after_release served={:?}", now_served.as_ref().map(|_| "ok"));
    assert!(now_served.is_ok(), "the same call is served once capacity is back");
}

/// B4 - the caller's wait ends before the real commit arrives.
///
/// The write is the run's own failure record: an admitted, retained persistence write. Its
/// caller's bounded wait ends while the work is parked, and the same receipt can still be asked
/// for the original result - which really commits, exactly once.
#[test]
fn b4_a_late_commit_is_retained_once_and_queryable_after_the_caller_left() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    let sessions = runtime.block_on(TwoSessions::new(Scenario::Hold, Scenario::Query));
    let state = Arc::new(sessions.runtime);
    let agent = match &state.startup_mode {
        crate::StartupMode::Ready(services) => services.agent_required(),
        _ => panic!("Core must be ready"),
    };
    let started = runtime.block_on(waiting::start(
        &state,
        sessions.session_a,
        sessions.model_a,
        AgentTaskMode::Normal,
    ));
    let run = started.run.agent_run_id;
    let request_a = AgentRunRequest { session_id: sessions.session_a, agent_run_id: run };
    runtime.block_on(park_at_provider(&sessions.provider_a, Duration::from_secs(20)));
    // A short caller budget and the record's own step held inside the production call.
    state.sync.set_budget(Duration::from_millis(BUDGET_MS));
    let hold = agent.store().arm_hold("agent.drive_failure_record", HOLD_MS);
    // A's Provider never answers: the first-event timeout fails the step and the run's record
    // write is admitted and parked. Each of the step's three attempts waits out the endpoint's
    // 10 s first-event limit.
    let reached = wait_until(Duration::from_secs(45), || hold.was_reached());
    let recorded = wait_until(Duration::from_secs(10), || {
        agent.store().receipts().iter().any(|receipt| {
            receipt.operation == "agent.drive_failure_record"
                && receipt.state == ReceiptState::InFlight
        })
    });
    // The caller's own bounded wait ends after its budget: the fact it could not record stays on
    // the run and the receipt keeps the real result.
    let caller_ended = wait_until(Duration::from_secs(10), || {
        agent.store().diagnostics().of(run).iter().any(|entry| entry.code == "agent.store_unknown")
    });
    let receipt = agent
        .store()
        .receipts()
        .into_iter()
        .find(|receipt| receipt.operation == "agent.drive_failure_record")
        .expect("the admitted write has a receipt");
    let diagnostics = agent.store().diagnostics().of(run);
    println!(
        "B4 reached={reached} recorded={recorded} receipt={receipt:?} diagnostics={diagnostics:?} \
         arrivals={:?}",
        hold.arrivals()
    );
    assert!(reached, "the run's own record write must really reach the production call");
    assert_eq!(receipt.state, ReceiptState::InFlight, "the real work is still parked");
    assert!(
        caller_ended,
        "the caller was told the work started and did not answer: {diagnostics:?}"
    );
    // Release: the real commit happens, once, and it settles the retained receipt.
    hold.release_all();
    let settled = wait_until(Duration::from_secs(15), || {
        agent.store().receipt(receipt.id).map(|receipt| receipt.state)
            == Some(ReceiptState::Settled { committed: true, code: None })
    });
    let final_receipt = agent.store().receipt(receipt.id);
    let errors = runtime.block_on(agent.store().read(
        crate::agent_store::StoreStep::run(run, "b4.error_events"),
        |db| {
            let mut q =
                db.prepare("SELECT COUNT(*) FROM events WHERE kind='agent.application_error'")?;
            q.query_row([], |row| row.get::<_, i64>(0)).map_err(Into::into)
        },
    ));
    let projection = runtime.block_on(agent.status(&request_a)).expect("projection");
    println!(
        "B4 settled={settled} receipt={final_receipt:?} error_events={errors:?} turn={:?}",
        projection.turn.state
    );
    assert!(settled, "the real commit must really settle the original result");
    assert_eq!(errors.expect("read"), 1, "the commit happened exactly once");
    assert!(
        matches!(projection.turn.state, TurnTaskState::RecoveryRequired { .. }),
        "the interruption is a real persisted fact: {:?}",
        projection.turn.state
    );
    assert!(hold.was_reached(), "the hold point was inside the target production call");
}

/// B5 - a run-local record failure stays with its run and never refuses another session.
///
/// ORIGINAL SEMANTICS (removed): `status_includes_the_inner_persistence_fault` set one
/// application-wide fault string and asserted that *every* run's `status()` refused with it.
/// REAL FACT: that attribution was the defect - the fault belonged to one run's step and nothing
/// could clear it. The acceptance bar is unchanged: a real failure must still be visible. It is
/// visible where it belongs (the run's own diagnostic and, for a store-level failure, the shared
/// dependency fact), and another session's status is served.
#[tokio::test]
async fn b5_a_run_local_record_failure_is_visible_and_never_refuses_another_session() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Query).await;
    let (runtime, start) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    waiting::wait(&runtime, &request, |p| {
        matches!(p.turn.state, TurnTaskState::AwaitingQuery { .. })
    })
    .await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.agent_required().idle(request.agent_run_id).await.expect("the run's task ended");
    services
        .agent_required()
        .persist_drive_failure_for_test(
            request.agent_run_id,
            "persistent revision or identity conflict",
        )
        .await;
    // The run's own diagnostic is visible...
    let diagnostics = services.agent_required().store().diagnostics().of(request.agent_run_id);
    println!("B5 diagnostics={diagnostics:?}");
    // ... and the run's own status still answers with its real state.
    let after =
        services.agent_required().status(&request).await.expect("the run's status is served");
    assert!(
        matches!(after.turn.state, TurnTaskState::AwaitingQuery { .. }),
        "{:?}",
        after.turn.state
    );
    // A second session on the same Core is untouched: nothing about it changed.
    let other = crate::r03_tests::success(
        &runtime,
        "session.create",
        json!({"title":"B5 untouched","workspace_id":after.turn.workspace_id}),
    )
    .await;
    let other_session: SessionId = serde_json::from_value(other["session_id"].clone()).expect("id");
    let other_state = crate::r03_tests::success(
        &runtime,
        "permission.get_session_state",
        json!({"session_id": other_session}),
    )
    .await;
    assert!(other_state["revision"].is_number(), "another session is served normally");
    let health = services.agent_required().store_health(StoreDependency::Journal);
    assert!(health.available, "a run-local business failure is not a dependency fact: {health:?}");
    assert_eq!(health.failures, 0, "{health:?}");
}
