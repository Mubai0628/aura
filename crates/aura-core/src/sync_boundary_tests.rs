//! R3 regressions: the bounded synchronous boundary, exercised through the real Core
//! service entries rather than through a pool helper.
//!
//! Each case drives `handle_request` - the same entry `run_handler` calls for a real IPC
//! request, with the same lanes, the same claim ledger, the same routes and the same
//! services - against an isolated real database and real workspace trees. The only thing
//! that is injected is a real hold point in the storage layer
//! (`PersistenceCoordinator::arm_admission_hold`), which is a coordination path the storage
//! layer already carries and which the R1/R2 cases use as well.
//!
//! What the cases are about is not "is a request fast" but "can the Core still answer at
//! all": the heartbeat, the control lane and another workspace's permitted work must keep
//! progressing while synchronous sections are stuck, because those all share the same
//! reactor. A section that cannot be admitted is refused inside its budget, honestly, and
//! its permit comes back only when the work that owns it really finishes.
use crate::r03_tests::Fixture;
use crate::sync_boundary::{SYNC_WORKERS, SyncSection};
use crate::*;
use aura_domain_common::{SessionId, WorkspaceId};
use aura_ipc::method;
use aura_workspace_domain::SessionSummary;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// The same number of reactor workers as the boundary has workers.
///
/// That equality is the point of the third case: when the same number of sections block the
/// reactor as there are reactor workers, nothing else can run at all.
const TEST_WORKER_THREADS: usize = SYNC_WORKERS;

/// How long the real storage hold parks each caller that reaches it.
const HOLD_MS: u64 = 2_600;
/// The boundary budget these cases run with. Short on purpose: a refusal must be observable
/// without waiting the production 30 s, and the budget is the only thing that shrinks.
const BUDGET_MS: u64 = 400;
/// Observe a real 2 s heartbeat while the hold is still active. The hold's fallback is longer
/// than this observation window, so timeout-based auto-release cannot make this gate pass.
///
/// THE WINDOW IS UNCHANGED (4 s) AND IS NOW MEASURED FROM ABSOLUTE INSTANTS.
///
/// What S04TS changed here is the MEASUREMENT, not the requirement. The previous reading was
/// `first_heartbeat_ms`, milliseconds from "just before `spawn`" to the frame being dequeued,
/// and a failing full-set run produced `6001`/`22016` - the hold's own length, plus a few ms.
/// That single number has two completely different causes, and the old sentinel could not tell
/// them apart because it also started its clock before `spawn` and created its `sleep` inside
/// the task:
///
///   * the task was not *polled* until the hold ended (a scheduler/progress fact), or
///   * the task was polled at once and its *timer* did not fire until the hold ended (a
///     timer-delivery fact).
///
/// Both of those are facts about this process's task scheduling while a real synchronous
/// section is parked. Neither of them is evidence about the heartbeat path, and - this is the
/// correction that matters - neither of them exonerates the product's synchronous boundary
/// either: `two timers delayed together` proves only that delivery to this runtime was
/// impeded, and the parked section really is production work holding a real storage lane.
/// So this case now records, on ONE monotonic origin, the spawn, the first poll, the `sleep`
/// creation and the wake of every probe, the heartbeat's own `sent_at` (its enqueue instant)
/// and its dequeue, the real hold arrival/release from the storage barrier itself, and the
/// thread each event happened on - plus, independently, whether a plain OS thread outside the
/// runtime kept running. The requirement it asserts is the original one: the heartbeat must be
/// dequeued before the hold's real release instant AND inside the 4 s window.
const HEARTBEAT_WINDOW_MS: u64 = 4_000;
const HEARTBEAT_HOLD_MS: u64 = HEARTBEAT_WINDOW_MS + 2_000;
/// How often the two independent progress witnesses report. They say only "this thread/this
/// runtime was scheduled again", never anything about the product.
const PROGRESS_TICK_MS: u64 = 50;
/// How many progress ticks one witness records; covers the window, the hold and the release.
const PROGRESS_TICKS: usize = 220;
/// How long past the hold's real release the case keeps watching for a late heartbeat, so a
/// delivery that only happened *because* the hold ended is recorded as exactly that instead of
/// being mistaken for a delivery during the hold.
const LATE_OBSERVATION_MS: u64 = 1_500;

struct Harness {
    // Declared first: the route ends before the fixture's directory is removed.
    _route: crate::CoreLogRoute,
    _fixture: Fixture,
    state: Arc<RuntimeState>,
    workspace: WorkspaceId,
    session: SessionId,
    other_workspace: WorkspaceId,
    other_session: SessionId,
}

impl Harness {
    /// The fixture's own isolated root, so every byte this case writes lands in it.
    fn isolated_root(fixture: &Fixture) -> PathBuf {
        fixture.root.path().to_path_buf()
    }

    async fn new(budget_ms: u64) -> Self {
        let fixture = Fixture::new();
        // THE CASE'S DIAGNOSTICS GO IN THE FIXTURE, NOT IN THE USER'S LOG.
        //
        // The test route (now `CoreLogRoute`) exists for exactly this ("routes core diagnostics
        // for one test"). Before these cases set it, `core_log_path` returned
        // `%LOCALAPPDATA%\Aura\logs\core.jsonl`, so `cargo test -p aura-core --lib` appended to
        // the real 50 MB log of whoever ran it - a host write this suite must not make, and a
        // shared file every one of these cases then had to append to and re-`create_dir_all`
        // from whichever thread happened to be logging. Setting the route before the Core starts
        // covers the startup stage as well as the case. The harness keeps the route until it is
        // dropped, and no other case can route the log meanwhile.
        let route = crate::CoreLogRoute::to(Self::isolated_root(&fixture).join("core.jsonl"));
        let state = Arc::new(fixture.start().await);
        let (workspace, session) =
            workspace_with_session(&fixture, &state, "ws-a", "boundary A").await;
        let (other_workspace, other_session) =
            workspace_with_session(&fixture, &state, "ws-b", "boundary B").await;
        std::fs::write(fixture.root.path().join("ws-b").join("b.txt"), "b body\n")
            .expect("B's file");
        state.sync.set_budget(Duration::from_millis(budget_ms));
        // REGRESSION-ONLY INJECTION, DEFAULT-OFF. With this set the post-execution continuation is
        // discarded when a caller's wait ends - the behaviour that existed before the completion
        // owner - so the corrected regression can be run once against it and its failing output
        // frozen. Nothing in the product reads this, and the default (unset) keeps the tail.
        if std::env::var("AURA_S03F_TAIL_CONTINUATION").is_ok_and(|value| value == "off") {
            state.post_execution.force_no_continuation_for_test(true);
        }
        Self {
            _route: route,
            _fixture: fixture,
            state,
            workspace,
            session,
            other_workspace,
            other_session,
        }
    }

    fn persistence(&self) -> Arc<aura_persistence::PersistenceCoordinator> {
        let StartupMode::Ready(services) = &self.state.startup_mode else {
            panic!("Core must be ready");
        };
        Arc::clone(&services.persistence)
    }
}

async fn workspace_with_session(
    fixture: &Fixture,
    state: &Arc<RuntimeState>,
    directory: &str,
    display_name: &str,
) -> (WorkspaceId, SessionId) {
    let root = fixture.root.path().join(directory);
    std::fs::create_dir_all(&root).expect("workspace root");
    let created = crate::r03_tests::success(
        state,
        method::WORKSPACE_CREATE,
        json!({"selected_root": root, "display_name": display_name}),
    )
    .await;
    let workspace: WorkspaceId =
        serde_json::from_value(created["workspace"]["workspace_id"].clone()).expect("workspace id");
    let session = serde_json::from_value::<SessionSummary>(
        crate::r03_tests::success(
            state,
            "session.create",
            json!({"title": format!("{display_name} session"), "workspace_id": workspace}),
        )
        .await,
    )
    .expect("session summary")
    .session_id;
    (workspace, session)
}

/// One real request through the production handler entry, returning the wire response.
async fn call(state: Arc<RuntimeState>, method_name: &str, params: Value) -> Value {
    let queue = AsyncPriorityQueue::new();
    let request = JsonRpcRequest {
        jsonrpc: JSON_RPC_VERSION.into(),
        id: CorrelationId::new(),
        command_invocation_id: is_side_effecting_main_method(method_name)
            .then(CommandInvocationId::new),
        method: method_name.into(),
        params,
    };
    let claim = match crate::reserve_command_invocation(&state, &queue, &request)
        .await
        .expect("the fixture's claim entry")
    {
        crate::CommandReservation::Granted(claim) => claim,
        crate::CommandReservation::Refused => {
            panic!("{method_name}: the fixture refused its claim")
        }
    };
    handle_request(
        &state,
        &queue,
        &ProjectionSubscriptions::default(),
        &Arc::new(std::sync::Mutex::new(RequestLedger::default())),
        &request,
        claim.as_ref(),
    )
    .await
    .expect("the production handler must return a terminal");
    let WireFrame::Control(bytes) = queue.recv().await.expect("the handler must answer").frame
    else {
        panic!("the handler must answer with a control frame")
    };
    serde_json::from_slice(&bytes).expect("response json")
}

/// A real request whose completion time is recorded, so a gate can be about *when* it
/// answered rather than only about whether it did.
fn observed_call(
    runtime: &tokio::runtime::Runtime,
    state: Arc<RuntimeState>,
    method_name: &'static str,
    params: Value,
) -> (tokio::task::JoinHandle<Value>, Arc<AtomicU64>) {
    let elapsed = Arc::new(AtomicU64::new(u64::MAX));
    let recorder = Arc::clone(&elapsed);
    let started = Instant::now();
    let handle = runtime.spawn(async move {
        let value = call(state, method_name, params).await;
        recorder.store(
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            Ordering::Release,
        );
        value
    });
    (handle, elapsed)
}

/// One monotonic event, with the real OS thread it happened on.
#[derive(Clone)]
struct TimelineEvent {
    at_us: u64,
    what: String,
}

/// One case's own monotonic clock.
///
/// WHY THIS EXISTS. These cases are about *when* something ran while a real synchronous section
/// was parked, and the earlier reading of them could not separate the two mechanisms that both
/// produce "the timer came back when the hold ended":
///
///   * the probe task was never *polled* until the hold ended (a scheduler/progress fact), or
///   * the probe task was polled at once and its *timer* never fired until the hold ended (a
///     timer-delivery fact).
///
/// The old sentinel started its clock before `spawn` and created its `sleep` inside the task, so
/// its single number mixed "spawn to first poll", "first poll to wake" and the timer's own
/// lateness into one value. This clock records all of them separately, on one origin, and adds
/// an independent OS thread that never enters the runtime - so "this runtime stopped being
/// scheduled" and "this process stopped running" are different readings instead of one guess.
struct CaseTimeline {
    origin: Instant,
    wall_origin_ms: u64,
    events: Mutex<Vec<TimelineEvent>>,
}

impl CaseTimeline {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            origin: Instant::now(),
            wall_origin_ms: epoch_ms(),
            events: Mutex::new(Vec::new()),
        })
    }

    fn now_us(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    fn mark(&self, label: impl Into<String>) {
        let at_us = self.now_us();
        let what = format!("{} on={}", label.into(), thread_identity());
        if let Ok(mut events) = self.events.lock() {
            events.push(TimelineEvent { at_us, what });
        }
    }

    /// A wall-clock millisecond instant - which is what the storage barrier records - on this
    /// monotonic origin. Accurate to the millisecond, which is the barrier's own resolution.
    fn origin_us_of_wall_ms(&self, wall_ms: u64) -> i128 {
        (i128::from(wall_ms) - i128::from(self.wall_origin_ms)) * 1_000
    }

    fn events(&self) -> Vec<TimelineEvent> {
        let mut events = self.events.lock().map(|events| events.clone()).unwrap_or_default();
        events.sort_by_key(|event| event.at_us);
        events
    }

    /// The case's whole clock as one JSON object, so a reader parses values instead of prose.
    fn evidence(&self, case: &str, extra: Value) -> Value {
        json!({
            "schema": "aura.s04ts.case-timeline.v1",
            "case": case,
            "wall_origin_ms": self.wall_origin_ms,
            "events": self.events().iter().map(|event| {
                json!({"at_us": event.at_us, "what": event.what})
            }).collect::<Vec<_>>(),
            "extra": extra,
        })
    }
}

fn thread_identity() -> String {
    let current = std::thread::current();
    format!("{}#{:?}", current.name().unwrap_or("unnamed"), current.id())
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

/// The real instant inside one heartbeat probe, on the case's monotonic origin.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct HeartbeatInstants {
    /// When the probe's own clock started (just before `spawn`), origin-relative.
    spawn_us: u64,
    /// When the production heartbeat task was first polled, origin-relative.
    task_polled_us: u64,
    /// The `sent_at` the production task stamped on the frame, origin-relative. This is the
    /// enqueue instant and it comes from the frame itself, not from an assumption.
    queued_us: u64,
    /// When the frame was really dequeued, origin-relative.
    dequeued_us: u64,
    /// Milliseconds from the probe's own start to the dequeue, for comparability with the
    /// earlier round's `first_heartbeat_ms` readings. `u64::MAX` when it never arrived.
    delivered_ms: u64,
}

/// The production heartbeat task, against its own queue, with the real instants kept.
///
/// This is `spawn_heartbeat` itself - the same task a connection starts, with the same interval
/// and the same P0 priority. Nothing about it is re-implemented; the test watches its queue and
/// reads the frame's own `sent_at`, so "enqueued" and "dequeued" are both measured rather than
/// inferred from the interval.
struct HeartbeatProbe {
    instants: Arc<Mutex<HeartbeatInstants>>,
    reader: tokio::task::JoinHandle<()>,
}

impl HeartbeatProbe {
    fn instants(&self) -> HeartbeatInstants {
        self.instants.lock().map(|instants| *instants).unwrap_or_default()
    }

    fn dequeued(&self) -> Option<u64> {
        let instants = self.instants();
        (instants.dequeued_us != u64::MAX).then_some(instants.dequeued_us)
    }

    fn join(self) {
        // Dropping the production task's handle detaches it rather than cancelling it, which is
        // what these cases want: the heartbeat keeps its own schedule to the end of the case.
        drop(self.reader);
    }
}

fn start_heartbeat(
    runtime: &tokio::runtime::Runtime,
    state: Arc<RuntimeState>,
    timeline: &Arc<CaseTimeline>,
) -> HeartbeatProbe {
    let queue = AsyncPriorityQueue::new();
    let instants = Arc::new(Mutex::new(HeartbeatInstants {
        spawn_us: timeline.now_us(),
        task_polled_us: u64::MAX,
        queued_us: u64::MAX,
        dequeued_us: u64::MAX,
        delivered_ms: u64::MAX,
    }));
    let reader_instants = Arc::clone(&instants);
    let started = Instant::now();
    let reader_queue = queue.clone();
    let reader_timeline = Arc::clone(timeline);
    let reader = runtime.spawn(async move {
        while let Ok(frame) = reader_queue.recv().await {
            let WireFrame::Control(bytes) = frame.frame else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            if value.get("method").and_then(Value::as_str) != Some(method::RUNTIME_HEARTBEAT) {
                continue;
            }
            let delivered_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let queued_us = value
                .pointer("/params/sent_at")
                .and_then(Value::as_str)
                .and_then(parse_rfc3339_ms)
                .map_or(u64::MAX, |sent_at_ms| {
                    u64::try_from(reader_timeline.origin_us_of_wall_ms(sent_at_ms))
                        .unwrap_or(u64::MAX)
                });
            reader_timeline.mark(format!("heartbeat.dequeued delivered_ms={delivered_ms}"));
            if let Ok(mut slot) = reader_instants.lock() {
                slot.dequeued_us = reader_timeline.now_us();
                slot.queued_us = queued_us;
                slot.delivered_ms = delivered_ms;
            }
            return;
        }
    });
    // `spawn_heartbeat` is the production task and it is started inside the runtime, the way
    // the connection starts it; dropping a JoinHandle detaches it rather than cancelling it.
    let heartbeat_instants = Arc::clone(&instants);
    let heartbeat_timeline = Arc::clone(timeline);
    let heartbeat_state = Arc::clone(&state);
    let _heartbeat_task = runtime.spawn(async move {
        heartbeat_timeline.mark("heartbeat.task_polled");
        if let Ok(mut slot) = heartbeat_instants.lock() {
            slot.task_polled_us = heartbeat_timeline.now_us();
        }
        let _ = crate::spawn_heartbeat(heartbeat_state, queue).await;
    });
    HeartbeatProbe { instants, reader }
}

/// The instants one independent 2 s timer on the SAME runtime really produced.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SentinelInstants {
    spawn_us: u64,
    first_poll_us: u64,
    sleep_created_us: u64,
    woke_us: u64,
    /// Milliseconds from `spawn` to the wake, for comparability with the earlier round.
    fired_ms: u64,
}

struct SentinelProbe {
    instants: Arc<Mutex<SentinelInstants>>,
}

impl SentinelProbe {
    fn instants(&self) -> SentinelInstants {
        self.instants.lock().map(|instants| *instants).unwrap_or_default()
    }
}

/// An independent timer on the same runtime, with first poll and timer wake separated.
///
/// WHY THIS EXISTS. A full-suite run missed the 4 s window while the control lane answered in
/// 0 ms. That reading is about this runtime being able to make progress at all, and the earlier
/// sentinel could not say which half of it was late. This one records the spawn, the first poll,
/// the `sleep` creation and the wake as four separate instants, so the case can state whether
/// the task was never polled or its timer never fired - two different facts, neither of which is
/// evidence that the product's synchronous section is innocent.
///
/// It touches none of the Core's state: it is a plain `tokio::time::sleep` on the same runtime,
/// which is exactly what makes it a reading of the runtime's timer/worker delivery rather than of
/// the heartbeat path.
fn start_runtime_sentinel(
    runtime: &tokio::runtime::Runtime,
    after: Duration,
    timeline: &Arc<CaseTimeline>,
) -> SentinelProbe {
    let instants = Arc::new(Mutex::new(SentinelInstants {
        spawn_us: timeline.now_us(),
        first_poll_us: u64::MAX,
        sleep_created_us: u64::MAX,
        woke_us: u64::MAX,
        fired_ms: u64::MAX,
    }));
    let recorder = Arc::clone(&instants);
    let started = Instant::now();
    let sentinel_timeline = Arc::clone(timeline);
    runtime.spawn(async move {
        sentinel_timeline.mark("sentinel.first_poll");
        let first_poll_us = sentinel_timeline.now_us();
        let created_us = first_poll_us;
        sentinel_timeline.mark("sentinel.sleep_created");
        tokio::time::sleep(after).await;
        let woke_us = sentinel_timeline.now_us();
        let fired_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        sentinel_timeline.mark(format!("sentinel.sleep_woke fired_ms={fired_ms}"));
        if let Ok(mut slot) = recorder.lock() {
            slot.first_poll_us = first_poll_us;
            slot.sleep_created_us = created_us;
            slot.woke_us = woke_us;
            slot.fired_ms = fired_ms;
        }
    });
    SentinelProbe { instants }
}

/// A task that only ever asks the runtime to run it again.
///
/// It is not a product path and it asserts nothing. It exists so "the runtime delivered no task
/// progress between these two instants" is a reading the case can print, instead of a conclusion
/// drawn from one delayed timer.
fn start_progress_probe(
    runtime: &tokio::runtime::Runtime,
    timeline: &Arc<CaseTimeline>,
    name: &'static str,
) {
    let ticks = Arc::clone(timeline);
    runtime.spawn(async move {
        ticks.mark(format!("{name}.first_poll"));
        for index in 0..PROGRESS_TICKS {
            tokio::time::sleep(Duration::from_millis(PROGRESS_TICK_MS)).await;
            ticks.mark(format!("{name}.tick{index}"));
        }
    });
}

/// The same witness outside the runtime: a plain OS thread that never touches Tokio.
///
/// A runtime-only stall and a whole-process stall look identical from inside the runtime. This
/// thread is the difference: if it keeps ticking while the runtime's probes do not, those runtime
/// tasks stopped progressing while this OS thread still ran. It does not identify the worker or
/// lock responsible for that gap.
fn start_os_probe(timeline: &Arc<CaseTimeline>, name: &'static str) -> std::thread::JoinHandle<()> {
    let ticks = Arc::clone(timeline);
    std::thread::Builder::new()
        .name(format!("{name}-os"))
        .spawn(move || {
            for index in 0..PROGRESS_TICKS {
                std::thread::sleep(Duration::from_millis(PROGRESS_TICK_MS));
                ticks.mark(format!("{name}.tick{index}"));
            }
        })
        .expect("the OS progress witness")
}

/// The regression-only switch that explicitly enables the active diagnostic control.
///
/// The normal test path uses a passive witness. The switch is read inside the probe, so the two
/// cases' holds, windows, occupancy and assertions are identical in both diagnostic readings.
const INJECTION_SWITCH: &str = "AURA_S04Q_TIMER_INJECTION";

/// Active injection requires an explicit opt-in; unset or unrecognised values stay passive.
///
/// An opt-in run is only a diagnostic control and cannot establish the formal exit condition.
fn injection_enabled() -> bool {
    std::env::var(INJECTION_SWITCH).is_ok_and(|value| {
        let value = value.trim().to_ascii_lowercase();
        value == "on" || value == "1" || value == "true"
    })
}

/// Tasks pushed onto the runtime from an INDEPENDENT OS thread - the DIAGNOSTIC CONTROL.
///
/// WHY THIS IS NOT THE SAME AS THE TIMER PROBES. A timer probe can only report "this timer did
/// not fire", which is equally consistent with a stalled time driver and with every worker being
/// blocked somewhere that never returns to the runtime's park. This probe asks the other
/// question directly: can an outside thread still get a task polled on this runtime at all? Its
/// push goes through the runtime's inject queue and wakes a parked worker, so a tick recorded on
/// time during another probe's stall means work was being run and only the timer path was late,
/// while a tick recorded late establishes delayed task delivery. Neither reading identifies
/// the responsible worker, timer driver, lock or owner by itself.
///
/// WHY IT IS SWITCHED OFF FOR THE FORMAL READING. `Handle::spawn` from an outside thread PUSHES
/// WORK into the runtime and unparks a worker. That is an intervention, not an observation: it
/// changes exactly the condition under test (whether this runtime delivers its own timers and
/// wakes while nothing outside it pushes work in). A green run with 119 pushes per case can
/// therefore not show that the stall is gone - it can only show that the pushes prevent it.
///
/// So this is the single diagnostic control, and the two readings are:
///
/// * **default (injection off)** - the thread sleeps and records passive marks; it pushes no
///   tasks and wakes no workers. This is the formal test path.
/// * **`AURA_S04Q_TIMER_INJECTION=on`** - the historical active injection is an explicit
///   diagnostic control. A green run with this intervention cannot establish a repair.
///
/// It never touches the product, never replaces the heartbeat, and is not a fix for anything: it
/// is the instrument whose intervention previously hid the failure.
fn start_inject_probe(
    runtime: &tokio::runtime::Runtime,
    timeline: &Arc<CaseTimeline>,
    name: &'static str,
) -> std::thread::JoinHandle<()> {
    let handle = runtime.handle().clone();
    let ticks = Arc::clone(timeline);
    let enabled = injection_enabled();
    std::thread::Builder::new()
        .name(format!("{name}-inject"))
        .spawn(move || {
            if !enabled {
                // The passive reading: this thread is still a witness of THIS PROCESS being
                // scheduled - it keeps the same cadence and the same lifetime as the injecting
                // one - but it pushes no work and wakes no worker, so nothing in the runtime is
                // disturbed by it.
                ticks.mark(format!("{name}.passive_witness"));
                for index in 0..PROGRESS_TICKS {
                    std::thread::sleep(Duration::from_millis(PROGRESS_TICK_MS));
                    ticks.mark(format!("{name}.passive{index}"));
                }
                return;
            }
            for index in 0..PROGRESS_TICKS {
                std::thread::sleep(Duration::from_millis(PROGRESS_TICK_MS));
                let ticks = Arc::clone(&ticks);
                handle.spawn(async move {
                    ticks.mark(format!("{name}.tick{index}"));
                });
            }
        })
        .expect("the inject witness")
}

/// What the storage layer was doing while the case watched, sampled from outside the runtime.
///
/// The writer's own health is the layer's statement about the lane the hold is armed on
/// (`sync_queued`, `running`, `executed`), and the boundary's counters are its statement about
/// which sections are really in flight. Both are read here at a fixed cadence so a stall has the
/// layer's own state beside it instead of only a missing timer.
fn start_writer_watchdog(
    timeline: &Arc<CaseTimeline>,
    persistence: Arc<aura_persistence::PersistenceCoordinator>,
    state: Arc<RuntimeState>,
    name: &'static str,
) -> (Arc<std::sync::atomic::AtomicBool>, std::thread::JoinHandle<()>) {
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let stop = Arc::clone(&running);
    let watcher = Arc::clone(timeline);
    let join = std::thread::Builder::new()
        .name(format!("{name}-watch"))
        .spawn(move || {
            let mut index = 0;
            while stop.load(Ordering::Acquire) {
                let health = persistence.writer_health();
                let session = state.sync.counters(SyncSection::SessionCommand);
                let evidence = state.sync.counters(SyncSection::Evidence);
                watcher.mark(format!(
                    "{name}.sample{index} writer_state={:?} sync_queued={} index_queued={} \
                     running={} executed={} session_in_flight={} session_completed={} \
                     evidence_in_flight={}",
                    health.state,
                    health.sync_queued,
                    health.index_queued,
                    health.running,
                    health.executed,
                    session.in_flight,
                    session.completed,
                    evidence.in_flight
                ));
                index += 1;
                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .expect("the writer watchdog");
    (running, join)
}

/// The samples the watchdog recorded, in order, for the case's own report.
fn watchdog_samples(timeline: &Arc<CaseTimeline>, name: &str) -> Vec<String> {
    timeline
        .events()
        .into_iter()
        .filter(|event| event.what.starts_with(&format!("{name}.sample")))
        .map(|event| format!("{:.3} ms {}", event.at_us as f64 / 1000.0, event.what))
        .collect()
}

/// The longest gap between two consecutive ticks of one witness, and when it ended.
///
/// This is the one number that separates the readings: a timer-driven witness that stops while an
/// OS thread outside the runtime keeps ticking is this runtime failing to deliver; the same gap
/// in both is the process failing to run; and the inject witness - whose ticks are pushed from an
/// outside thread into the runtime's own queue - says whether any worker was there to take work
/// while the timers were overdue.
fn longest_tick_gap(timeline: &Arc<CaseTimeline>, name: &str) -> (u64, u64) {
    let ticks: Vec<u64> = timeline
        .events()
        .into_iter()
        .filter(|event| event.what.starts_with(&format!("{name}.tick")))
        .map(|event| event.at_us)
        .collect();
    let mut worst = (0_u64, 0_u64);
    for pair in ticks.windows(2) {
        let gap = pair[1].saturating_sub(pair[0]);
        if gap > worst.0 {
            worst = (gap, pair[1]);
        }
    }
    worst
}

/// Waits on the real clock, outside the async runtime, so the runtime's workers keep
/// running while this thread watches for a result.
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

/// Waits on the real clock until `deadline_us` on the case's origin.
///
/// It runs outside the async runtime on purpose, so the runtime's workers keep running while
/// this thread watches.
fn wait_until_us(
    timeline: &Arc<CaseTimeline>,
    deadline_us: u64,
    predicate: impl Fn() -> bool,
) -> bool {
    loop {
        if predicate() {
            return true;
        }
        if timeline.now_us() >= deadline_us {
            return predicate();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// What the heartbeat really did relative to the hold and the window.
///
/// EVERY FIELD IS A MEASUREMENT. The judgement never says "4 s passed, then 6 s more passed, so
/// the second reading was during the hold": the window is the probe's own spawn plus 4 s, and the
/// hold's release instant is the storage barrier's own record of how long the arrival it parked
/// really served. A delivery that only happened after that release is reported as a post-release
/// delivery and fails the gate; it is never counted as during-hold.
#[derive(Clone, Copy, Debug, Default)]
struct HeartbeatObservation {
    within_window: bool,
    before_hold_released: bool,
    instants: HeartbeatInstants,
    /// The release instant this judgement used, on the case's origin.
    hold_release_us: Option<u64>,
    /// How long the parked arrival really served, published by the barrier when its permit
    /// dropped.
    served_hold_us: u64,
    arrival_us: Option<u64>,
}

/// Waits for the first real heartbeat, bounded by the case's own absolute deadline.
///
/// The bound exists only so a run cannot hang: a delivery beyond it is still reported as a
/// missing measurement rather than as a pass.
fn wait_for_heartbeat(
    timeline: &Arc<CaseTimeline>,
    probe: &HeartbeatProbe,
    observed_until_us: u64,
) {
    wait_until_us(timeline, observed_until_us, || probe.dequeued().is_some());
}

/// Waits until the storage barrier has published what its parked arrival really served.
///
/// The barrier writes that record when the permit drops - i.e. when the hold really ended - so
/// this wait is what turns "the hold's release instant" into a value read from the layer itself.
fn wait_for_served_hold(
    timeline: &Arc<CaseTimeline>,
    hold: &aura_persistence::barrier::Barrier,
    observed_until_us: u64,
) -> u64 {
    let served = || {
        hold.arrivals()
            .into_iter()
            .filter(|entry| entry.held)
            .map(|entry| entry.hold_micros)
            .max()
            .unwrap_or(0)
    };
    wait_until_us(timeline, observed_until_us, || served() > 0);
    served()
}

/// Whether every parked arrival was still inside its hold at this instant.
///
/// This is the storage barrier's own "not published yet" state, and reading it immediately after
/// a heartbeat was dequeued proves that the dequeue happened strictly before the release: the
/// dequeue is at or before this read, and the arrival is still unpublished after it. That is a
/// stronger statement than comparing two timestamps, and it is the one the case reports.
fn still_inside_hold(hold: &aura_persistence::barrier::Barrier) -> Option<usize> {
    let parked: Vec<_> = hold.arrivals().into_iter().filter(|entry| entry.held).collect();
    (!parked.is_empty() && parked.iter().all(|entry| entry.hold_nanos == 0)).then_some(parked.len())
}

/// The first parked arrival's own instant on the case's clock.
fn first_arrival_us(
    timeline: &Arc<CaseTimeline>,
    hold: &aura_persistence::barrier::Barrier,
) -> Option<u64> {
    hold.arrivals()
        .into_iter()
        .find(|entry| entry.held)
        .map(|entry| timeline.origin_us_of_wall_ms(entry.at_ms))
        .and_then(|arrival| u64::try_from(arrival).ok())
        .filter(|arrival| *arrival > 0)
}

/// Judges one heartbeat from measured instants only.
fn judge_heartbeat(
    probe: &HeartbeatProbe,
    window_ms: u64,
    arrival_us: Option<u64>,
    served_hold_us: u64,
) -> HeartbeatObservation {
    let instants = probe.instants();
    let window_deadline_us = instants.spawn_us.saturating_add(window_ms.saturating_mul(1_000));
    let hold_release_us = arrival_us
        .map(|arrival| arrival.saturating_add(served_hold_us))
        .filter(|release| *release > 0);
    let Some(dequeued_us) = probe.dequeued() else {
        return HeartbeatObservation {
            hold_release_us,
            served_hold_us,
            arrival_us,
            ..HeartbeatObservation::default()
        };
    };
    HeartbeatObservation {
        within_window: dequeued_us <= window_deadline_us,
        before_hold_released: hold_release_us.is_some_and(|release| dequeued_us < release),
        instants,
        hold_release_us,
        served_hold_us,
        arrival_us,
    }
}

/// `YYYY-MM-DDTHH:MM:SS[.fff…](Z|±HH:MM)` to epoch milliseconds.
///
/// The heartbeat frame carries its own `sent_at`, which is the production task's enqueue
/// instant. Reading it needs no dependency: the format is RFC 3339 with a fixed shape, and the
/// only arithmetic is the civil-date conversion below.
fn parse_rfc3339_ms(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        text.get(range).and_then(|part| part.parse::<i64>().ok())
    };
    let year = number(0..4)?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    let fraction_ms = if bytes.get(19) == Some(&b'.') {
        let digits: String = text
            .get(20..)
            .unwrap_or_default()
            .chars()
            .take_while(char::is_ascii_digit)
            .take(3)
            .collect();
        if digits.is_empty() { 0 } else { format!("{digits:0<3}").parse::<i64>().ok()? }
    } else {
        0
    };
    // Howard Hinnant's days_from_civil, so no calendar dependency is needed here.
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    u64::try_from(seconds * 1_000 + fraction_ms).ok()
}

fn session_params(workspace: WorkspaceId, title: &str) -> Value {
    json!({"title": title, "workspace_id": workspace})
}

/// One real session request whose identity the caller keeps.
///
/// The production handler is driven exactly as `call` drives it - same claim entry, same route,
/// same settlement owner - and the ids are returned because the requirement is about *one*
/// invocation: what its own caller was told, what the ledger says about it while the work is
/// parked, and what it says once the real work finished.
#[derive(Clone)]
struct SessionCall {
    request: JsonRpcRequest,
    invocation: CommandInvocationId,
}

impl SessionCall {
    fn new(method: &str, workspace: WorkspaceId, title: &str) -> Self {
        Self::with_params(method, session_params(workspace, title))
    }

    fn with_params(method: &str, params: Value) -> Self {
        let invocation = CommandInvocationId::new();
        Self {
            request: JsonRpcRequest {
                jsonrpc: JSON_RPC_VERSION.into(),
                id: CorrelationId::new(),
                command_invocation_id: Some(invocation),
                method: method.into(),
                params,
            },
            invocation,
        }
    }
}

/// Drives one real session request through the production handler and returns the wire answer.
async fn drive_session(state: Arc<RuntimeState>, call: SessionCall) -> Value {
    let queue = AsyncPriorityQueue::new();
    let claim = match crate::reserve_command_invocation(&state, &queue, &call.request)
        .await
        .expect("the fixture's claim entry")
    {
        crate::CommandReservation::Granted(claim) => claim,
        crate::CommandReservation::Refused => panic!("the fixture refused its claim"),
    };
    handle_request(
        &state,
        &queue,
        &ProjectionSubscriptions::default(),
        &Arc::new(std::sync::Mutex::new(RequestLedger::default())),
        &call.request,
        claim.as_ref(),
    )
    .await
    .expect("the production handler must return a terminal");
    let WireFrame::Control(bytes) = queue.recv().await.expect("the handler must answer").frame
    else {
        panic!("the handler must answer with a control frame")
    };
    serde_json::from_slice(&bytes).expect("response json")
}

/// The authoritative ledger answer for one invocation, through the real entry point.
async fn query_command_outcome(state: Arc<RuntimeState>, invocation: CommandInvocationId) -> Value {
    call(state, method::RUNTIME_COMMAND_OUTCOME, json!({"command_invocation_id": invocation})).await
}

/// How many times one marker appears in a serialized projection.
fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

/// The error envelope of a wire failure, or a panic naming what came back instead.
fn failure_envelope(response: &Value) -> &Value {
    response.get("error").and_then(|error| error.get("data")).unwrap_or_else(|| {
        panic!("a failure envelope was expected, got {response}");
    })
}

fn test_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(TEST_WORKER_THREADS)
        .enable_all()
        .build()
        .expect("test runtime")
}

/// G1 - one real synchronous section is at its hold point; the Core's control lane, its
/// heartbeat and another workspace's real projection read all still happen, and work that
/// really does need the shared writer degrades honestly instead of being faked.
#[test]
fn the_control_lane_heartbeat_and_another_workspace_keep_working_while_a_section_is_stuck() {
    let timeline = CaseTimeline::new();
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    // One real hold, on the real admission path every interactive write goes through. It is
    // a genuinely shared dependency - one SQLite writer - so nothing that needs that writer
    // can be served while it is held. Nothing here pretends otherwise.
    let hold = harness.persistence().arm_admission_hold(Some("sync"), HEARTBEAT_HOLD_MS);
    timeline.mark(format!("hold.armed hold_ms={HEARTBEAT_HOLD_MS}"));

    let (stuck, stuck_elapsed) = observed_call(
        &runtime,
        Arc::clone(&state),
        "session.create",
        session_params(harness.workspace, "stuck A"),
    );
    let (control, control_elapsed) = observed_call(
        &runtime,
        Arc::clone(&state),
        method::PERMISSION_GET_SESSION_STATE,
        json!({"session_id": harness.session}),
    );
    // Another workspace's legitimate projection read: persisted rows through the read pool,
    // which does not need the writer that A is holding. It is real, and it must contain both
    // workspaces - including the one whose write is parked.
    let (projection, projection_elapsed) = observed_call(
        &runtime,
        Arc::clone(&state),
        method::PROJECTION_SNAPSHOT,
        json!({
            "projection_kind": "workspace_session",
            "projection_scope": {"type": "global"},
            "known_runtime_instance_id": null,
        }),
    );
    // The section really reached the hold before the heartbeat is observed, from the storage
    // barrier's own record rather than from a timer: `hold_nanos == 0` is the barrier's own way
    // of saying "this arrival has not been published yet", i.e. the case is inside the hold right
    // now. Waiting for it is what makes "the heartbeat was delivered while a section was parked"
    // a fact about a parked section and not about a likely one.
    let arrived = hold.wait_for_arrivals(1, Duration::from_secs(2));
    timeline.mark(format!("hold.arrived arrived={arrived}"));
    assert!(arrived, "the real session write must reach the storage hold");
    assert!(
        hold.arrivals().iter().any(|entry| entry.held && entry.hold_nanos == 0),
        "the arrival must still be inside its hold while this case observes it"
    );

    let heartbeat = start_heartbeat(&runtime, Arc::clone(&state), &timeline);
    let sentinel = start_runtime_sentinel(&runtime, Duration::from_secs(2), &timeline);
    start_progress_probe(&runtime, &timeline, "rt");
    let os_probe = start_os_probe(&timeline, "os");
    let inject_probe = start_inject_probe(&runtime, &timeline, "inject");
    let (watchdog_stop, watchdog) = start_writer_watchdog(
        &timeline,
        Arc::clone(&harness.persistence()),
        Arc::clone(&state),
        "watch",
    );

    let answered =
        wait_until_us(&timeline, timeline.now_us() + HEARTBEAT_WINDOW_MS * 1_000, || {
            control_elapsed.load(Ordering::Acquire) != u64::MAX
                && projection_elapsed.load(Ordering::Acquire) != u64::MAX
        });
    let control_ms = control_elapsed.load(Ordering::Acquire);
    let projection_ms = projection_elapsed.load(Ordering::Acquire);
    // THE HOLD'S REAL RELEASE INSTANT, from the layer that owns it. The barrier publishes the
    // duration an arrival really served when that arrival's permit drops, so this is a measured
    // value and not the rule "arrival plus the armed hold". The wait is bounded only so a run
    // cannot hang; the hold itself is left to end on its own deadline, exactly as the window's
    // contract says ("the hold's fallback is longer than this window"), which is what leaves a
    // real margin between the heartbeat and the release instead of a coincidence.
    let arrival_us = first_arrival_us(&timeline, &hold);
    let observed_until_us =
        timeline.now_us().saturating_add((HEARTBEAT_WINDOW_MS + LATE_OBSERVATION_MS) * 1_000).max(
            arrival_us
                .unwrap_or(0)
                .saturating_add((HEARTBEAT_HOLD_MS + LATE_OBSERVATION_MS) * 1_000),
        );
    wait_for_heartbeat(&timeline, &heartbeat, observed_until_us);
    let still_parked = still_inside_hold(&hold);
    timeline.mark(format!("hold.still_parked_at_heartbeat parked={still_parked:?}"));
    let served_hold_us = wait_for_served_hold(&timeline, &hold, observed_until_us);
    let observation = judge_heartbeat(&heartbeat, HEARTBEAT_WINDOW_MS, arrival_us, served_hold_us);
    let hold_release_us = observation.hold_release_us;
    let heartbeat_instants = observation.instants;
    let sentinel_instants = sentinel.instants();
    let stuck_ms = stuck_elapsed.load(Ordering::Acquire);
    let timeline_evidence = timeline.evidence(
        "R3-G1",
        json!({
            "answered": answered,
            "control_ms": control_ms,
            "heartbeat": {
                "before_hold_released": observation.before_hold_released,
                "delivered_ms": heartbeat_instants.delivered_ms,
                "dequeued_us": heartbeat_instants.dequeued_us,
                "queued_us": heartbeat_instants.queued_us,
                "spawn_us": heartbeat_instants.spawn_us,
                "task_polled_us": heartbeat_instants.task_polled_us,
                "within_window": observation.within_window,
            },
            "arrival_us": observation.arrival_us,
            "hold_ms": HEARTBEAT_HOLD_MS,
            "hold_release_us": hold_release_us,
            "other_workspace_projection_ms": projection_ms,
            "sentinel": {
                "fired_ms": sentinel_instants.fired_ms,
                "first_poll_us": sentinel_instants.first_poll_us,
                "sleep_created_us": sentinel_instants.sleep_created_us,
                "spawn_us": sentinel_instants.spawn_us,
                "woke_us": sentinel_instants.woke_us,
            },
            "served_hold_us": observation.served_hold_us,
            "stuck_section_caller_ms": stuck_ms,
            "window_ms": HEARTBEAT_WINDOW_MS,
        }),
    );
    // Emitted BEFORE the assertions below, so a case that fails still leaves its whole clock in
    // the run output instead of only the one value the failing assertion happened to name.
    println!("S04TS-TIMELINE {timeline_evidence}");
    println!(
        "R3-G1 hold_ms={HEARTBEAT_HOLD_MS} stuck_section_caller_ms={stuck_ms} \
              control_ms={control_ms} other_workspace_projection_ms={projection_ms} \
              answered={answered} heartbeat_within_window={} \
              heartbeat_before_hold_released={} first_heartbeat_ms={} \
              heartbeat_task_polled_us={} heartbeat_queued_us={} heartbeat_dequeued_us={} \
              hold_release_us={hold_release_us:?} runtime_sentinel_ms={} \
              sentinel_first_poll_us={} sentinel_sleep_created_us={} sentinel_woke_us={} \
              counters={:?}",
        observation.within_window,
        observation.before_hold_released,
        heartbeat_instants.delivered_ms,
        heartbeat_instants.task_polled_us,
        heartbeat_instants.queued_us,
        heartbeat_instants.dequeued_us,
        sentinel_instants.fired_ms,
        sentinel_instants.first_poll_us,
        sentinel_instants.sleep_created_us,
        sentinel_instants.woke_us,
        state.sync.all_counters()
    );
    assert!(
        answered,
        "the control lane and the projection snapshot must answer while the section is parked: \
         control_ms={control_ms} projection_ms={projection_ms}"
    );
    let control_value = runtime.block_on(control).expect("control task");
    assert!(
        control_value.get("error").is_none(),
        "the control lane must answer a real permission state: {control_value}"
    );
    let projection_value = runtime.block_on(projection).expect("projection task");
    assert!(
        projection_value.get("error").is_none(),
        "a real projection read must be served while A's write is parked: {projection_value}"
    );
    for workspace in [harness.workspace, harness.other_workspace] {
        assert!(
            projection_value.to_string().contains(&workspace.to_string()),
            "the projection must still carry workspace {workspace}: {projection_value}"
        );
    }
    assert!(
        projection_value.to_string().contains(&harness.other_session.to_string()),
        "the projection must still carry B's session: {projection_value}"
    );
    assert!(
        control_ms <= BUDGET_MS + 400,
        "the control lane must not wait for the stuck section: {control_ms} ms"
    );
    assert!(
        still_parked.is_some(),
        "the heartbeat must be dequeued while the parked section is still inside its hold: \
         heartbeat_dequeued_us={} hold_release_us={hold_release_us:?} \
         first_heartbeat_ms={} hold_ms={HEARTBEAT_HOLD_MS} (the barrier's own unpublished state \
         read right after the dequeue is what proves this, not a wait that ran long)",
        heartbeat_instants.dequeued_us,
        heartbeat_instants.delivered_ms
    );
    assert!(
        observation.within_window,
        "the heartbeat must be dequeued inside the {HEARTBEAT_WINDOW_MS} ms window measured from \
         the probe's own spawn: first_heartbeat_ms={} heartbeat_spawn_us={} \
         heartbeat_dequeued_us={} hold_release_us={hold_release_us:?} \
         runtime_sentinel_ms={} sentinel_first_poll_us={} sentinel_woke_us={}",
        heartbeat_instants.delivered_ms,
        heartbeat_instants.spawn_us,
        heartbeat_instants.dequeued_us,
        sentinel_instants.fired_ms,
        sentinel_instants.first_poll_us,
        sentinel_instants.woke_us
    );
    // Validity of the measurement above: the independent 2 s timer on the same runtime must have
    // fired at all. It touches none of the Core's state, so if it never fired the window could
    // not be measured here and a green result would be an absence of measurement. Its first poll
    // and its wake are recorded separately, so a late reading says WHICH half was late instead of
    // blaming the host: `first_poll_us` late means this runtime did not schedule the task, and
    // `woke_us - sleep_created_us` late means it did not deliver its timer.
    assert!(
        sentinel_instants.fired_ms != u64::MAX,
        "the independent 2 s timer on the same runtime never fired, so this window was not \
         measured at all: first_heartbeat_ms={} events={}",
        heartbeat_instants.delivered_ms,
        timeline.evidence("R3-G1-events", json!({}))
    );

    // A's own section really was parked: its caller gave up on its own budget while the work
    // was still inside the hold, which is the "unknown, do not resend" answer, not a failure.
    let stuck_value = runtime.block_on(stuck).expect("stuck task");
    let stuck_code = stuck_value
        .pointer("/error/data/code")
        .and_then(Value::as_str)
        .unwrap_or("none")
        .to_owned();
    println!("R3-G1 stuck_section_answer code={stuck_code} value={stuck_value}");
    assert_eq!(
        stuck_code, "command.outcome_unknown",
        "a section that started and did not answer is unknown, never 'did not run': {stuck_value}"
    );

    // Release, for good order: the hold already ended on its own deadline - which is what the
    // measurement above used - so this only guarantees the layer is not left armed.
    hold.release_all();
    timeline.mark("hold.released_by_case");
    let after_release = runtime.block_on(call(
        Arc::clone(&state),
        "session.create",
        session_params(harness.workspace, "after release"),
    ));
    println!("R3-G1 after_release={after_release}");
    assert!(
        after_release.get("error").is_none(),
        "once the shared writer is free again the same write must really happen: {after_release}"
    );
    let spans = state.sync.spans();
    // The parked arrival really served the armed hold, so the release instant the heartbeat was
    // judged against is the barrier's own deadline and not an early release by this case.
    assert!(
        observation.served_hold_us >= HEARTBEAT_HOLD_MS * 1_000 - 50_000,
        "the parked arrival must really serve the armed hold, which is what makes its published \
         release instant the deadline this case judged the heartbeat against: served_hold_us={} \
         hold_ms={HEARTBEAT_HOLD_MS}",
        observation.served_hold_us
    );
    let served_hold_us = observation.served_hold_us;
    let mut extra = timeline_evidence;
    if let Some(fields) = extra.get_mut("extra").and_then(Value::as_object_mut) {
        fields.insert("served_hold_us".to_owned(), json!(served_hold_us));
        fields.insert(
            "spans".to_owned(),
            Value::Array(
                spans
                    .iter()
                    .map(|span| {
                        json!({
                            "caller_waited_ms": span.caller_waited_ms,
                            "elapsed_ms": span.elapsed_ms,
                            "outcome": span.outcome,
                            "section": span.section,
                            "thread": span.thread,
                        })
                    })
                    .collect(),
            ),
        );
    }
    println!("S04TS-TIMELINE-FINAL {extra}");
    let rt_gap = longest_tick_gap(&timeline, "rt");
    let os_gap = longest_tick_gap(&timeline, "os");
    let inject_gap = longest_tick_gap(&timeline, "inject");
    println!(
        "S04TS-PROGRESS R3-G1 rt_gap_us={} (ended_at_us={}) os_gap_us={} (ended_at_us={}) \
         inject_gap_us={} (ended_at_us={})",
        rt_gap.0, rt_gap.1, os_gap.0, os_gap.1, inject_gap.0, inject_gap.1
    );
    for sample in watchdog_samples(&timeline, "watch") {
        println!("S04TS-WATCHDOG R3-G1 {sample}");
    }
    watchdog_stop.store(false, Ordering::Release);
    let _ = watchdog.join();
    let _ = inject_probe.join();
    let _ = os_probe.join();
    heartbeat.join();
}

/// G2 - the boundary really is bounded: a full boundary refuses inside the budget, proves
/// the refused section never ran, keeps authentication and the heartbeat alive, and gives
/// its permits back only when the work that owns them really finishes.
#[test]
fn a_full_boundary_refuses_within_bounds_and_keeps_authentication_and_heartbeat_alive() {
    let timeline = CaseTimeline::new();
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let hold = harness.persistence().arm_admission_hold(Some("sync"), HEARTBEAT_HOLD_MS);
    timeline.mark(format!("hold.armed hold_ms={HEARTBEAT_HOLD_MS}"));
    let workers = state.sync.capacity();
    assert_eq!(workers, SYNC_WORKERS);
    let baseline = state.sync.counters(SyncSection::SessionCommand);

    // Every real worker of the boundary is taken by a real production request.
    let mut occupants = Vec::new();
    for index in 0..workers {
        occupants.push(runtime.spawn(call(
            Arc::clone(&state),
            "session.create",
            session_params(harness.workspace, &format!("occupant {index}")),
        )));
    }
    let filled = wait_until(Duration::from_millis(2_000), || state.sync.available() == 0);
    assert!(filled, "every boundary worker must be occupied by a real request");
    // Every worker is inside its hold right now, from the barrier's own records: four arrivals,
    // none of them published yet.
    let parked = hold.wait_for_arrivals(workers as u64, Duration::from_secs(4));
    timeline.mark(format!("hold.arrived parked={parked}"));
    assert!(parked, "every boundary worker must really be inside the storage hold");
    assert_eq!(
        hold.arrivals().iter().filter(|entry| entry.held && entry.hold_nanos == 0).count(),
        workers,
        "every parked arrival must still be inside its hold while this case observes it"
    );

    let (control, control_elapsed) = observed_call(
        &runtime,
        Arc::clone(&state),
        method::PERMISSION_GET_SESSION_STATE,
        json!({"session_id": harness.session}),
    );
    let heartbeat = start_heartbeat(&runtime, Arc::clone(&state), &timeline);
    let sentinel = start_runtime_sentinel(&runtime, Duration::from_secs(2), &timeline);
    start_progress_probe(&runtime, &timeline, "rt");
    let os_probe = start_os_probe(&timeline, "os");
    let inject_probe = start_inject_probe(&runtime, &timeline, "inject");
    let (watchdog_stop, watchdog) = start_writer_watchdog(
        &timeline,
        Arc::clone(&harness.persistence()),
        Arc::clone(&state),
        "watch",
    );
    // The refused section: one more real session write, on a boundary with no free worker.
    let refused_at = Instant::now();
    let refused = runtime.block_on(call(
        Arc::clone(&state),
        "session.create",
        session_params(harness.workspace, "refused A"),
    ));
    let refused_ms = u64::try_from(refused_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    let counters_at_refusal = state.sync.counters(SyncSection::SessionCommand);
    println!(
        "R3-G2 refused_ms={refused_ms} refused={refused} counters_at_refusal=\
         {counters_at_refusal:?} available={}",
        state.sync.available()
    );
    let code =
        refused.pointer("/error/data/code").and_then(Value::as_str).unwrap_or("none").to_owned();
    assert_eq!(
        code, "persistence.busy",
        "a section refused for want of a worker is a proven 'it did not run': {refused}"
    );
    assert!(
        refused_ms <= BUDGET_MS + 900,
        "the refusal must be bounded by the budget, not by the hold: {refused_ms} ms"
    );
    assert_eq!(
        counters_at_refusal.refused,
        baseline.refused + 1,
        "exactly one session command was refused"
    );
    assert_eq!(
        counters_at_refusal.completed, baseline.completed,
        "the refused section must not have run"
    );
    assert_eq!(
        counters_at_refusal.in_flight, workers as u64,
        "the running sections still own their permits"
    );

    let answered =
        wait_until_us(&timeline, timeline.now_us() + HEARTBEAT_WINDOW_MS * 1_000, || {
            control_elapsed.load(Ordering::Acquire) != u64::MAX
        });
    let control_ms = control_elapsed.load(Ordering::Acquire);
    // Read the boundary's own state at the same instant the window is judged, so the extended
    // heartbeat wait below cannot be confused with a hold that simply expired.
    let available_at_window = state.sync.available();
    // THE HOLD'S REAL RELEASE INSTANT, from the layer that owns it: the barrier publishes the
    // duration its parked arrivals really served when their permits drop, and the hold is left to
    // end on its own deadline (the window's contract says that deadline is longer than the
    // window), so the release is a measured value with a real margin, not "a wait that ran long".
    let arrival_us = first_arrival_us(&timeline, &hold);
    let observed_until_us =
        timeline.now_us().saturating_add((HEARTBEAT_WINDOW_MS + LATE_OBSERVATION_MS) * 1_000).max(
            arrival_us
                .unwrap_or(0)
                .saturating_add((HEARTBEAT_HOLD_MS + LATE_OBSERVATION_MS) * 1_000),
        );
    wait_for_heartbeat(&timeline, &heartbeat, observed_until_us);
    let still_parked = still_inside_hold(&hold);
    timeline.mark(format!("hold.still_parked_at_heartbeat parked={still_parked:?}"));
    let served_hold_us = wait_for_served_hold(&timeline, &hold, observed_until_us);
    let observation = judge_heartbeat(&heartbeat, HEARTBEAT_WINDOW_MS, arrival_us, served_hold_us);
    let hold_release_us = observation.hold_release_us;
    let heartbeat_instants = observation.instants;
    let sentinel_instants = sentinel.instants();
    // Emitted BEFORE the assertions below, so a failing case still leaves its whole clock in the
    // run output instead of only the value the failing assertion happened to name.
    println!(
        "S04TS-TIMELINE {}",
        timeline.evidence(
            "R3-G2-while-parked",
            json!({
                "answered": answered,
                "arrival_us": observation.arrival_us,
                "available_at_window": available_at_window,
                "control_ms": control_ms,
                "heartbeat": {
                    "before_hold_released": observation.before_hold_released,
                    "delivered_ms": heartbeat_instants.delivered_ms,
                    "dequeued_us": heartbeat_instants.dequeued_us,
                    "queued_us": heartbeat_instants.queued_us,
                    "spawn_us": heartbeat_instants.spawn_us,
                    "task_polled_us": heartbeat_instants.task_polled_us,
                    "within_window": observation.within_window,
                },
                "hold_ms": HEARTBEAT_HOLD_MS,
                "hold_release_us": hold_release_us,
                "refused_ms": refused_ms,
                "sentinel": {
                    "fired_ms": sentinel_instants.fired_ms,
                    "first_poll_us": sentinel_instants.first_poll_us,
                    "sleep_created_us": sentinel_instants.sleep_created_us,
                    "spawn_us": sentinel_instants.spawn_us,
                    "woke_us": sentinel_instants.woke_us,
                },
                "served_hold_us": observation.served_hold_us,
                "window_ms": HEARTBEAT_WINDOW_MS,
            }),
        )
    );
    println!(
        "R3-G2 while_full answered={answered} control_ms={control_ms} \
         heartbeat_within_window={} heartbeat_before_hold_released={} first_heartbeat_ms={} \
         heartbeat_task_polled_us={} heartbeat_queued_us={} heartbeat_dequeued_us={} \
         hold_release_us={hold_release_us:?} runtime_sentinel_ms={} sentinel_first_poll_us={} \
         sentinel_woke_us={} available_at_window={available_at_window}",
        observation.within_window,
        observation.before_hold_released,
        heartbeat_instants.delivered_ms,
        heartbeat_instants.task_polled_us,
        heartbeat_instants.queued_us,
        heartbeat_instants.dequeued_us,
        sentinel_instants.fired_ms,
        sentinel_instants.first_poll_us,
        sentinel_instants.woke_us
    );
    assert!(answered, "the control lane must answer while the pool is full");
    assert_eq!(available_at_window, 0, "no held worker has been released by a timer");
    assert!(
        control_ms <= BUDGET_MS + 400,
        "authentication must not wait for the boundary: {control_ms} ms"
    );
    assert!(
        still_parked.is_some(),
        "the heartbeat must be dequeued while every parked worker is still inside its hold: \
         heartbeat_dequeued_us={} hold_release_us={hold_release_us:?} \
         first_heartbeat_ms={} hold_ms={HEARTBEAT_HOLD_MS} (the barrier's own unpublished state \
         read right after the dequeue is what proves this, not a wait that ran long)",
        heartbeat_instants.dequeued_us,
        heartbeat_instants.delivered_ms
    );
    assert!(
        observation.within_window,
        "the heartbeat must be dequeued inside the {HEARTBEAT_WINDOW_MS} ms window measured from \
         the probe's own spawn: first_heartbeat_ms={} heartbeat_spawn_us={} \
         heartbeat_dequeued_us={} hold_release_us={hold_release_us:?} \
         runtime_sentinel_ms={} sentinel_first_poll_us={} sentinel_woke_us={}",
        heartbeat_instants.delivered_ms,
        heartbeat_instants.spawn_us,
        heartbeat_instants.dequeued_us,
        sentinel_instants.fired_ms,
        sentinel_instants.first_poll_us,
        sentinel_instants.woke_us
    );
    // Same validity requirement as R3-G1: the independent 2 s timer on the same runtime must
    // really have fired for this window to have been measured, and its first poll and its wake
    // are separate readings of which half was late.
    assert!(
        sentinel_instants.fired_ms != u64::MAX,
        "the independent 2 s timer on the same runtime never fired, so this window was not \
         measured at all: first_heartbeat_ms={}",
        heartbeat_instants.delivered_ms
    );
    let control_value = runtime.block_on(control).expect("control task");
    assert!(control_value.get("error").is_none(), "{control_value}");

    // Release, for good order: the hold already ended on its own deadline - which is what the
    // measurement above used - and the permits come back only because the real work finished.
    hold.release_all();
    for occupant in occupants {
        let _ = runtime.block_on(occupant);
    }
    let released = wait_until(Duration::from_millis(6_000), || state.sync.available() == workers);
    let session = state.sync.counters(SyncSection::SessionCommand);
    println!(
        "R3-G2 after_release available={} counters={session:?} spans={:?}",
        state.sync.available(),
        state.sync.spans()
    );
    assert!(released, "the boundary must recover once its work really ends");
    assert_eq!(session.in_flight, 0, "every permit is released by the work that held it");
    assert!(
        session.completed >= baseline.completed + workers as u64,
        "the real sections completed and were counted by the threads that ran them: {session:?}"
    );
    let spans = state.sync.spans();
    let completed: Vec<_> = spans
        .iter()
        .filter(|span| span.section == "session_command" && span.outcome == "completed")
        .collect();
    assert!(
        !completed.is_empty() && completed.iter().all(|span| !span.thread.is_empty()),
        "a completed section is attributed to the real thread that ran it"
    );
    let recovered = runtime.block_on(call(
        Arc::clone(&state),
        method::PERMISSION_GET_SESSION_STATE,
        json!({"session_id": harness.session}),
    ));
    assert!(recovered.get("error").is_none(), "{recovered}");
    // The parked arrivals really served the armed hold, so the release instant the heartbeat was
    // judged against is the barrier's own deadline and not an early release by this case.
    let served_hold_us = observation.served_hold_us;
    assert!(
        served_hold_us >= HEARTBEAT_HOLD_MS * 1_000 - 50_000,
        "the parked arrivals must really serve the armed hold, which is what makes their published \
         release instant the deadline this case judged the heartbeat against: served_hold_us=\
         {served_hold_us} hold_ms={HEARTBEAT_HOLD_MS}"
    );
    let evidence = timeline.evidence(
        "R3-G2",
        json!({
            "answered": answered,
            "available_at_window": available_at_window,
            "control_ms": control_ms,
            "counters_at_refusal": {
                "completed": counters_at_refusal.completed,
                "in_flight": counters_at_refusal.in_flight,
                "refused": counters_at_refusal.refused,
            },
            "heartbeat": {
                "before_hold_released": observation.before_hold_released,
                "delivered_ms": heartbeat_instants.delivered_ms,
                "dequeued_us": heartbeat_instants.dequeued_us,
                "queued_us": heartbeat_instants.queued_us,
                "spawn_us": heartbeat_instants.spawn_us,
                "task_polled_us": heartbeat_instants.task_polled_us,
                "within_window": observation.within_window,
            },
            "hold_ms": HEARTBEAT_HOLD_MS,
            "hold_release_us": hold_release_us,
            "refused_ms": refused_ms,
            "sentinel": {
                "fired_ms": sentinel_instants.fired_ms,
                "first_poll_us": sentinel_instants.first_poll_us,
                "sleep_created_us": sentinel_instants.sleep_created_us,
                "spawn_us": sentinel_instants.spawn_us,
                "woke_us": sentinel_instants.woke_us,
            },
            "served_hold_us": served_hold_us,
            "spans": spans
                .iter()
                .map(|span| {
                    json!({
                        "caller_waited_ms": span.caller_waited_ms,
                        "elapsed_ms": span.elapsed_ms,
                        "outcome": span.outcome,
                        "section": span.section,
                        "thread": span.thread,
                    })
                })
                .collect::<Vec<_>>(),
            "window_ms": HEARTBEAT_WINDOW_MS,
        }),
    );
    println!("S04TS-TIMELINE {evidence}");
    let rt_gap = longest_tick_gap(&timeline, "rt");
    let os_gap = longest_tick_gap(&timeline, "os");
    let inject_gap = longest_tick_gap(&timeline, "inject");
    println!(
        "S04TS-PROGRESS R3-G2 rt_gap_us={} (ended_at_us={}) os_gap_us={} (ended_at_us={}) \
         inject_gap_us={} (ended_at_us={})",
        rt_gap.0, rt_gap.1, os_gap.0, os_gap.1, inject_gap.0, inject_gap.1
    );
    for sample in watchdog_samples(&timeline, "watch") {
        println!("S04TS-WATCHDOG R3-G2 {sample}");
    }
    watchdog_stop.store(false, Ordering::Release);
    let _ = watchdog.join();
    let _ = inject_probe.join();
    let _ = os_probe.join();
    heartbeat.join();
}

/// G3 - the discrimination: the same gate, with the boundary injected back to the behaviour
/// it replaced. If the boundary is what keeps the Core answerable, this must fail.
#[test]
fn the_same_gate_fails_when_the_boundary_runs_sections_inline() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    // The injection: sections run on whatever thread calls them, which for a Core request is
    // a reactor worker - exactly what existed before the boundary.
    state.sync.force_inline_for_test(true);
    let hold = harness.persistence().arm_admission_hold(Some("sync"), HOLD_MS);
    let workers = state.sync.capacity();

    let mut occupants = Vec::new();
    for index in 0..workers {
        occupants.push(runtime.spawn(call(
            Arc::clone(&state),
            "session.create",
            session_params(harness.workspace, &format!("inline occupant {index}")),
        )));
    }
    // The inline path takes no permit, so the only way to know the sections really are
    // inside their hold is the boundary's own in-flight count, which the work itself sets.
    let parked = wait_until(Duration::from_millis(1_000), || {
        state.sync.counters(SyncSection::SessionCommand).in_flight == workers as u64
    });
    assert!(parked, "every injected section must really be inside its hold");

    let (control, control_elapsed) = observed_call(
        &runtime,
        Arc::clone(&state),
        method::PERMISSION_GET_SESSION_STATE,
        json!({"session_id": harness.session}),
    );
    let timeline = CaseTimeline::new();
    let heartbeat = start_heartbeat(&runtime, Arc::clone(&state), &timeline);

    // The same window G2 asserted: while the sections are stuck, does anything else run?
    let answered = wait_until_us(&timeline, timeline.now_us() + (HOLD_MS - 200) * 1_000, || {
        control_elapsed.load(Ordering::Acquire) != u64::MAX && heartbeat.dequeued().is_some()
    });
    let control_ms = control_elapsed.load(Ordering::Acquire);
    let heartbeat_ms = heartbeat.instants().delivered_ms;
    println!(
        "R3-G3 inline=true answered_before_hold_ends={answered} control_ms={control_ms} \
         first_heartbeat_ms={heartbeat_ms} spans={:?}",
        state.sync.spans()
    );
    assert!(
        !answered,
        "with the boundary removed the reactor must be starved - if this passes, the \
         boundary is not what the G2 gate measures"
    );
    state.sync.force_inline_for_test(false);
    hold.release_all();
    for occupant in occupants {
        let _ = runtime.block_on(occupant);
    }
    let _ = runtime.block_on(control);
    heartbeat.join();
}

/// Q1-A: a real session write whose caller gave up keeps its result, and the ORIGINAL invocation
/// is what the real late commit settles.
///
/// This is the production chain, not a pool helper: the real handler, the real claim, the real
/// `PersistenceCoordinator` and its SQLite writer, the real storage hold and the real
/// `runtime.command_outcome` entry. Four facts are asserted together, because each of them was
/// separately false before this batch:
///  1. the caller's answer is a typed Unknown that carries the contract's recovery field and the
///     original invocation (so the desktop can recognise it and query it);
///  2. while the work is really parked the authoritative ledger does NOT hold a confirmed
///     failure - it holds the honest Unknown;
///  3. after the release the SAME invocation answers with its real success, i.e. the late result
///     settled the command the caller abandoned;
///  4. the business ran exactly once, and a replay of the same invocation is refused.
#[test]
fn a_late_session_commit_settles_the_invocation_its_caller_abandoned() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    let hold = persistence.arm_admission_hold(Some("sync"), HOLD_MS);
    let title = "late settlement A";
    let call = SessionCall::new("session.create", harness.workspace, title);
    let invocation = call.invocation;
    let correlation = call.request.id;
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call.clone()));

    let arrived = hold.wait_for_arrivals(1, Duration::from_secs(2));
    let response = runtime.block_on(driver).expect("the driven request");

    // 1. The caller was told "started, not settled" - by contract field, not by wording.
    let envelope = failure_envelope(&response);
    println!("Q1-A caller_answer={response}");
    assert_eq!(
        envelope.pointer("/code").and_then(Value::as_str),
        Some("command.outcome_unknown"),
        "a started write must be answered with the unknown code: {response}"
    );
    assert_eq!(envelope.pointer("/terminal").and_then(Value::as_str), Some("failed"));
    assert_eq!(
        envelope.pointer("/details/outcome_recovery").and_then(Value::as_str),
        Some("query_runtime.command_outcome_without_replay"),
        "the answer must carry the contract's own recovery field: {response}"
    );
    assert_eq!(
        envelope
            .pointer("/details/command_invocation_id")
            .and_then(|value| serde_json::from_value::<CommandInvocationId>(value.clone()).ok()),
        Some(invocation),
        "the answer must carry the invocation it is about: {response}"
    );

    // 2. While the write is really parked, the session does not exist and the ledger says Unknown.
    let projection_while_held = runtime
        .block_on(async { persistence.workspace_session_projection() })
        .map(|value| serde_json::to_string(&value).expect("projection json"))
        .expect("the projection is readable while the write is parked");
    assert_eq!(
        occurrences(&projection_while_held, title),
        0,
        "the parked write must not have committed yet"
    );
    let held = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    println!("Q1-A ledger_while_held={held}");
    assert_eq!(
        held.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("unknown"),
        "an in-flight write must never be recorded as a confirmed failure: {held}"
    );

    // A replay of the same invocation is refused and runs nothing.
    let replay = {
        let queue = AsyncPriorityQueue::new();
        let again = SessionCall::new("session.create", harness.workspace, "replay must not run");
        let mut repeated = again.request.clone();
        repeated.command_invocation_id = Some(invocation);
        let reservation = runtime
            .block_on(crate::reserve_command_invocation(&state, &queue, &repeated))
            .expect("the claim entry");
        let refused = matches!(reservation, crate::CommandReservation::Refused);
        let answer: Value = runtime.block_on(async {
            let WireFrame::Control(bytes) = queue.recv().await.expect("a refusal answer").frame
            else {
                panic!("control frame expected")
            };
            serde_json::from_slice(&bytes).expect("refusal json")
        });
        (refused, answer)
    };
    println!("Q1-A replay_refused={}", replay.1);
    assert!(replay.0, "a replay of an in-flight invocation must be refused");
    assert_eq!(
        replay.1.pointer("/error/data/code").and_then(Value::as_str),
        Some("ipc.command_invocation_in_flight"),
        "the refusal must say the original attempt is still executing: {}",
        replay.1
    );

    // 3. Release: the SAME request really commits, and the SAME invocation now answers with it.
    hold.release_all();
    let committed = wait_until(Duration::from_secs(6), || {
        persistence
            .workspace_session_projection()
            .ok()
            .and_then(|value| serde_json::to_string(&value).ok())
            .is_some_and(|value| occurrences(&value, title) == 1)
    });
    assert!(committed, "the parked write must really commit after the release");
    let settled = wait_until(Duration::from_secs(6), || {
        let answer = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
        answer.pointer("/result/value/outcome").and_then(Value::as_str) == Some("confirmed")
    });
    let authoritative = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    println!("Q1-A ledger_after_release={authoritative}");
    assert!(settled, "the late result must settle the original invocation: {authoritative}");
    assert_eq!(
        authoritative.pointer("/result/value/terminal").and_then(Value::as_str),
        Some("succeeded"),
        "the settled terminal must be the real one: {authoritative}"
    );
    assert_eq!(
        authoritative
            .pointer("/result/value/response/id")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<CorrelationId>().ok()),
        Some(correlation),
        "the settled response must be addressed to the original request: {authoritative}"
    );

    // 4. Exactly one business effect, and the owner released its record.
    let projection_after = runtime
        .block_on(async { persistence.workspace_session_projection() })
        .map(|value| serde_json::to_string(&value).expect("projection json"))
        .expect("projection after the release");
    assert_eq!(occurrences(&projection_after, title), 1, "the business must have run exactly once");
    assert!(arrived, "the write must have reached the real storage hold");
    assert_eq!(state.settlements.pending(), 0, "the settlement record was released");
}

/// Q1-B: the SAME gate with the ownership injected away.
///
/// With the completion owner made inert - the behaviour that existed when the timeout dropped the
/// blocking handle and nothing received its value - the real session still commits, but the
/// original invocation is never settled. That difference is what makes Q1-A a measurement of the
/// ownership rather than of timing.
#[test]
fn the_late_result_is_what_settles_the_invocation_and_nothing_else_does() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    state.settlements.force_inert_for_test(true);
    let hold = persistence.arm_admission_hold(Some("sync"), HOLD_MS);
    let title = "late settlement without owner";
    let call = SessionCall::new("session.create", harness.workspace, title);
    let invocation = call.invocation;
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call.clone()));
    let _arrived = hold.wait_for_arrivals(1, Duration::from_secs(2));
    let response = runtime.block_on(driver).expect("the driven request");
    assert_eq!(
        failure_envelope(&response).pointer("/code").and_then(Value::as_str),
        Some("command.outcome_unknown")
    );
    hold.release_all();
    let committed = wait_until(Duration::from_secs(6), || {
        persistence
            .workspace_session_projection()
            .ok()
            .and_then(|value| serde_json::to_string(&value).ok())
            .is_some_and(|value| occurrences(&value, title) == 1)
    });
    assert!(committed, "the injected behaviour must not stop the business from committing");
    let authoritative = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    println!("Q1-B ledger_without_owner={authoritative}");
    assert_eq!(
        authoritative.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("unknown"),
        "without an owner the late commit cannot settle the invocation - which is exactly the \
         defect this gate measures: {authoritative}"
    );
    state.settlements.force_inert_for_test(false);
}

/// Q1-C: a real late FAILURE settles as that failure, and never as a success.
///
/// The parked work is a real `session.rename` of a session that does not exist, so when the hold
/// is released the store really answers "no such row". The caller is told Unknown before that
/// happens; only after the real business and its settlement finish does the invocation become a
/// confirmed failure, and the error in the ledger is the store's own.
#[test]
fn a_late_real_failure_settles_as_that_failure_and_never_as_success() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    let absent = SessionId::new();
    let hold = persistence.arm_admission_hold(Some("sync"), HOLD_MS);
    let call = SessionCall::with_params(
        "session.rename",
        json!({"session_id": absent, "title": "never applies", "expected_title": "whatever"}),
    );
    let invocation = call.invocation;
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call.clone()));
    let arrived = hold.wait_for_arrivals(1, Duration::from_secs(2));
    let response = runtime.block_on(driver).expect("the driven request");
    println!("Q1-C caller_answer={response} arrived={arrived}");
    assert_eq!(
        failure_envelope(&response).pointer("/code").and_then(Value::as_str),
        Some("command.outcome_unknown"),
        "the parked rename must be answered as started-and-unsettled: {response}"
    );
    let held = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    assert_eq!(
        held.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("unknown"),
        "while the work is parked the ledger stays Unknown: {held}"
    );
    hold.release_all();
    let settled = wait_until(Duration::from_secs(6), || {
        let answer = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
        answer.pointer("/result/value/outcome").and_then(Value::as_str) == Some("confirmed")
    });
    let authoritative = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    println!("Q1-C ledger_after_release={authoritative}");
    assert!(settled, "the real late failure must settle the invocation: {authoritative}");
    assert_eq!(
        authoritative.pointer("/result/value/terminal").and_then(Value::as_str),
        Some("failed"),
        "a real late failure must settle as a failure: {authoritative}"
    );
    assert!(
        authoritative.pointer("/result/value/response/error/data/code").and_then(Value::as_str)
            != Some("command.outcome_unknown"),
        "the settled error must be the real failure the business produced, not the caller's own \
         unknown: {authoritative}"
    );
}

/// Q1-D: at the settlement cap the refusal happens BEFORE the business runs.
///
/// The cap is filled with real parked writes, and the next request must be refused with the
/// definite "it did not run" answer - never started with nobody to settle it. This is the positive
/// that keeps the Unknown fix from demoting every failure to Unknown.
#[test]
fn a_settlement_cap_refusal_is_a_definite_not_run_answer() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    // Every real worker is parked, so the writes below really start and really stay unresolved.
    let hold = persistence.arm_admission_hold(Some("sync"), HOLD_MS);
    let workers = state.sync.capacity();
    let mut occupants = Vec::new();
    for index in 0..workers {
        let call =
            SessionCall::new("session.create", harness.workspace, &format!("cap occupant {index}"));
        occupants.push(call.invocation);
        let _occupant_driver = runtime.spawn(drive_session(Arc::clone(&state), call));
    }
    let filled = wait_until(Duration::from_millis(3_000), || {
        state.settlements.pending() == workers && state.sync.available() == 0
    });
    assert!(filled, "every settlement slot and worker must really be occupied");
    // Fill the remaining slots directly, so the request under test is the one refused at the cap.
    let cap = crate::command_settlement::MAX_PENDING_SETTLEMENTS;
    let mut held = Vec::new();
    for _ in workers..cap {
        let invocation = CommandInvocationId::new();
        let ticket = state
            .settlements
            .begin(invocation, CorrelationId::new(), "session.create")
            .expect("a settlement slot below the cap");
        held.push(ticket);
    }
    assert_eq!(state.settlements.pending(), cap);
    let refused_call =
        SessionCall::new("session.create", harness.workspace, "must not run at the cap");
    let response = runtime.block_on(drive_session(Arc::clone(&state), refused_call.clone()));
    let envelope = failure_envelope(&response);
    println!("Q1-D cap_refusal={response}");
    assert_eq!(
        envelope.pointer("/code").and_then(Value::as_str),
        Some("persistence.busy"),
        "a refusal at the cap is a definite not-run answer: {response}"
    );
    assert_eq!(envelope.pointer("/retryable").and_then(Value::as_bool), Some(true));
    assert!(
        envelope.pointer("/details/outcome_recovery").is_none(),
        "a refusal must not carry the query-instead-of-retry instruction: {response}"
    );
    let projection = runtime
        .block_on(async { persistence.workspace_session_projection() })
        .map(|value| serde_json::to_string(&value).expect("projection json"))
        .expect("projection");
    assert_eq!(
        occurrences(&projection, "must not run at the cap"),
        0,
        "the refused write must not have run"
    );
    // The invocation that was refused is a recorded DEFINITE failure: nothing was attempted, so
    // the existing definite-failure semantics still apply. This is the positive that keeps the
    // Unknown fix from demoting every failure to Unknown.
    let after_refusal =
        runtime.block_on(query_command_outcome(Arc::clone(&state), refused_call.invocation));
    println!("Q1-D refused_invocation_record={after_refusal}");
    assert_eq!(
        after_refusal.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("confirmed"),
        "a proven pre-execution refusal is a definite result, not an Unknown: {after_refusal}"
    );
    assert_eq!(
        after_refusal.pointer("/result/value/terminal").and_then(Value::as_str),
        Some("failed"),
        "{after_refusal}"
    );
    assert_eq!(
        after_refusal.pointer("/result/value/response/error/data/code").and_then(Value::as_str),
        Some("persistence.busy"),
        "the recorded error is the refusal the caller really received: {after_refusal}"
    );
    assert!(
        !held.is_empty(),
        "the cap had to be filled with slots that are really claimed, not assumed"
    );
    hold.release_all();
    let released =
        wait_until(Duration::from_secs(8), || state.settlements.pending() <= cap - workers);
    println!(
        "Q1-D after_release pending={} of cap={cap} occupants={}",
        state.settlements.pending(),
        occupants.len()
    );
    assert!(
        released,
        "the records of the work that really finished are released once it really finished"
    );
    // The direct slots below the cap never received an ending, and nothing else may release them:
    // their work never ran, so their record can only be reaped by its own completion budget, and
    // until then their invocations stay claimed. A record released early would be the same defect
    // as a permit freed while its work still holds it.
    assert_eq!(
        state.settlements.pending(),
        cap - workers,
        "a record whose operation delivered no ending must not be released by anything else"
    );
}

/// How many endings the real broker ledger holds.
///
/// `execution_results` is written by exactly one step - the port's completion after the evidence -
/// so an unchanged count is a measurement that this step did not run, rather than an assumption
/// about it. The count is read from the same SQLite file the product uses.
fn recorded_completions(state: &RuntimeState) -> i64 {
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must be ready");
    };
    let store = services.broker.lock().expect("the broker store lock");
    store
        .database
        .connection()
        .query_row("SELECT count(*) FROM execution_results", [], |row| row.get::<_, i64>(0))
        .expect("the ledger count")
}

/// Q1-E (CORRECTED): the real evidence step of a real write is held, and the SAME invocation is
/// what the late evidence settles once it really finishes.
///
/// The chain is the production one, not a helper: the real `handle_request` entry, the real
/// `fs.write_file` route (`broker/files.rs::dispatch_files`), the real broker claim, the real
/// `PersistenceCoordinator` and its SQLite writer. The handler really writes the file, and then the
/// evidence step - the real post-state read plus `workspace_broker_evidence`, which is the
/// coordinator's own Sync-class dispatch - really reaches the storage layer's admission hold and
/// does not answer inside the section budget. That is the live case the requirement names:
///
///   * the bytes the handler wrote are on this machine's disk;
///   * the caller is told `command.outcome_unknown`, never the `persistence.busy` refusal the same
///     boundary produces for a step that provably did not run;
///   * the Evidence section's own counters and span say it started and did not answer, so the
///     Unknown is about this step rather than about timing elsewhere;
///   * while it is held the ledger holds no confirmed result and no confirmed failure;
///   * the permit belongs to the work: it comes back when the evidence really finished, not when
///     the caller stopped waiting;
///   * AFTER the release the SAME invocation is settled with the handler's real value, its real
///     terminal and the original correlation id, its necessary completion is written exactly once,
///     and the file was never written a second time.
///
/// The first five facts are the hold; the last one is the whole point: a caller's timeout is a fact
/// about one waiter, and the earlier version of this test ended by asserting that the invocation
/// must NOT become confirmed after the release - which is exactly the defect, not a property.
#[test]
fn a_real_write_whose_evidence_step_is_held_keeps_its_effect_and_settles_its_original_invocation() {
    /// The evidence read is the coordinator's Sync-class dispatch, so this is the label its own
    /// arrival carries at the storage layer's admission point.
    const EVIDENCE_LABEL: &str = "sync";
    /// Longer than the section budget below and short enough not to be a production wait.
    const EVIDENCE_HOLD_MS: u64 = 1_500;

    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    let root = harness._fixture.root.path().join("ws-a");
    let relative = "q1e-evidence-step.txt";
    let content = "written before the evidence step could not be collected\n";
    let written = root.join(relative);
    assert!(!written.exists(), "the designated write must start from an absent path");

    let completions_before = recorded_completions(&state);
    let evidence_before = state.sync.counters(SyncSection::Evidence);

    // The hold the evidence step's own read reaches, armed BEFORE the request so that which case
    // parks is decided by the storage layer's own arrival record rather than by where a test slept.

    let call = SessionCall::with_params(
        method::FS_WRITE_FILE,
        json!({
            "workspace_id": harness.workspace,
            "session_id": harness.session,
            "target": { "path": relative },
            "content": content,
            "expected": { "expected_kind": "absent" },
            "encoding": "utf8",
            "line_ending": "lf"
        }),
    );
    let invocation = call.invocation;
    let hold = persistence.arm_admission_hold_selected(
        Some(EVIDENCE_LABEL),
        &aura_persistence::PersistenceCoordinator::broker_evidence_identity(call.request.id),
        EVIDENCE_HOLD_MS,
    );
    let correlation = call.request.id;
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call.clone()));
    let arrived = hold.wait_for_arrivals(1, Duration::from_secs(4));
    let response = runtime.block_on(driver).expect("the driven request");
    let envelope = failure_envelope(&response);
    println!(
        "Q1-E caller_answer={response} arrived={arrived} evidence={:?}",
        state.sync.counters(SyncSection::Evidence)
    );

    // 1. The handler really ran and really wrote: the bytes are on disk, on this machine.
    assert!(arrived, "the storage layer must record a real arrival at the armed hold");
    let on_disk = std::fs::read(&written).expect("the handler's real effect must be on disk");
    assert_eq!(
        on_disk,
        content.as_bytes(),
        "the effect the handler really made must be preserved exactly"
    );

    // 2. The answer is the honest Unknown by contract field - never the pre-dispatch refusal the
    //    same boundary produces for a step that did not run.
    assert_eq!(
        envelope.pointer("/code").and_then(Value::as_str),
        Some("command.outcome_unknown"),
        "a write whose end cannot be established must be answered as unknown: {response}"
    );
    assert_ne!(
        envelope.pointer("/code").and_then(Value::as_str),
        Some("persistence.busy"),
        "an executed handler must never be reported with the not-run refusal: {response}"
    );
    assert_eq!(
        envelope.pointer("/details/outcome_recovery").and_then(Value::as_str),
        Some("query_runtime.command_outcome_without_replay"),
        "the answer must carry the contract's own recovery instruction: {response}"
    );

    // 3. The step that did not answer is the EVIDENCE step, measured by the boundary's own record.
    assert!(
        wait_until(Duration::from_millis(800), || {
            state.sync.counters(SyncSection::Evidence).unknown == evidence_before.unknown + 1
        }),
        "the evidence wait must cross its budget while its work remains held"
    );
    let evidence_after = state.sync.counters(SyncSection::Evidence);
    assert_eq!(
        evidence_after.unknown,
        evidence_before.unknown + 1,
        "the evidence section is the one that did not answer in its budget: {evidence_after:?}"
    );
    assert_eq!(
        evidence_after.completed, evidence_before.completed,
        "the evidence section must not be counted as completed: {evidence_after:?}"
    );
    assert_eq!(
        evidence_after.in_flight, 1,
        "the parked evidence work still owns its permit after its caller stopped waiting: \
         {evidence_after:?}"
    );
    let evidence_spans: Vec<_> = state
        .sync
        .spans()
        .into_iter()
        .filter(|span| span.section == "evidence" && span.outcome == "unknown")
        .collect();
    assert_eq!(
        evidence_spans.len(),
        1,
        "one evidence span must record the unanswered step, with the request it belonged to"
    );
    assert!(
        !evidence_spans[0].request.as_deref().unwrap_or_default().is_empty(),
        "the unanswered evidence step is attributed to the request it belonged to"
    );

    // 4. The completion step really did not run, and the ledger never says the write failed.
    assert_eq!(
        recorded_completions(&state),
        completions_before,
        "no completion may be written for an attempt whose evidence was never collected"
    );
    let held = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    println!("Q1-E ledger_while_held={held}");
    assert_eq!(
        held.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("unknown"),
        "a write whose effect is real but unproven must stay unknown, never a confirmed failure: \
         {held}"
    );

    // 5. Release: the parked evidence read finishes under its own permit, and the SAME invocation
    //    is settled with the handler's real result - exactly once.
    //
    //    The hold is ended a moment after the caller's budget really expired, so that "the section
    //    did not answer inside its budget" is a measured fact rather than a race with the release.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        state.sync.counters(SyncSection::Evidence).completed,
        evidence_before.completed,
        "the evidence work is still parked when the release happens"
    );
    hold.release_all();
    let finished = wait_until(Duration::from_secs(6), || {
        state.sync.counters(SyncSection::Evidence).completed == evidence_before.completed + 1
    });
    println!(
        "Q1-E after_release evidence={:?} available={}",
        state.sync.counters(SyncSection::Evidence),
        state.sync.available()
    );
    assert!(finished, "the parked evidence work must really finish once the hold ends");
    let after_release = std::fs::read(&written).expect("the effect is still there afterwards");
    assert_eq!(
        after_release,
        content.as_bytes(),
        "a second execution of the same command could not produce this file: it was asked for an \
         absent path, so the bytes on disk are one write"
    );

    // THE ORIGINAL RESULT IS EVENTUALLY QUERYABLE. This is the assertion a version without the
    // continuation fails on: the work finished, but nothing settled the invocation it belonged to.
    let settled = wait_until(Duration::from_secs(8), || {
        let answer = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
        answer.pointer("/result/value/outcome").and_then(Value::as_str) == Some("confirmed")
    });
    let authoritative = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    println!("Q1-E ledger_after_release={authoritative}");
    assert!(settled, "the original result must eventually be queryable: {authoritative}");
    assert_eq!(
        authoritative.pointer("/result/value/terminal").and_then(Value::as_str),
        Some("succeeded"),
        "the settled terminal must be the one the handler really produced: {authoritative}"
    );
    assert_eq!(
        authoritative
            .pointer("/result/value/response/id")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<CorrelationId>().ok()),
        Some(correlation),
        "the settled response must be addressed to the original request: {authoritative}"
    );
    assert_eq!(
        authoritative
            .pointer("/result/value/response/result/value/affected_paths/0")
            .and_then(Value::as_str),
        Some(relative),
        "the settled result must carry the value the handler really produced: {authoritative}"
    );
    // The necessary completion is durable and written exactly once.
    assert_eq!(
        recorded_completions(&state),
        completions_before + 1,
        "the original Attempt's completion is written exactly once: {:?}",
        recorded_completions(&state)
    );
    let settled_permits =
        wait_until(Duration::from_secs(4), || state.sync.available() == state.sync.capacity());
    assert!(
        settled_permits,
        "every permit comes back with the work that owned it: available={}",
        state.sync.available()
    );
    assert_eq!(
        state.settlements.pending(),
        0,
        "the completion owner released its slot on the real completion"
    );
}

/// S03F-D1: the same gate with the continuation injected away.
///
/// With the post-execution continuation discarded - the behaviour that existed when the caller's
/// timeout dropped the remainder of the chain - the evidence work still finishes and the effect is
/// still on disk, but the invocation it belonged to is never settled. That difference is what makes
/// the corrected regression above a measurement of the ownership rather than of timing, and it is
/// the failing case the first pre-fix run is frozen from.
#[test]
fn the_held_evidence_does_not_settle_its_invocation_when_the_continuation_is_injected_away() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    state.post_execution.force_no_continuation_for_test(true);
    let root = harness._fixture.root.path().join("ws-a");
    let relative = "s03f-injected-no-continuation.txt";
    let content = "written once, settled never (injected)\n";
    let written = root.join(relative);
    let completions_before = recorded_completions(&state);
    let evidence_before = state.sync.counters(SyncSection::Evidence);
    let call = SessionCall::with_params(
        method::FS_WRITE_FILE,
        json!({
            "workspace_id": harness.workspace,
            "session_id": harness.session,
            "target": { "path": relative },
            "content": content,
            "expected": { "expected_kind": "absent" },
            "encoding": "utf8",
            "line_ending": "lf"
        }),
    );
    let invocation = call.invocation;
    let hold = persistence.arm_admission_hold_selected(
        Some("sync"),
        &aura_persistence::PersistenceCoordinator::broker_evidence_identity(call.request.id),
        1_200,
    );
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call.clone()));
    let arrived = hold.wait_for_arrivals(1, Duration::from_secs(4));
    let response = runtime.block_on(driver).expect("the driven request");
    assert!(arrived, "the evidence read must really reach the storage hold");
    assert_eq!(
        failure_envelope(&response).pointer("/code").and_then(Value::as_str),
        Some("command.outcome_unknown"),
        "the caller's answer is the same honest Unknown: {response}"
    );
    assert_eq!(
        std::fs::read(&written).expect("the handler's effect"),
        content.as_bytes(),
        "the effect is real whether or not anything settles it"
    );

    // The hold is ended a moment after the caller's budget really expired, so the parked work
    // finishing is not a race with the release.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        state.sync.counters(SyncSection::Evidence).completed,
        evidence_before.completed,
        "the evidence work is still parked when the release happens"
    );
    hold.release_all();
    let finished = wait_until(Duration::from_secs(6), || {
        state.sync.counters(SyncSection::Evidence).completed == evidence_before.completed + 1
    });
    assert!(finished, "the parked evidence work really finished even in the injected case");
    let authoritative = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    println!("S03F-D1 ledger_without_continuation={authoritative}");
    assert_eq!(
        authoritative.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("unknown"),
        "WITHOUT A CONTINUATION THE ORIGINAL RESULT IS NOT QUERYABLE - which is exactly the defect \
         this gate measures: {authoritative}"
    );
    assert_eq!(
        recorded_completions(&state),
        completions_before,
        "nothing wrote the completion either"
    );
    state.post_execution.force_no_continuation_for_test(false);
}

/// S03F-C2: a write that is really running inside the persistence writer still settles its
/// original invocation when it commits late.
///
/// The hold is on the **writer thread, after the job was taken off the queue** - not on admission
/// and not on an outer wrapper - so the job really is admitted and really is executing while the
/// caller's own section budget ends. The caller is told Unknown; the write commits when its own
/// hold ends; the same invocation is then settled with the session the store really wrote. The
/// assertion is the settled *value*, never pool idleness.
#[test]
fn a_write_that_is_really_running_inside_the_writer_still_settles_its_invocation_late() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    let hold = persistence.arm_writer_hold_once(
        aura_persistence::barrier::Point::WriterExecution,
        Some("sync"),
        1_200,
    );
    let title = "S03F late commit inside the writer";
    let call = SessionCall::new("session.create", harness.workspace, title);
    let invocation = call.invocation;
    let correlation = call.request.id;
    let completions_before = recorded_completions(&state);
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call.clone()));
    let arrived = hold.wait_for_arrivals(1, Duration::from_secs(4));
    let response = runtime.block_on(driver).expect("the driven request");
    let health = persistence.writer_health();
    println!("S03F-C2 caller_answer={response} arrived={arrived} writer={health:?}");

    assert!(arrived, "the real writer must have taken the job off its queue");
    assert!(
        health.running >= 1,
        "the real job is admitted and running inside the writer, not merely held outside it: \
         {health:?}"
    );
    assert_eq!(
        failure_envelope(&response).pointer("/code").and_then(Value::as_str),
        Some("command.outcome_unknown"),
        "the caller whose budget ended is told Unknown: {response}"
    );
    assert_ne!(
        failure_envelope(&response).pointer("/code").and_then(Value::as_str),
        Some("persistence.busy"),
        "a write that was admitted is never reported as not dispatched: {response}"
    );
    let held = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    assert_eq!(
        held.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("unknown"),
        "while the job is still inside the writer the ledger holds no verdict: {held}"
    );
    let projection_while_held = runtime
        .block_on(async { persistence.workspace_session_projection() })
        .map(|value| serde_json::to_string(&value).expect("projection json"))
        .expect("the projection is readable while the write is parked");
    assert_eq!(
        occurrences(&projection_while_held, title),
        0,
        "the parked job must not have committed yet"
    );

    // The job's own hold ends; it commits under the caller's abandoned wait.
    let settled = wait_until(Duration::from_secs(8), || {
        let answer = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
        answer.pointer("/result/value/outcome").and_then(Value::as_str) == Some("confirmed")
    });
    let authoritative = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    println!("S03F-C2 ledger_after_release={authoritative}");
    assert!(settled, "the late real commit must settle the original invocation: {authoritative}");
    assert_eq!(
        authoritative.pointer("/result/value/terminal").and_then(Value::as_str),
        Some("succeeded")
    );
    assert_eq!(
        authoritative
            .pointer("/result/value/response/id")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<CorrelationId>().ok()),
        Some(correlation),
        "the settled response is addressed to the original request: {authoritative}"
    );
    assert_eq!(
        authoritative.pointer("/result/value/response/result/value/title").and_then(Value::as_str),
        Some(title),
        "the settled value is the session the store really wrote: {authoritative}"
    );
    assert_ne!(
        authoritative
            .pointer("/result/value/response/result/value/session_id")
            .and_then(Value::as_str),
        None,
        "the settled value carries the real session identity: {authoritative}"
    );
    let projection_after = runtime
        .block_on(async { persistence.workspace_session_projection() })
        .map(|value| serde_json::to_string(&value).expect("projection json"))
        .expect("projection after the release");
    assert_eq!(occurrences(&projection_after, title), 1, "the business ran exactly once");
    assert_eq!(
        recorded_completions(&state),
        completions_before,
        "the session route's completion is the outcome row, not a second broker completion"
    );
    assert_eq!(state.settlements.pending(), 0, "the owner released its slot");
}

/// S03F-C3: when the necessary settlement genuinely cannot be established, the invocation stays an
/// honest `Unknown` and the real teardown fact is recorded - the effect is never described as a
/// failure and the business is never redone.
///
/// The evidence step's own read is parked **before** admission; closing the writer while it is
/// parked turns it into the store's own definite "the writer is gone" answer, which is a different
/// fact from the writer's "admitted, not answered" one. The tail must not read that as "established
/// failure material" and must not fabricate a terminal.
#[test]
fn an_evidence_step_that_can_never_be_established_stays_an_honest_unknown_with_a_teardown_fact() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    let root = harness._fixture.root.path().join("ws-a");
    let relative = "s03f-unresolved-evidence.txt";
    let content = "the effect is real, the settlement is not establishable\n";
    let written = root.join(relative);
    let completions_before = recorded_completions(&state);
    let unresolved_before = state.post_execution.facts().unresolved;
    let call = SessionCall::with_params(
        method::FS_WRITE_FILE,
        json!({
            "workspace_id": harness.workspace,
            "session_id": harness.session,
            "target": { "path": relative },
            "content": content,
            "expected": { "expected_kind": "absent" },
            "encoding": "utf8",
            "line_ending": "lf"
        }),
    );
    let invocation = call.invocation;
    let hold = persistence.arm_admission_hold_selected(
        Some("sync"),
        &aura_persistence::PersistenceCoordinator::broker_evidence_identity(call.request.id),
        2_500,
    );
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call.clone()));
    let arrived = hold.wait_for_arrivals(1, Duration::from_secs(4));
    let response = runtime.block_on(driver).expect("the driven request");
    assert!(arrived, "the evidence read must really reach the storage hold");
    assert_eq!(
        failure_envelope(&response).pointer("/code").and_then(Value::as_str),
        Some("command.outcome_unknown")
    );
    assert_eq!(std::fs::read(&written).expect("the handler's real effect"), content.as_bytes());

    // The store is torn down while the evidence read is parked before admission, so when it is let
    // through it meets a definite "no writer" answer rather than a late value.
    let health = persistence.close_writer(Duration::from_secs(2));
    println!("S03F-C3 writer_after_close={health:?}");
    hold.release_all();
    let recorded = wait_until(Duration::from_secs(8), || {
        state.post_execution.facts().unresolved > unresolved_before
    });
    let facts = state.post_execution.facts();
    println!("S03F-C3 facts={facts:?}");
    assert!(
        recorded,
        "the tail must record the real teardown fact instead of waiting forever: {facts:?}"
    );
    let authoritative = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    assert_eq!(
        authoritative.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("unknown"),
        "an unestablished settlement stays an honest Unknown - never a fabricated terminal: \
         {authoritative}"
    );
    assert_eq!(
        recorded_completions(&state),
        completions_before,
        "no completion may be invented for an attempt whose evidence was never established"
    );
    assert_eq!(
        std::fs::read(&written).expect("the effect is preserved"),
        content.as_bytes(),
        "the business must not be redone to make the settlement easy"
    );
    assert_eq!(
        state.settlements.pending(),
        0,
        "the owner gave its slot back on a real teardown fact, keeping the Unknown row"
    );
}

/// S03F-C4: the persistence writer's **own** wait deadline fires while the real job is admitted and
/// still executing, and the value that job commits is still received - by the invocation that
/// asked for it.
///
/// The hold is on the writer thread, after it took the job off the queue, so the job really is
/// admitted and really is executing - not an outer wrapper. The storage layer's own budget is then
/// what expires, which is its structured "admitted, not answered" answer, and the business call
/// underneath it receives that answer while the write may still commit.
///
/// WHAT THE CALLER IS TOLD. The caller's own section budget ends first, so it is told the Core's
/// own "started, not settled" answer with the contract's recovery field - never the ordinary
/// `broker.dispatch_unavailable` and never a confirmed failure. That part is unchanged.
///
/// WHAT CHANGED, AND WHY THIS CASE IS THE POINT. The writer's own deadline is not the job's
/// deadline: the job kept running and committed. The value it produced is the answer to the
/// invocation that asked for it, so it is received from the thread that produced it and settles
/// that same invocation - instead of being thrown away because the caller's wait had ended. The
/// earlier revision of this case asserted that the honest `Unknown` must be *kept*; that assertion
/// was the defect written down, and it is replaced here by "the original invocation is settled with
/// the session the store really wrote". The case that keeps the honest `Unknown` is the one below,
/// where the value genuinely never arrives inside the receipt window.
///
/// The storage budget is shortened **for this coordinator instance only**; the production defaults
/// are asserted unchanged.
#[test]
fn the_writers_own_deadline_firing_first_still_receives_the_value_the_store_committed() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    assert_eq!(
        aura_persistence::DEFAULT_SYNC_DEADLINE,
        Duration::from_secs(30),
        "the production storage budget is not this batch's to change"
    );
    assert_eq!(
        persistence.sync_deadline(),
        aura_persistence::DEFAULT_SYNC_DEADLINE,
        "a fresh coordinator really uses the production budget"
    );
    assert_eq!(
        aura_persistence::DEFAULT_SETTLED_RECEIPT_BUDGET,
        Duration::from_secs(5),
        "the production receipt window is not this batch's to change"
    );
    assert_eq!(
        persistence.settled_receipt_budget(),
        aura_persistence::DEFAULT_SETTLED_RECEIPT_BUDGET,
        "a fresh coordinator really uses the production receipt window"
    );
    persistence.set_sync_deadline_for_test(Duration::from_millis(500));
    persistence.set_settled_receipt_budget_for_test(Duration::from_millis(1_500));
    assert_eq!(
        persistence.sync_deadline(),
        Duration::from_millis(500),
        "the seam is scoped to this instance"
    );
    assert_eq!(
        persistence.settled_receipt_budget(),
        Duration::from_millis(1_500),
        "the receipt seam is scoped to this instance"
    );
    // A window longer than the production default is not something a test may ask for.
    persistence.set_settled_receipt_budget_for_test(Duration::from_secs(600));
    assert_eq!(
        persistence.settled_receipt_budget(),
        aura_persistence::DEFAULT_SETTLED_RECEIPT_BUDGET,
        "the receipt window may be made cheaper to observe, never longer"
    );
    persistence.set_settled_receipt_budget_for_test(Duration::from_millis(1_500));
    // Longer than the writer's injected budget, so the store's own deadline is what fires
    // first, and inside the receipt window, so the committed value still has a receiver.
    let hold = persistence.arm_writer_hold_once(
        aura_persistence::barrier::Point::WriterExecution,
        Some("sync"),
        1_500,
    );
    let title = "S03F writer deadline at the business write";
    let call = SessionCall::new("session.create", harness.workspace, title);
    let invocation = call.invocation;
    let correlation = call.request.id;
    let identity = aura_persistence::PersistenceCoordinator::session_write_identity(invocation);
    let completions_before = recorded_completions(&state);
    let unresolved_before = state.post_execution.facts().unresolved;
    let call_started = Instant::now();
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call.clone()));
    let arrived = hold.wait_for_arrivals(1, Duration::from_secs(4));
    let response = runtime.block_on(driver).expect("the driven request");
    let health = persistence.writer_health();
    println!(
        "S03F-C4 caller_answer={response} arrived={arrived} caller_ms={} writer={health:?}",
        call_started.elapsed().as_millis()
    );
    assert!(arrived, "the real job must have been taken off the writer's queue");
    assert!(
        health.running >= 1,
        "the real job is admitted and executing inside the writer, not merely held outside it: \
         {health:?}"
    );
    let envelope = failure_envelope(&response);
    assert_eq!(
        envelope.pointer("/code").and_then(Value::as_str),
        Some("command.outcome_unknown"),
        "an admitted write whose writer did not answer is the Core's own unknown answer: {response}"
    );
    assert_ne!(
        envelope.pointer("/code").and_then(Value::as_str),
        Some("broker.dispatch_unavailable"),
        "a write that may still commit must not be reported through the ordinary dispatch failure: \
         {response}"
    );
    assert_eq!(
        envelope.pointer("/details/outcome_recovery").and_then(Value::as_str),
        Some("query_runtime.command_outcome_without_replay"),
        "the answer must carry the contract's own recovery instruction: {response}"
    );
    let held = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    println!("S03F-C4 ledger_while_held={held}");
    assert_eq!(
        held.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("unknown"),
        "the writer's own deadline is not a business failure: {held}"
    );
    assert_eq!(
        persistence.job_settlement(&identity),
        None,
        "the job really has not settled while it is still inside the writer, so the value this \
         case receives cannot have come from an ordinary answer"
    );
    let projection_while_held = runtime
        .block_on(async { persistence.workspace_session_projection() })
        .map(|value| serde_json::to_string(&value).expect("projection json"))
        .expect("the projection is readable while the write is parked");
    assert_eq!(
        occurrences(&projection_while_held, title),
        0,
        "the parked job must not have committed yet"
    );

    // The job's own hold ends at 1.5 s: it commits and publishes its value, which is what the
    // caller that ended at 400 ms is still owed.
    let settled = wait_until(Duration::from_secs(8), || {
        let answer = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
        answer.pointer("/result/value/outcome").and_then(Value::as_str) == Some("confirmed")
    });
    let authoritative = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    let settlement = persistence.job_settlement(&identity);
    let facts = state.post_execution.facts();
    println!(
        "S03F-C4 ledger_after_release={authoritative} settlement={settlement:?} facts={facts:?}"
    );
    assert!(
        settled,
        "the value the store really committed must settle the original invocation: {authoritative}"
    );
    assert_eq!(
        authoritative.pointer("/result/value/terminal").and_then(Value::as_str),
        Some("succeeded")
    );
    assert_eq!(
        authoritative
            .pointer("/result/value/response/id")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<CorrelationId>().ok()),
        Some(correlation),
        "the settled response is addressed to the original request: {authoritative}"
    );
    assert_eq!(
        authoritative.pointer("/result/value/response/result/value/title").and_then(Value::as_str),
        Some(title),
        "the settled value is the session the store really wrote: {authoritative}"
    );
    assert_ne!(
        authoritative
            .pointer("/result/value/response/result/value/session_id")
            .and_then(Value::as_str),
        None,
        "the settled value carries the real session identity: {authoritative}"
    );
    let settlement = settlement.expect("the job recorded its own terminal state");
    assert!(settlement.succeeded, "the late write really committed: {settlement:?}");
    assert!(
        settlement.answered,
        "the writer answered its retained receipt even though the IPC caller had left: \
         {settlement:?}"
    );
    let projection_after = runtime
        .block_on(async { persistence.workspace_session_projection() })
        .map(|value| serde_json::to_string(&value).expect("projection json"))
        .expect("projection after the release");
    assert_eq!(occurrences(&projection_after, title), 1, "the business ran exactly once");
    assert_eq!(
        recorded_completions(&state),
        completions_before,
        "the session route's completion is the outcome row, not a second broker completion"
    );
    assert_eq!(state.settlements.pending(), 0, "the owner released its slot");
    assert_eq!(
        facts.unresolved, unresolved_before,
        "a value that was really received is not a teardown: {facts:?}"
    );
}

/// The original job commits after both storage wait windows. The caller stays Unknown while
/// it runs; the owner must later settle the same invocation with the actual value, once.
#[test]
fn a_writer_result_after_the_old_receipt_window_is_still_queryable() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    persistence.set_sync_deadline_for_test(Duration::from_millis(500));
    persistence.set_settled_receipt_budget_for_test(Duration::from_millis(300));
    let hold = persistence.arm_writer_hold_once(
        aura_persistence::barrier::Point::WriterExecution,
        Some("sync"),
        1_500,
    );
    let title = "S03F receipt window expired at the business write";
    let call = SessionCall::new("session.create", harness.workspace, title);
    let invocation = call.invocation;
    let identity = aura_persistence::PersistenceCoordinator::session_write_identity(invocation);
    let completions_before = recorded_completions(&state);
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call.clone()));
    let arrived = hold.wait_for_arrivals(1, Duration::from_secs(4));
    let response = runtime.block_on(driver).expect("the driven request");
    println!("S03F-C5 caller_answer={response} arrived={arrived}");
    assert!(arrived, "the real job must have been taken off the writer's queue");
    assert_eq!(
        failure_envelope(&response).pointer("/code").and_then(Value::as_str),
        Some("command.outcome_unknown"),
        "the caller is told the Core's own unknown answer: {response}"
    );

    // The completion owner keeps the original receipt after the caller stops waiting.
    let committed = wait_until(Duration::from_secs(8), || {
        persistence
            .workspace_session_projection()
            .ok()
            .and_then(|value| serde_json::to_string(&value).ok())
            .is_some_and(|value| occurrences(&value, title) == 1)
    });
    assert!(wait_until(Duration::from_secs(3), || state.settlements.pending() == 0));
    let authoritative = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    let facts = state.post_execution.facts();
    println!(
        "S03F-C5 committed={committed} ledger={authoritative} settlement={:?} facts={facts:?}",
        persistence.job_settlement(&identity)
    );
    assert!(committed, "the real job must really commit after its own hold ends");
    assert_eq!(
        authoritative.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("confirmed"),
        "the original receipt must survive the caller wait: {authoritative}"
    );
    assert_ne!(
        authoritative.pointer("/result/value/terminal").and_then(Value::as_str),
        Some("failed"),
        "the real commit must not be recorded as a failure: {authoritative}"
    );
    assert_eq!(
        recorded_completions(&state),
        completions_before,
        "a session route must not invent a separate broker completion"
    );
    let released = wait_until(Duration::from_secs(4), || state.settlements.pending() == 0);
    assert!(
        released,
        "the owner gives its slot back only after the real result is settled: pending={}",
        state.settlements.pending()
    );
}

/// S03F-C6: work whose settlement is held does not contaminate another workspace, the control lane
/// or the heartbeat, and its own result is queryable once it is released.
#[test]
fn a_held_settlement_does_not_contaminate_another_workspace_or_the_control_lane() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let persistence = harness.persistence();
    let root = harness._fixture.root.path().join("ws-a");
    let relative = "s03f-held-while-b-works.txt";
    let content = "A's effect, written while B keeps working\n";
    let call = SessionCall::with_params(
        method::FS_WRITE_FILE,
        json!({
            "workspace_id": harness.workspace,
            "session_id": harness.session,
            "target": { "path": relative },
            "content": content,
            "expected": { "expected_kind": "absent" },
            "encoding": "utf8",
            "line_ending": "lf"
        }),
    );
    let invocation = call.invocation;
    let hold = persistence.arm_admission_hold_selected(
        Some("sync"),
        &aura_persistence::PersistenceCoordinator::broker_evidence_identity(call.request.id),
        HEARTBEAT_HOLD_MS,
    );
    let driver = runtime.spawn(drive_session(Arc::clone(&state), call.clone()));
    let timeline = CaseTimeline::new();
    let arrived = hold.wait_for_arrivals(1, Duration::from_secs(4));
    timeline.mark(format!("hold.arrived arrived={arrived}"));
    assert!(arrived, "A's evidence read must really reach the storage hold");
    assert!(
        hold.arrivals().iter().any(|entry| entry.held && entry.hold_nanos == 0),
        "the arrival must still be inside its hold while this case observes it"
    );

    // B: another workspace's real write, the control lane and the production heartbeat, all while
    // A's settlement is parked. None of them is re-implemented for the test.
    let (control, control_elapsed) = observed_call(
        &runtime,
        Arc::clone(&state),
        method::PERMISSION_GET_SESSION_STATE,
        json!({"session_id": harness.other_session}),
    );
    let (projection, projection_elapsed) = observed_call(
        &runtime,
        Arc::clone(&state),
        method::PROJECTION_SNAPSHOT,
        json!({
            "projection_kind": "workspace_session",
            "projection_scope": {"type": "global"},
            "known_runtime_instance_id": null,
        }),
    );
    let heartbeat = start_heartbeat(&runtime, Arc::clone(&state), &timeline);
    let sentinel = start_runtime_sentinel(&runtime, Duration::from_secs(2), &timeline);
    let answered =
        wait_until_us(&timeline, timeline.now_us() + HEARTBEAT_WINDOW_MS * 1_000, || {
            control_elapsed.load(Ordering::Acquire) != u64::MAX
                && projection_elapsed.load(Ordering::Acquire) != u64::MAX
        });
    let control_ms = control_elapsed.load(Ordering::Acquire);
    // The hold's real release instant, from the layer that owns it. This case releases the hold
    // itself, so the served duration is only known after that release; until then the load-bearing
    // "still parked" proof is the barrier's own unpublished state, read immediately below.
    let arrival_us = first_arrival_us(&timeline, &hold);
    let observed_until_us =
        timeline.now_us().saturating_add((HEARTBEAT_WINDOW_MS + LATE_OBSERVATION_MS) * 1_000);
    wait_for_heartbeat(&timeline, &heartbeat, observed_until_us);
    let still_parked = still_inside_hold(&hold);
    timeline.mark(format!("hold.still_parked_at_heartbeat parked={still_parked:?}"));
    let observation = judge_heartbeat(&heartbeat, HEARTBEAT_WINDOW_MS, arrival_us, 0);
    let heartbeat_instants = observation.instants;
    let sentinel_instants = sentinel.instants();
    println!(
        "S03F-C6 while_held answered={answered} control_ms={control_ms} \
         projection_ms={} heartbeat_within_window={} arrivals_still_parked_at_heartbeat={} \
         first_heartbeat_ms={} heartbeat_dequeued_us={} heartbeat_queued_us={} \
         hold_arrival_us={arrival_us:?} runtime_sentinel_ms={} sentinel_first_poll_us={} \
         sentinel_woke_us={}",
        projection_elapsed.load(Ordering::Acquire),
        observation.within_window,
        still_parked.map_or_else(|| "none".to_owned(), |count| count.to_string()),
        heartbeat_instants.delivered_ms,
        heartbeat_instants.dequeued_us,
        heartbeat_instants.queued_us,
        sentinel_instants.fired_ms,
        sentinel_instants.first_poll_us,
        sentinel_instants.woke_us
    );
    assert!(
        answered,
        "the control lane and a real read must answer while A's settlement is parked"
    );
    assert!(
        control_ms <= BUDGET_MS + 400,
        "authentication must not wait for A's parked settlement: {control_ms} ms"
    );
    // The heartbeat was dequeued while the parked arrival was still unpublished, which is the
    // barrier's own statement that the hold had not ended: the dequeue is at or before this read.
    assert!(
        still_parked.is_some(),
        "the heartbeat must be dequeued while A is still inside the storage hold: \
         heartbeat_dequeued_us={} first_heartbeat_ms={} hold_ms={HEARTBEAT_HOLD_MS}",
        heartbeat_instants.dequeued_us,
        heartbeat_instants.delivered_ms
    );
    assert!(
        observation.within_window,
        "the heartbeat must be dequeued inside the {HEARTBEAT_WINDOW_MS} ms window measured from \
         the probe's own spawn: first_heartbeat_ms={} heartbeat_dequeued_us={} \
         runtime_sentinel_ms={} sentinel_first_poll_us={} sentinel_woke_us={}",
        heartbeat_instants.delivered_ms,
        heartbeat_instants.dequeued_us,
        sentinel_instants.fired_ms,
        sentinel_instants.first_poll_us,
        sentinel_instants.woke_us
    );
    // The same window margin as R3-G1/R3-G2, so the same independent timer is required to have
    // fired for this to be a measurement at all.
    assert!(
        sentinel_instants.fired_ms != u64::MAX,
        "the independent 2 s timer on the same runtime never fired, so this window was not \
         measured at all: first_heartbeat_ms={}",
        heartbeat_instants.delivered_ms
    );
    let control_value = runtime.block_on(control).expect("control task");
    assert!(control_value.get("error").is_none(), "{control_value}");
    let projection_value = runtime.block_on(projection).expect("projection task");
    assert!(projection_value.get("error").is_none(), "{projection_value}");
    assert!(
        projection_value.to_string().contains(&harness.other_workspace.to_string()),
        "the projection read must still carry the other workspace: {projection_value}"
    );

    let response = runtime.block_on(driver).expect("the driven request");
    assert_eq!(
        failure_envelope(&response).pointer("/code").and_then(Value::as_str),
        Some("command.outcome_unknown"),
        "A's caller is honestly Unknown while its settlement is parked: {response}"
    );

    hold.release_all();
    let settled = wait_until(Duration::from_secs(8), || {
        let answer = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
        answer.pointer("/result/value/outcome").and_then(Value::as_str) == Some("confirmed")
    });
    let authoritative = runtime.block_on(query_command_outcome(Arc::clone(&state), invocation));
    println!("S03F-C6 A_after_release={authoritative}");
    assert!(settled, "A's own result is queryable after the release: {authoritative}");
    assert_eq!(std::fs::read(root.join(relative)).expect("A's effect"), content.as_bytes());

    // New work in the other workspace is normal afterwards.
    let b_relative = "s03f-b-after-release.txt";
    let b_content = "B's own write after A settled\n";
    let b_call = SessionCall::with_params(
        method::FS_WRITE_FILE,
        json!({
            "workspace_id": harness.other_workspace,
            "session_id": harness.other_session,
            "target": { "path": b_relative },
            "content": b_content,
            "expected": { "expected_kind": "absent" },
            "encoding": "utf8",
            "line_ending": "lf"
        }),
    );
    let b_response = runtime.block_on(drive_session(Arc::clone(&state), b_call));
    println!("S03F-C6 B_after_release={b_response}");
    assert!(b_response.get("error").is_none(), "new work must be normal: {b_response}");
    assert_eq!(
        std::fs::read(harness._fixture.root.path().join("ws-b").join(b_relative))
            .expect("B's effect"),
        b_content.as_bytes()
    );
    heartbeat.join();
}

/// S03F-C7: at the completion cap the refusal happens BEFORE the handler runs, and it stays the
/// definite not-run answer the rest of the path already gives.
///
/// The cap is filled with real claimed slots, and the next `fs.write_file` must be refused with a
/// retryable `persistence.busy` that carries no recovery instruction - because there is nothing to
/// query, nothing ran - and the disk must hold no effect at all.
#[test]
fn a_completion_cap_refusal_on_the_broker_path_is_a_definite_not_run_answer() {
    let runtime = test_runtime();
    let harness = runtime.block_on(Harness::new(BUDGET_MS));
    let state = Arc::clone(&harness.state);
    let cap = crate::command_settlement::MAX_PENDING_SETTLEMENTS;
    let started_before = state.post_execution.facts().started;
    let mut held = Vec::new();
    for _ in 0..cap {
        held.push(
            state
                .settlements
                .begin(CommandInvocationId::new(), CorrelationId::new(), "fixture")
                .expect("a settlement slot below the cap"),
        );
    }
    assert_eq!(state.settlements.pending(), cap);
    let root = harness._fixture.root.path().join("ws-a");
    let relative = "s03f-cap-refusal.txt";
    let written = root.join(relative);
    let call = SessionCall::with_params(
        method::FS_WRITE_FILE,
        json!({
            "workspace_id": harness.workspace,
            "session_id": harness.session,
            "target": { "path": relative },
            "content": "must never reach the disk\n",
            "expected": { "expected_kind": "absent" },
            "encoding": "utf8",
            "line_ending": "lf"
        }),
    );
    let response = runtime.block_on(drive_session(Arc::clone(&state), call.clone()));
    let envelope = failure_envelope(&response);
    println!("S03F-C7 cap_refusal={response}");
    assert_eq!(
        envelope.pointer("/code").and_then(Value::as_str),
        Some("persistence.busy"),
        "a refusal at the completion cap is a definite not-run answer: {response}"
    );
    assert_eq!(envelope.pointer("/retryable").and_then(Value::as_bool), Some(true));
    assert!(
        envelope.pointer("/details/outcome_recovery").is_none(),
        "a refusal must not tell the caller to query an invocation that never ran: {response}"
    );
    assert!(
        !written.exists(),
        "the handler body must not have run: the disk holds no effect for a refused command"
    );
    assert_eq!(
        state.post_execution.facts().started,
        started_before,
        "a refused command never entered the completion owner"
    );
    let after_refusal =
        runtime.block_on(query_command_outcome(Arc::clone(&state), call.invocation));
    assert_eq!(
        after_refusal.pointer("/result/value/outcome").and_then(Value::as_str),
        Some("confirmed"),
        "a proven pre-execution refusal stays a definite result, not an Unknown: {after_refusal}"
    );
    assert_eq!(
        after_refusal.pointer("/result/value/terminal").and_then(Value::as_str),
        Some("failed")
    );
    assert_eq!(
        after_refusal.pointer("/result/value/response/error/data/code").and_then(Value::as_str),
        Some("persistence.busy"),
        "the recorded error is the refusal the caller really received: {after_refusal}"
    );
    assert!(!held.is_empty(), "the cap had to be filled with slots that are really claimed");
}

// =============================================================================================
// Remaining repair: the catalog refresh must have exactly one real owner for its whole
// lifecycle, and the Models/Tools configuration-maintenance callbacks must do their storage
// work on the one boundary instead of on the async consumer.
//
// Everything below drives real services: the real `ProviderCatalogService` over the real
// persistence cache, the real `ModelConfigService`, the process's real `SyncBoundary` and the
// real `PersistenceCoordinator`. The only injected things are (a) the catalog *source*, which
// is the network half and must not be a real `models.dev` request in a unit test, and (b) real
// hold points the storage layer already carries (`arm_writer_hold_once`). Nothing here copies
// the implementation it is checking.
// =============================================================================================

/// A minimal but real `models.dev` catalog body: the parser requires a non-empty provider map
/// with non-blank provider and model names.
const S04_CATALOG_BODY: &[u8] =
    br#"{"provider-a":{"name":"Provider A","models":{"model-a":{"name":"Model A"}}}}"#;

/// A second real catalog body, so "a normal refresh follows and really succeeds" is measured by
/// the content it published rather than by the absence of an error.
const S04_CATALOG_BODY_V2: &[u8] =
    br#"{"provider-a":{"name":"Provider A","models":{"model-a":{"name":"Model A"},"model-b":{"name":"Model B"}}}}"#;

/// What the scripted source does for one fetch, in order. An exhausted queue answers with the
/// ordinary body, so "the refresh can follow normally" is a real successful refresh.
enum CatalogFetchStep {
    Body(&'static [u8]),
    Failure,
    Panic,
    /// Park until this is notified: the fetch is really in flight and has written nothing.
    Hold(Arc<tokio::sync::Notify>),
}

struct ScriptedCatalogSource {
    steps: Mutex<std::collections::VecDeque<CatalogFetchStep>>,
    fetches: AtomicU64,
    conditional: Mutex<Option<aura_model_catalog::CatalogConditionalRequest>>,
}

impl ScriptedCatalogSource {
    fn new() -> Self {
        Self {
            steps: Mutex::new(std::collections::VecDeque::new()),
            fetches: AtomicU64::new(0),
            conditional: Mutex::new(None),
        }
    }

    fn push(&self, step: CatalogFetchStep) {
        self.steps.lock().expect("scripted source").push_back(step);
    }

    fn fetches(&self) -> u64 {
        self.fetches.load(Ordering::Acquire)
    }
}

impl aura_model_catalog::CatalogSource for ScriptedCatalogSource {
    async fn fetch(
        &self,
        conditional: aura_model_catalog::CatalogConditionalRequest,
    ) -> Result<aura_model_catalog::CatalogFetchResponse, aura_model_catalog::CatalogError> {
        self.fetches.fetch_add(1, Ordering::AcqRel);
        *self.conditional.lock().expect("scripted source conditionals") = Some(conditional);
        let step = self.steps.lock().expect("scripted source").pop_front();
        let body = match step {
            Some(CatalogFetchStep::Panic) => panic!("injected catalog fetch panic"),
            Some(CatalogFetchStep::Failure) => {
                return Err(aura_model_catalog::CatalogError::Source(
                    "injected offline catalog source".to_owned(),
                ));
            }
            Some(CatalogFetchStep::Hold(notify)) => {
                notify.notified().await;
                S04_CATALOG_BODY.to_vec()
            }
            Some(CatalogFetchStep::Body(body)) => body.to_vec(),
            None => S04_CATALOG_BODY.to_vec(),
        };
        Ok(aura_model_catalog::CatalogFetchResponse {
            status: 200,
            etag: Some("\"s04-catalog\"".to_owned()),
            last_modified: None,
            body,
        })
    }
}

/// The real persistent catalog cache, with a default-off injected fault so the regression can
/// reach the two storage failures the settlement rules are about (a cache write that errors and
/// a cache write that panics) without re-implementing any of the persistence.
struct ScriptedCatalogCache {
    real: aura_model_catalog::PersistenceCatalogCache,
    fault: Arc<AtomicU64>,
}

const CATALOG_CACHE_PASSTHROUGH: u64 = 0;
const CATALOG_CACHE_FAILURE: u64 = 1;
const CATALOG_CACHE_PANIC: u64 = 2;

impl aura_model_catalog::CatalogCache for ScriptedCatalogCache {
    fn load(
        &self,
    ) -> Result<aura_model_catalog::CatalogCacheRecord, aura_model_catalog::CatalogError> {
        self.real.load()
    }

    fn persist_success(
        &self,
        raw: &[u8],
        snapshot: aura_model_domain::ProviderCatalogSnapshot,
    ) -> Result<aura_model_domain::ProviderCatalogSnapshot, aura_model_catalog::CatalogError> {
        match self.fault.load(Ordering::Acquire) {
            CATALOG_CACHE_PANIC => panic!("injected catalog cache panic"),
            CATALOG_CACHE_FAILURE => {
                return Err(aura_model_catalog::CatalogError::Source(
                    "injected catalog cache write failure".to_owned(),
                ));
            }
            _ => {}
        }
        self.real.persist_success(raw, snapshot)
    }

    fn persist_revalidated(
        &self,
        snapshot: aura_model_domain::ProviderCatalogSnapshot,
    ) -> Result<(), aura_model_catalog::CatalogError> {
        self.real.persist_revalidated(snapshot)
    }

    fn persist_failure(
        &self,
        issue_code: &aura_domain_common::NormalizedIssueCode,
    ) -> Result<(), aura_model_catalog::CatalogError> {
        self.real.persist_failure(issue_code)
    }
}

type ScriptedCatalog = aura_model_catalog::ProviderCatalogService<
    Arc<ScriptedCatalogSource>,
    Arc<ScriptedCatalogCache>,
>;

/// The real catalog stack: real persistence, the real model config service, the real boundary.
struct CatalogStack {
    // Declared first: the route ends before the fixture's directory is removed.
    _route: crate::CoreLogRoute,
    _fixture: Fixture,
    log_path: PathBuf,
    persistence: Arc<aura_persistence::PersistenceCoordinator>,
    model_config: Arc<aura_model_config::ModelConfigService>,
    sync: crate::sync_boundary::SyncBoundary,
    source: Arc<ScriptedCatalogSource>,
    fault: Arc<AtomicU64>,
    catalog: Arc<ScriptedCatalog>,
    icons: crate::p05::provider_icon::ProviderIconService,
}

impl CatalogStack {
    fn new(budget: Duration) -> Self {
        let fixture = Fixture::new();
        let log_path = fixture.root.path().join("core.jsonl");
        // The stack's route is this case's own until the stack is dropped.
        let route = crate::CoreLogRoute::to(log_path.clone());
        fixture.paths.ensure_layout().expect("isolated data layout");
        let aura_persistence::PersistenceStartup::Ready(persistence) =
            aura_persistence::PersistenceCoordinator::start(
                aura_persistence::PersistenceOptions::new(
                    fixture.paths.database_path.clone(),
                    fixture.paths.artifact_objects_dir.clone(),
                    aura_domain_common::RuntimeInstanceId::new(),
                ),
            )
            .expect("isolated persistence startup")
        else {
            panic!("a fresh fixture database must be ready")
        };
        let persistence = Arc::new(persistence);
        let settings = aura_config::SettingsFacade::open(fixture.paths.clone())
            .expect("isolated settings facade");
        let model_config = Arc::new(aura_model_config::ModelConfigService::new(settings));
        let source = Arc::new(ScriptedCatalogSource::new());
        let fault = Arc::new(AtomicU64::new(CATALOG_CACHE_PASSTHROUGH));
        let cache = Arc::new(ScriptedCatalogCache {
            real: aura_model_catalog::PersistenceCatalogCache::new(Arc::clone(&persistence)),
            fault: Arc::clone(&fault),
        });
        // A REAL STALE LKG, written through the real persistent cache before the service opens:
        // this is what a catalog refreshed a day ago looks like at startup, it is what makes the
        // scheduled check due, and it is what a failed refresh has to keep.
        let refreshed_at =
            UtcTimestamp::from_value(UtcTimestamp::now().value() - ::time::Duration::hours(25));
        let providers = aura_model_catalog::parse_models_dev_catalog(S04_CATALOG_BODY)
            .expect("the fixture catalog body parses");
        let provider_order = providers.keys().cloned().collect();
        let seeded = aura_model_domain::ProviderCatalogSnapshot {
            catalog_revision: aura_domain_common::Revision::INITIAL,
            catalog_content_hash: aura_domain_common::ContentHash::blake3(S04_CATALOG_BODY),
            checked_at: refreshed_at,
            last_successful_refresh_at: refreshed_at,
            etag: None,
            last_modified: None,
            raw_artifact_id: aura_domain_common::ArtifactId::new(),
            provider_order,
            providers,
        };
        aura_model_catalog::CatalogCache::persist_success(cache.as_ref(), S04_CATALOG_BODY, seeded)
            .expect("seed the stale real LKG");
        let catalog = Arc::new(
            ScriptedCatalog::open(Arc::clone(&source), Arc::clone(&cache))
                .expect("scripted catalog service"),
        );
        // A real gateway, only so the real ProviderIconService can exist. Its own catalog has no
        // snapshot in a fresh fixture, so `schedule_missing` returns without doing anything.
        let grants = Arc::new(aura_network_control::PersistenceNetworkGrantStore::new(
            persistence.as_ref().clone(),
        ));
        let gateway = Arc::new(aura_network_control::CoreHttpClientGateway::new(
            grants,
            None,
            aura_network_control::NetworkControlOptions::default(),
        ));
        let real_catalog = Arc::new(
            crate::p05::catalog::open_catalog(Arc::clone(&gateway), Arc::clone(&persistence))
                .expect("real catalog for the icon service"),
        );
        let icons = crate::p05::provider_icon::ProviderIconService::new(
            gateway,
            real_catalog,
            Arc::clone(&model_config),
            Arc::clone(&persistence),
        );
        Self {
            _route: route,
            _fixture: fixture,
            log_path,
            persistence,
            model_config,
            sync: crate::sync_boundary::SyncBoundary::with_budget(budget),
            source,
            fault,
            catalog,
            icons,
        }
    }

    fn path(
        &self,
    ) -> crate::p05::catalog::CatalogRefreshPath<
        Arc<ScriptedCatalogSource>,
        Arc<ScriptedCatalogCache>,
    > {
        crate::p05::catalog::CatalogRefreshPath {
            sync: self.sync.clone(),
            catalog: Arc::clone(&self.catalog),
            model_config: Arc::clone(&self.model_config),
            persistence: Arc::clone(&self.persistence),
            provider_icons: self.icons.clone(),
        }
    }

    /// The real driver, with the real source, boundary, persistence and commit section.
    ///
    /// The occupancy is taken first, exactly as every product path takes it, and its one owner is
    /// then handed to the driver. This handover is the thing the whole repair is about, and it is
    /// the only place in these regressions that knows the API's shape.
    fn spawn_driver(&self, runtime: &tokio::runtime::Runtime) -> tokio::task::JoinHandle<()> {
        let ownership = self
            .catalog
            .begin_refresh()
            .expect("take the refresh occupancy")
            .expect("the fixture takes the occupancy before it starts the driver");
        let path = self.path();
        // `spawn_catalog_refresh` is synchronous: it registers the driver on the runtime and hands
        // its handle straight back, so there is nothing to poll or await here. What it does need
        // from its caller is the runtime context `tokio::spawn` requires, which entering the
        // runtime provides without driving or polling anything.
        let _entered = runtime.enter();
        crate::p05::catalog::spawn_catalog_refresh(
            path.sync,
            path.catalog,
            path.model_config,
            path.persistence,
            path.provider_icons,
            ownership,
        )
    }

    /// The latest committed ProviderCatalog projection, from real persistence.
    fn catalog_projection(&self) -> Option<aura_projection_contract::ProjectionChange> {
        self.persistence
            .latest_model_projection(
                aura_projection_contract::ProjectionKind::ProviderCatalog,
                aura_projection_contract::ProjectionScope::Global,
            )
            .expect("latest provider catalog projection")
    }

    /// The latest committed ProviderCatalog projection revision, from real persistence.
    fn catalog_projection_revision(&self) -> Option<u64> {
        self.catalog_projection().map(|change| change.revision().value())
    }

    /// Reads the routed Core log, which is where the refresh settles its real outcome.
    fn core_log(&self) -> String {
        std::fs::read_to_string(&self.log_path).unwrap_or_default()
    }
}

fn wait_until_async(deadline: Duration, predicate: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    predicate()
}

/// A real timer inside this runtime, so "the timer driver still delivers" is measured rather
/// than assumed while a refresh section is parked.
struct TimerProbe {
    ticks: Arc<AtomicU64>,
    task: tokio::task::JoinHandle<()>,
}

impl TimerProbe {
    fn start(runtime: &tokio::runtime::Runtime, ticks: usize, period_ms: u64) -> Self {
        let counter = Arc::new(AtomicU64::new(0));
        let recorder = Arc::clone(&counter);
        let task = runtime.spawn(async move {
            for _ in 0..ticks {
                tokio::time::sleep(Duration::from_millis(period_ms)).await;
                recorder.fetch_add(1, Ordering::AcqRel);
            }
        });
        Self { ticks: counter, task }
    }

    fn fired(&self) -> u64 {
        self.ticks.load(Ordering::Acquire)
    }

    fn join(self) {
        // Same as `HeartbeatProbe::join`: the timer task is detached, not cancelled.
        drop(self.task);
    }
}

/// A1 - the announcement has started, is really parked in storage, and is over its budget. The
/// refresh still owns the one occupancy, a second begin cannot take it, and control still runs.
#[test]
fn a_refresh_announcement_over_its_budget_keeps_the_only_occupancy_while_control_still_runs() {
    let runtime = test_runtime();
    let stack = CatalogStack::new(Duration::from_millis(BUDGET_MS));
    let lkg = stack.catalog.current_snapshot().expect("the seeded stale LKG is loaded");
    assert!(
        stack.catalog.needs_refresh(UtcTimestamp::now()),
        "a catalog refreshed 25 hours ago makes the scheduled check due"
    );

    // Hold the next real write on the writer thread for longer than the boundary budget. The
    // announcement's first write is its loading-projection commit, so it is the one parked.
    let hold = stack.persistence.arm_writer_hold_once(
        aura_persistence::barrier::Point::WriterExecution,
        None,
        HOLD_MS,
    );
    // The driver's own fetch parks on this gate, so "the announcement settled and the fetch has
    // not written anything yet" is an instant the case can really hold still.
    let fetch_gate = Arc::new(tokio::sync::Notify::new());
    stack.source.push(CatalogFetchStep::Hold(Arc::clone(&fetch_gate)));
    let probe = TimerProbe::start(&runtime, 6, 100);
    let path = stack.path();
    let announce =
        runtime.spawn(async move { crate::p05::catalog::refresh_catalog_if_due_for(path).await });
    assert!(
        wait_until_async(Duration::from_secs(10), || still_inside_hold(&hold).is_some()),
        "the scheduled announcement must really reach the writer and park there"
    );
    // Past the caller's budget, still parked: the work the announcement owns has not finished.
    std::thread::sleep(Duration::from_millis(BUDGET_MS + 300));
    assert!(
        still_inside_hold(&hold).is_some(),
        "the announcement is still inside its hold, so its real work has not settled"
    );
    assert!(
        stack.catalog.refresh_in_progress(),
        "an announcement whose caller budget expired is still real work: its occupancy stays owned"
    );
    let second = stack.catalog.begin_refresh().expect("second begin");
    assert!(
        second.is_none(),
        "a second scheduled/manual begin must not take an occupancy a running refresh still owns"
    );
    // Control keeps working: the refresh holds one of the four permits, not the boundary.
    assert_eq!(
        stack.sync.available(),
        SYNC_WORKERS - 1,
        "one refresh section occupies exactly one permit, so three remain for control"
    );
    let served = runtime.block_on(stack.sync.run(
        SyncSection::JournalRead,
        crate::sync_boundary::SyncSubject::new().method("control"),
        || 7_u64,
    ));
    assert!(
        matches!(served, crate::blocking_pool::BlockingAdmission::Completed(7)),
        "another section is still admitted while the refresh is over its budget"
    );
    assert!(
        probe.fired() >= 2,
        "the runtime's own timer driver still delivers while the refresh section is parked: {}/{}",
        probe.fired(),
        6
    );
    let timers_fired = probe.fired();
    probe.join();

    // Release: the announcement's real work finishes, settles and releases the occupancy once.
    assert!(
        wait_for_served_hold(&CaseTimeline::new(), &hold, HOLD_MS * 4_000) > 0,
        "the parked announcement must really finish"
    );
    let settled = runtime.block_on(announce).expect("the scheduled path must answer");
    assert!(
        settled.expect("a settled announcement"),
        "work that really committed a loading projection is not reported as 'no refresh started'"
    );
    // The driver is now in its fetch, which is parked on the gate: this instant belongs to the
    // announcement alone, so the projection really is the one loading projection it committed.
    assert!(
        wait_until_async(Duration::from_secs(10), || stack.source.fetches() >= 1),
        "the refresh really continued into its asynchronous fetch"
    );
    let loading_revision = stack.catalog_projection_revision().expect("a loading projection");
    assert_eq!(loading_revision, 1, "the announcement committed exactly one loading projection");
    match stack.catalog_projection().expect("a loading projection") {
        aura_projection_contract::ProjectionChange::ProviderCatalog { payload, .. } => assert_eq!(
            payload.state,
            aura_model_domain::ProviderCatalogState::RefreshingWithLkg,
            "the committed projection is the real loading state, keeping the LKG"
        ),
        other => panic!("expected a ProviderCatalog projection, got {other:?}"),
    }
    fetch_gate.notify_waiters();
    assert!(
        wait_until_async(Duration::from_secs(20), || !stack.catalog.refresh_in_progress()),
        "the refresh really ended and gave its occupancy back"
    );
    assert_eq!(
        stack.catalog.status().expect("status").state,
        aura_model_domain::ProviderCatalogState::Fresh,
        "the fetch behind the announcement really settled the refresh"
    );
    assert_eq!(
        stack.catalog.current_snapshot().map(|s| s.catalog_content_hash),
        Some(lkg.catalog_content_hash.clone()),
        "the same real catalog body is the LKG before and after"
    );
    assert_eq!(stack.sync.available(), SYNC_WORKERS, "every permit came back");
    let final_revision = stack.catalog_projection_revision().expect("a settled projection");
    println!(
        "S04C-CAT-1 announced_over_budget=true second_begin_taken=false permits_free_during_hold={} \
         loading_revision={loading_revision} final_revision={final_revision} \
         timers_fired={timers_fired} source_fetches={}",
        SYNC_WORKERS - 1,
        stack.source.fetches()
    );
}

/// A2 - after the real work settles, the same refresh settles exactly once: the occupancy is
/// released once, cannot be taken twice at the same time, and a normal refresh follows.
#[test]
fn a_settled_refresh_releases_the_occupancy_once_and_a_normal_refresh_can_follow() {
    let runtime = test_runtime();
    let stack = CatalogStack::new(Duration::from_millis(BUDGET_MS));
    let lkg_hash = stack
        .catalog
        .current_snapshot()
        .expect("the seeded stale LKG is loaded")
        .catalog_content_hash;

    // One whole real refresh, driver included: the fetch, the bounded commit section and the
    // publication all run against the real persistence.
    let driver = stack.spawn_driver(&runtime);
    assert!(
        wait_until_async(Duration::from_secs(20), || !stack.catalog.refresh_in_progress()),
        "the driver's own section releases the refresh slot when the real work ends"
    );
    runtime.block_on(driver).expect("the driver must not panic");
    let after_driver = stack.catalog_projection_revision().expect("projection after the refresh");

    // The occupancy is free and can be taken exactly once: two begins cannot both own it.
    let first = stack.catalog.begin_refresh().expect("first begin");
    assert!(first.is_some(), "a settled refresh releases its occupancy for the next one");
    let second = stack.catalog.begin_refresh().expect("second begin");
    assert!(second.is_none(), "one occupancy cannot be taken twice");
    drop(first);
    assert!(!stack.catalog.refresh_in_progress(), "released");

    // A normal refresh follows and really succeeds, on the same real services, and the content
    // it publishes is the content the source really answered with.
    stack.source.push(CatalogFetchStep::Body(S04_CATALOG_BODY_V2));
    let followed = stack.spawn_driver(&runtime);
    assert!(
        wait_until_async(Duration::from_secs(20), || !stack.catalog.refresh_in_progress()),
        "the following refresh settles"
    );
    runtime.block_on(followed).expect("the following driver must not panic");
    let status = stack.catalog.status().expect("status");
    assert_eq!(
        status.state,
        aura_model_domain::ProviderCatalogState::Fresh,
        "a normal refresh after settlement really succeeds"
    );
    assert!(status.last_error_code.is_none());
    assert_ne!(
        status.snapshot.as_ref().map(|snapshot| snapshot.catalog_content_hash.clone()),
        Some(lkg_hash),
        "the following refresh published the catalog its source really answered with"
    );
    let after_follow = stack.catalog_projection_revision().expect("projection after the follow-up");
    assert!(
        after_follow >= after_driver,
        "a later refresh never moves the projection backwards: {after_follow} >= {after_driver}"
    );
    assert_eq!(stack.sync.available(), SYNC_WORKERS, "every permit came back");
    println!(
        "S04C-CAT-2 settlements=true occupancy_released_once=true follow_up_state=fresh \
         projection_revision={after_follow} source_fetches={}",
        stack.source.fetches()
    );
}

/// A3 - a refresh that has written nothing gives its occupancy back on cancellation; a refresh
/// whose storage section has really started keeps it and settles it.
#[test]
fn a_cancelled_refresh_that_never_reached_storage_returns_its_occupancy_and_a_started_one_keeps_it()
{
    let runtime = test_runtime();
    let stack = CatalogStack::new(Duration::from_millis(BUDGET_MS));

    // (a) Aborted while the fetch is really in flight and has written nothing.
    let gate = Arc::new(tokio::sync::Notify::new());
    stack.source.push(CatalogFetchStep::Hold(Arc::clone(&gate)));
    let driver = stack.spawn_driver(&runtime);
    assert!(
        wait_until_async(Duration::from_secs(10), || stack.source.fetches() >= 1),
        "the fetch must really have started before it is cancelled"
    );
    driver.abort();
    assert!(
        wait_until_async(Duration::from_secs(10), || !stack.catalog.refresh_in_progress()),
        "a cancelled fetch wrote nothing, so the occupancy must come back"
    );
    assert!(
        stack.catalog_projection_revision().is_none(),
        "a cancelled fetch must not have committed a loading projection"
    );
    let retaken = stack.catalog.begin_refresh().expect("begin after the cancel");
    assert!(retaken.is_some(), "with no work started, a begin can be retaken");
    drop(retaken);
    gate.notify_waiters();

    // (b) A panic in the fetch half: the driver future unwinds and its occupancy is released.
    stack.source.push(CatalogFetchStep::Panic);
    let driver = stack.spawn_driver(&runtime);
    let panicked = runtime.block_on(driver);
    assert!(panicked.is_err(), "the injected fetch panic must reach the driver");
    assert!(
        wait_until_async(Duration::from_secs(10), || !stack.catalog.refresh_in_progress()),
        "the unwinding driver must release the occupancy it held"
    );
    let after_panic_begin = stack.catalog.begin_refresh().expect("begin after the panic");
    assert!(after_panic_begin.is_some(), "the occupancy came back after the fetch panic");
    drop(after_panic_begin);

    // (c) The storage section has really started: the occupancy is held until it settles, even
    // though the driver's own caller budget expires first.
    let hold = stack.persistence.arm_writer_hold_once(
        aura_persistence::barrier::Point::WriterExecution,
        None,
        HOLD_MS,
    );
    let driver = stack.spawn_driver(&runtime);
    assert!(
        wait_until_async(Duration::from_secs(10), || still_inside_hold(&hold).is_some()),
        "the commit section must really reach the writer"
    );
    std::thread::sleep(Duration::from_millis(BUDGET_MS + 300));
    assert!(still_inside_hold(&hold).is_some(), "the commit section is still inside its hold");
    assert!(
        stack.catalog.refresh_in_progress(),
        "a storage section that really started keeps the occupancy past the caller's budget"
    );
    assert!(
        stack.catalog.begin_refresh().expect("begin while the commit section runs").is_none(),
        "no second owner while the commit section is really running"
    );
    assert!(wait_until_async(Duration::from_secs(20), || {
        still_inside_hold(&hold).is_none() && !stack.catalog.refresh_in_progress()
    }));
    runtime.block_on(driver).expect("the driver settles");
    assert!(
        stack.catalog.current_snapshot().is_some(),
        "the started refresh settled its real snapshot"
    );
    // After a caller-budget expiry, the driver is no longer the blocking task's join. Releasing
    // the refresh occupancy ends its work, while the boundary still owns the permit through its
    // settlement sink. Observe that separate completion instead of racing the sink's log write.
    assert!(
        wait_until_async(Duration::from_secs(20), || stack.sync.available() == SYNC_WORKERS),
        "the permit must return after the real settlement sink finishes"
    );
    assert_eq!(stack.sync.available(), SYNC_WORKERS, "the permit came back with the work");
    println!(
        "S04C-CAT-3 fetch_abort_released=true fetch_panic_released=true \
         started_section_held_until_settled=true source_fetches={}",
        stack.source.fetches()
    );
}

/// A4 - a refusal, a storage error and a storage panic all return the permit and the occupancy,
/// keep the LKG, and are never reported as a successful refresh.
#[test]
fn a_refused_or_failing_catalog_refresh_returns_its_permit_and_occupancy_and_keeps_the_lkg() {
    let runtime = test_runtime();
    let stack = CatalogStack::new(Duration::from_millis(BUDGET_MS));
    let lkg = stack.catalog.current_snapshot().expect("the seeded stale LKG is loaded");

    // (a) The boundary is full: the announcement provably did not run.
    let gate = Arc::new(AtomicU64::new(0));
    let mut holders = Vec::new();
    for _ in 0..SYNC_WORKERS {
        let held = stack.sync.clone();
        let gate = Arc::clone(&gate);
        holders.push(runtime.spawn(async move {
            held.run(SyncSection::Inspect, crate::sync_boundary::SyncSubject::new(), move || {
                let deadline = Instant::now() + Duration::from_secs(10);
                while gate.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
            .await
        }));
    }
    assert!(wait_until_async(Duration::from_secs(10), || stack.sync.available() == 0));
    let refused = runtime
        .block_on(crate::p05::catalog::refresh_catalog_if_due_for(stack.path()))
        .expect("a refused announcement is not an error");
    assert!(!refused, "a refused section is never reported as a started refresh");
    assert!(
        !stack.catalog.refresh_in_progress(),
        "a refusal proves the section did not run, so the occupancy is already back"
    );
    assert_eq!(
        stack.catalog_projection_revision(),
        None,
        "a refused announcement committed no loading projection at all"
    );
    assert_eq!(
        stack.sync.counters(SyncSection::Projection).refused,
        1,
        "the boundary's own record says the section was refused"
    );
    assert!(
        stack.catalog.needs_refresh(aura_domain_common::UtcTimestamp::now()),
        "a refusal leaves last_successful_refresh_at untouched, so the check retries"
    );
    gate.store(1, Ordering::Release);
    for holder in holders {
        runtime.block_on(holder).expect("the held sections end");
    }
    assert!(wait_until_async(Duration::from_secs(10), || stack.sync.available() == SYNC_WORKERS));

    // (b) The fetch itself fails: the failure is published, the LKG is kept, the occupancy
    // comes back and the refresh is not a success.
    stack.source.push(CatalogFetchStep::Failure);
    let driver = stack.spawn_driver(&runtime);
    assert!(wait_until_async(Duration::from_secs(20), || !stack.catalog.refresh_in_progress()));
    runtime.block_on(driver).expect("a failed fetch is not a panic");
    let fetch_failed = stack.catalog.status().expect("status");
    assert_eq!(
        fetch_failed.state,
        aura_model_domain::ProviderCatalogState::RefreshFailedWithLkg,
        "a failed fetch is a failed refresh, never a successful one"
    );
    assert_eq!(
        fetch_failed.snapshot.as_ref().map(|s| s.catalog_content_hash.clone()),
        Some(lkg.catalog_content_hash.clone()),
        "a failed fetch preserves the last known good catalog"
    );
    assert_eq!(
        stack.sync.available(),
        SYNC_WORKERS,
        "the permit came back after the fetch failure"
    );

    // (c) The storage half fails: the LKG survives and the failure is not a success. The body
    // differs from the seeded LKG, so this really is the cache-write path and not revalidation.
    stack.fault.store(CATALOG_CACHE_FAILURE, Ordering::Release);
    stack.source.push(CatalogFetchStep::Body(S04_CATALOG_BODY_V2));
    let driver = stack.spawn_driver(&runtime);
    assert!(wait_until_async(Duration::from_secs(20), || !stack.catalog.refresh_in_progress()));
    runtime.block_on(driver).expect("a storage error is not a panic");
    let status = stack.catalog.status().expect("status");
    assert_eq!(
        status.state,
        aura_model_domain::ProviderCatalogState::RefreshFailedWithLkg,
        "a storage failure is a failed refresh, never a successful one"
    );
    assert_eq!(
        status.snapshot.as_ref().map(|s| s.catalog_content_hash.clone()),
        Some(lkg.catalog_content_hash.clone()),
        "the last known good catalog is preserved"
    );
    assert!(status.last_error_code.is_some(), "the real error code is published");
    // The failure is carried into the committed projection rather than being reported as a
    // successful refresh: this is the product's own state, not a log read.
    match stack.catalog_projection().expect("the failed refresh still commits its real state") {
        aura_projection_contract::ProjectionChange::ProviderCatalog { payload, .. } => {
            assert_eq!(
                payload.state,
                aura_model_domain::ProviderCatalogState::RefreshFailedWithLkg,
                "the committed projection says the refresh failed"
            );
            assert!(
                payload.last_error_code.is_some(),
                "the committed projection carries the real error code"
            );
        }
        other => panic!("expected a ProviderCatalog projection, got {other:?}"),
    }
    // The settlement record is the product's own separate record of the errors and of the
    // ownership release. The diagnostic route stays this case's file for as long as the stack
    // lives, so the record is this case's evidence.
    let log = stack.core_log();
    assert!(
        log.contains("provider_catalog.refresh_settled"),
        "the settlement outcome is recorded separately from the ownership release"
    );
    assert!(
        log.contains("injected catalog cache write failure"),
        "the real cache error is recorded as itself, not swallowed"
    );
    assert_eq!(stack.sync.available(), SYNC_WORKERS, "the permit came back");
    stack.fault.store(CATALOG_CACHE_PASSTHROUGH, Ordering::Release);

    // (d) The commit section panics: nothing is reported as settled and everything comes back.
    stack.fault.store(CATALOG_CACHE_PANIC, Ordering::Release);
    stack.source.push(CatalogFetchStep::Body(S04_CATALOG_BODY_V2));
    let driver = stack.spawn_driver(&runtime);
    assert!(
        wait_until_async(Duration::from_secs(20), || !stack.catalog.refresh_in_progress()),
        "a panicking section releases the occupancy from its own unwinding"
    );
    runtime.block_on(driver).expect("the driver itself is not the panicking task");
    let after_panic = stack.catalog.status().expect("status after the panic");
    assert_ne!(
        after_panic.state,
        aura_model_domain::ProviderCatalogState::Fresh,
        "a panicked section publishes no success"
    );
    assert_eq!(
        after_panic.state,
        aura_model_domain::ProviderCatalogState::RefreshingWithLkg,
        "the loading state `begin_refresh` published is what stands, not a fabricated success"
    );
    assert_eq!(
        after_panic.snapshot.as_ref().map(|s| s.catalog_content_hash.clone()),
        Some(lkg.catalog_content_hash.clone()),
        "the LKG survived the panic"
    );
    let recovered = stack.catalog.begin_refresh().expect("begin after the panic");
    assert!(recovered.is_some(), "the occupancy came back after the panic");
    drop(recovered);
    assert_eq!(stack.sync.available(), SYNC_WORKERS, "the panicked section's permit came back");
    println!(
        "S04C-CAT-4 refused_did_not_run=true refusal_released=true cache_error_state=refresh_failed_with_lkg \
         panic_state_unchanged=true permits_returned=true source_fetches={}",
        stack.source.fetches()
    );
}

/// How long each arriving write is parked while a configuration-maintenance case measures what
/// another session's control and the Core's heartbeat do inside that window.
///
/// Longer than `HOLD_MS` on purpose: the case has to observe a real parked arrival, run a real
/// control request and wait for a real heartbeat, and the claim it makes is that all of that
/// happened while the storage lane was still held. A window too short for the observation itself
/// would make the claim depend on how fast the test thread noticed the hold.
const MAINTENANCE_HOLD_MS: u64 = HOLD_MS + 2_400;

/// The real Core services a configuration-maintenance case drives.
struct MaintenanceHarness {
    harness: Harness,
    settings: Arc<SettingsFacade>,
    persistence: Arc<aura_persistence::PersistenceCoordinator>,
    sync: crate::sync_boundary::SyncBoundary,
}

impl MaintenanceHarness {
    fn new(runtime: &tokio::runtime::Runtime) -> Self {
        let harness = runtime.block_on(Harness::new(BUDGET_MS));
        let state = Arc::clone(&harness.state);
        let StartupMode::Ready(services) = &state.startup_mode else {
            panic!("Core must be ready");
        };
        Self {
            settings: Arc::clone(&services._settings),
            persistence: Arc::clone(&services.persistence),
            sync: services.sync.clone(),
            harness,
        }
    }

    /// A real Models configuration change through the real model config service, which is the
    /// same entry the `models.provider.create` IPC route uses. It publishes a real `Changed`
    /// event on the channel the P05 maintenance task subscribes to.
    fn create_model_provider(&self, display_name: &str) -> (u64, aura_domain_common::ProviderId) {
        let service = aura_model_config::ModelConfigService::new(Arc::clone(&self.settings));
        let revision = service.snapshot().expect("models snapshot").revision;
        let (snapshot, provider_id) = service
            .create_provider(
                revision,
                aura_model_domain::CreateModelProviderInput {
                    display_name: display_name.to_owned(),
                    source: aura_model_domain::ModelProviderSource::Custom {
                        catalog_provider_id: None,
                    },
                },
                UtcTimestamp::now(),
            )
            .expect("a real Models configuration change");
        (snapshot.revision, provider_id)
    }

    /// A real Tools configuration change through the real settings facade - the entry the config
    /// bundle transaction and the watcher both use - so the maintenance task sees a real event.
    fn set_max_concurrent_tool_calls(&self, value: u32) -> u64 {
        let before = self.settings.snapshot(ConfigModule::Tools).expect("tools snapshot");
        let revision = before.document.revision;
        let updated = self
            .settings
            .update(ConfigModule::Tools, revision, |values| {
                values
                    .insert(aura_config::MAX_CONCURRENT_TOOL_CALLS_FIELD.to_owned(), json!(value));
            })
            .expect("a real Tools configuration change");
        updated.document.revision
    }

    fn model_settings_projection(&self) -> Option<aura_projection_contract::ProjectionChange> {
        self.persistence
            .latest_model_projection(
                aura_projection_contract::ProjectionKind::ModelSettings,
                aura_projection_contract::ProjectionScope::Global,
            )
            .expect("latest model settings projection")
    }

    fn tool_settings_projection(&self) -> Option<aura_projection_contract::ProjectionChange> {
        self.persistence
            .latest_tool_projection(
                aura_projection_contract::ProjectionKind::ToolSettings,
                aura_projection_contract::ProjectionScope::Global,
            )
            .expect("latest tool settings projection")
    }

    /// The completed section spans of one maintenance method, oldest first.
    fn maintenance_spans(&self, method: &str) -> Vec<crate::sync_boundary::SyncSpan> {
        self.sync
            .spans()
            .into_iter()
            .filter(|span| span.method.as_deref() == Some(method))
            .collect()
    }

    /// The identity a reactor worker thread has in this runtime.
    ///
    /// The section's own recorded thread is only evidence that the work left the reactor if it is
    /// a *different* thread from the one the async consumer runs on, and this is that thread,
    /// measured rather than assumed from a name.
    fn reactor_worker_thread(runtime: &tokio::runtime::Runtime) -> String {
        runtime.block_on(async {
            tokio::spawn(async { thread_identity() })
                .await
                .expect("a reactor worker reports its own thread")
        })
    }
}

/// B1 - a real Models configuration change reaches storage on the boundary while another
/// session's ordinary control and the Core's own heartbeat keep running, and after release the
/// committed projection carries the real revision and content.
#[test]
fn a_models_configuration_change_reaches_storage_on_the_boundary_while_control_and_the_heartbeat_run()
 {
    let timeline = CaseTimeline::new();
    let runtime = test_runtime();
    let maintenance = MaintenanceHarness::new(&runtime);
    let state = Arc::clone(&maintenance.harness.state);
    let heartbeat = start_heartbeat(&runtime, Arc::clone(&state), &timeline);

    // The writer is single-threaded, so parking one job in it for the whole window is what makes
    // "inside the hold" a real state: every other write queues behind it and any request that
    // needs no write still answers.
    let hold = maintenance.persistence.arm_writer_hold_once(
        aura_persistence::barrier::Point::WriterExecution,
        None,
        MAINTENANCE_HOLD_MS,
    );
    let (models_revision, provider_id) = maintenance.create_model_provider("S04 B Models");
    assert!(
        wait_until(Duration::from_secs(15), || still_inside_hold(&hold).is_some()),
        "the Models maintenance section must really reach storage and park there"
    );
    // Another session's ordinary control, inside the hold: a real read-only product request
    // through the real handler.
    let inside_before = still_inside_hold(&hold).is_some();
    let control_started = Instant::now();
    let control =
        runtime.block_on(call(Arc::clone(&state), "models.settings.get_snapshot", json!({})));
    let control_ms = u64::try_from(control_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let inside_after = still_inside_hold(&hold).is_some();
    let served_holds: Vec<u64> =
        hold.arrivals().into_iter().filter(|a| a.held).map(|a| a.hold_micros).collect();
    assert_eq!(
        control.pointer("/result/terminal").and_then(Value::as_str),
        Some("succeeded"),
        "an ordinary control call really answers while the maintenance section is parked: {control}"
    );
    assert!(
        control.pointer("/result/value").is_some(),
        "the answered control call carries the real projection: {control}"
    );
    assert!(
        inside_before && inside_after,
        "the control call really answered inside the hold, not after it: control_ms={control_ms} \
         inside_before={inside_before} inside_after={inside_after} \
         served_holds_us={served_holds:?} hold_ms={MAINTENANCE_HOLD_MS}"
    );
    // The Core's own heartbeat, inside the same hold.
    assert!(
        wait_until(Duration::from_secs(8), || heartbeat.dequeued().is_some()),
        "the Core's heartbeat is delivered while the maintenance section is parked"
    );
    assert!(
        still_inside_hold(&hold).is_some(),
        "the heartbeat was dequeued strictly before the hold was released"
    );
    let heartbeat_instants = heartbeat.instants();
    heartbeat.join();

    // After release: the section really ran on the boundary and the projection really reflects
    // the change.
    //
    // The caller's own timed wait expires first and is recorded as its own `unknown` terminal
    // span; the work's real completion is the `completed` span the blocking thread writes when
    // it really ends. This case is about the second one.
    assert!(
        wait_until(Duration::from_secs(25), || maintenance
            .maintenance_spans("config.models.rebuild")
            .iter()
            .any(|span| span.outcome == "completed")),
        "the Models maintenance section really completed"
    );
    let completed: Vec<_> = maintenance
        .maintenance_spans("config.models.rebuild")
        .into_iter()
        .filter(|span| span.outcome == "completed")
        .collect();
    assert_eq!(completed.len(), 1, "exactly one Models maintenance section really ran");
    let span = completed.into_iter().next().expect("a completed Models maintenance section");
    assert!(
        span.elapsed_ms >= MAINTENANCE_HOLD_MS - 250,
        "the section's own elapsed time shows it really did its storage work inside the hold: {}",
        span.elapsed_ms
    );
    // The section is a real boundary section: the boundary records it, with its own method, by
    // the thread that ran it. (The boundary records the thread's NAME only, and this Tokio names
    // both its reactor workers and its blocking threads `tokio-rt-worker`, so the name is
    // deliberately NOT used here as evidence about which pool ran it. The pool evidence is that
    // this span exists at all - a direct synchronous call from the maintenance task would produce
    // none - together with the reactor staying responsive, which is asserted above by the control
    // call and the heartbeat completing inside the section's real hold.)
    assert!(
        !span.thread.is_empty() && !span.thread.starts_with("inline:"),
        "the section ran on the boundary's own worker, not inline on the caller's thread: {}",
        span.thread
    );
    let projection =
        maintenance.model_settings_projection().expect("a committed ModelSettings projection");
    let aura_projection_contract::ProjectionChange::ModelSettings { revision, payload, .. } =
        projection
    else {
        panic!("expected a ModelSettings projection")
    };
    assert_eq!(
        payload.models_revision.value(),
        models_revision,
        "the committed projection carries the real Models configuration revision"
    );
    assert!(
        payload.registry.providers.contains_key(&provider_id),
        "the committed projection really contains the provider the configuration change created"
    );
    assert!(
        maintenance.maintenance_spans("config.models.rebuild").len() >= 2,
        "the caller's timed wait and the work's real completion are recorded separately"
    );
    let reactor_thread = MaintenanceHarness::reactor_worker_thread(&runtime);
    println!(
        "S04C-CFG-1 models_revision={models_revision} projection_revision={} \
         section_elapsed_ms={} section_thread={} reactor_thread={reactor_thread} \
         heartbeat_delivered_ms={} control_ms={control_ms} control_in_hold=true",
        revision.value(),
        span.elapsed_ms,
        span.thread,
        heartbeat_instants.delivered_ms
    );
}

/// B2 - the same for Tools, whose maintenance path commits and publishes through the control
/// service.
#[test]
fn a_tools_configuration_change_reaches_storage_on_the_boundary_while_control_and_the_heartbeat_run()
 {
    let timeline = CaseTimeline::new();
    let runtime = test_runtime();
    let maintenance = MaintenanceHarness::new(&runtime);
    let state = Arc::clone(&maintenance.harness.state);
    let heartbeat = start_heartbeat(&runtime, Arc::clone(&state), &timeline);

    let hold = maintenance.persistence.arm_writer_hold_once(
        aura_persistence::barrier::Point::WriterExecution,
        None,
        MAINTENANCE_HOLD_MS,
    );
    let tools_revision = maintenance.set_max_concurrent_tool_calls(7);
    assert!(
        wait_until(Duration::from_secs(15), || still_inside_hold(&hold).is_some()),
        "the Tools maintenance section must really reach storage and park there"
    );
    let inside_before = still_inside_hold(&hold).is_some();
    let control_started = Instant::now();
    let control =
        runtime.block_on(call(Arc::clone(&state), "models.settings.get_snapshot", json!({})));
    let control_ms = u64::try_from(control_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let inside_after = still_inside_hold(&hold).is_some();
    assert_eq!(
        control.pointer("/result/terminal").and_then(Value::as_str),
        Some("succeeded"),
        "an ordinary control call really answers while the Tools section is parked: {control}"
    );
    assert!(
        inside_before && inside_after,
        "the control call really answered inside the hold: control_ms={control_ms} \
         inside_before={inside_before} inside_after={inside_after}"
    );
    assert!(
        wait_until(Duration::from_secs(8), || heartbeat.dequeued().is_some()),
        "the heartbeat is delivered while the Tools section is parked"
    );
    assert!(still_inside_hold(&hold).is_some(), "the heartbeat was dequeued inside the hold");
    let heartbeat_instants = heartbeat.instants();
    heartbeat.join();

    assert!(
        wait_until(Duration::from_secs(25), || maintenance
            .maintenance_spans("config.tools.publish")
            .iter()
            .any(|span| span.outcome == "completed")),
        "the Tools maintenance section really completed"
    );
    let completed: Vec<_> = maintenance
        .maintenance_spans("config.tools.publish")
        .into_iter()
        .filter(|span| span.outcome == "completed")
        .collect();
    assert_eq!(completed.len(), 1, "exactly one Tools maintenance section really ran");
    let span = completed.into_iter().next().expect("a completed Tools maintenance section");
    assert!(
        span.elapsed_ms >= MAINTENANCE_HOLD_MS - 250,
        "the Tools section really did its storage work inside the hold: {}",
        span.elapsed_ms
    );
    // The section is a real boundary section, recorded by the boundary with its own method. The
    // thread field is not used as pool evidence (both pools are named `tokio-rt-worker` in this
    // Tokio); the response inside the hold above is the reactor-is-free evidence.
    assert!(
        !span.thread.is_empty() && !span.thread.starts_with("inline:"),
        "the Tools section ran on the boundary's own worker, not inline on the caller's thread: {}",
        span.thread
    );
    let reactor_thread = MaintenanceHarness::reactor_worker_thread(&runtime);
    let projection =
        maintenance.tool_settings_projection().expect("a committed ToolSettings projection");
    let aura_projection_contract::ProjectionChange::ToolSettings { revision, payload, .. } =
        projection
    else {
        panic!("expected a ToolSettings projection")
    };
    assert_eq!(
        payload.settings.max_concurrent_tool_calls, 7,
        "the committed projection carries the value the configuration change really wrote"
    );
    assert!(
        payload.configuration_issue.is_none(),
        "an accepted Tools configuration publishes no issue"
    );
    let _ = tools_revision;
    println!(
        "S04C-CFG-2 tool_settings_projection_revision={} max_concurrent_tool_calls={} \
         section_elapsed_ms={} section_thread={} reactor_thread={reactor_thread} \
         heartbeat_delivered_ms={} control_ms={control_ms} control_in_hold=true \
         tools_config_revision={tools_revision}",
        revision.value(),
        payload.settings.max_concurrent_tool_calls,
        span.elapsed_ms,
        span.thread,
        heartbeat_instants.delivered_ms
    );
}

/// B3 - a saturating boundary refuses the maintenance section, the consumed configuration change
/// is not lost, and it is applied on a later poll tick once the boundary has room.
#[test]
fn a_refused_maintenance_section_keeps_the_consumed_configuration_change_and_applies_it_after_release()
 {
    let runtime = test_runtime();
    let maintenance = MaintenanceHarness::new(&runtime);
    let start_revision =
        maintenance.tool_settings_projection().map(|change| change.revision().value()).unwrap_or(0);

    // Fill every permit with real held sections, so the maintenance section is refused.
    let gate = Arc::new(AtomicU64::new(0));
    let mut holders = Vec::new();
    for _ in 0..SYNC_WORKERS {
        let held = maintenance.sync.clone();
        let gate = Arc::clone(&gate);
        holders.push(runtime.spawn(async move {
            held.run(SyncSection::Inspect, crate::sync_boundary::SyncSubject::new(), move || {
                let deadline = Instant::now() + Duration::from_secs(20);
                while gate.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
            .await
        }));
    }
    assert!(wait_until(Duration::from_secs(10), || maintenance.sync.available() == 0));

    // A real Tools configuration change, consumed while the boundary has no room at all.
    let _ = maintenance.set_max_concurrent_tool_calls(9);
    assert!(
        wait_until(Duration::from_secs(15), || maintenance.sync.spans().iter().any(|span| span
            .method
            .as_deref()
            == Some("config.tools.publish")
            && span.outcome == "refused")),
        "the maintenance section was really refused while the boundary was full"
    );
    assert!(
        maintenance.sync.counters(SyncSection::Projection).refused >= 1,
        "the boundary's own record says the section was refused"
    );

    // Release the boundary: the consumed change must still be applied, from the newest settings.
    gate.store(1, Ordering::Release);
    for holder in holders {
        runtime.block_on(holder).expect("the held sections end");
    }
    assert!(
        wait_until(Duration::from_secs(20), || maintenance.tool_settings_projection().is_some_and(
            |change| matches!(
                change,
                aura_projection_contract::ProjectionChange::ToolSettings { ref payload, .. }
                    if payload.settings.max_concurrent_tool_calls == 9
            )
        )),
        "the refused, already-consumed Tools change is applied after the boundary is released"
    );
    let applied_revision = maintenance
        .tool_settings_projection()
        .map(|change| change.revision().value())
        .expect("an applied projection");
    assert!(
        applied_revision > start_revision,
        "applying the change really committed a newer revision: {applied_revision} > {start_revision}"
    );
    let refused = maintenance
        .sync
        .spans()
        .iter()
        .filter(|span| span.method.as_deref() == Some("config.tools.publish"))
        .count();
    println!(
        "S04C-CFG-3 tool_settings_revision={applied_revision} max_concurrent_tool_calls=9 \
         refused_sections={} applied_after_release=true",
        refused
    );
}

/// B4 - several rapid real configuration changes are neither lost nor applied backwards.
#[test]
fn several_rapid_configuration_changes_are_neither_lost_nor_applied_backwards() {
    let runtime = test_runtime();
    let maintenance = MaintenanceHarness::new(&runtime);
    let observed = Arc::new(Mutex::new(Vec::<(u64, u32)>::new()));
    let sampler = {
        let observed = Arc::clone(&observed);
        let persistence = Arc::clone(&maintenance.persistence);
        let stop = Arc::new(AtomicU64::new(0));
        let sampler_stop = Arc::clone(&stop);
        let handle = runtime.spawn(async move {
            while sampler_stop.load(Ordering::Acquire) == 0 {
                if let Ok(Some(aura_projection_contract::ProjectionChange::ToolSettings {
                    revision,
                    payload,
                    ..
                })) = persistence.latest_tool_projection(
                    aura_projection_contract::ProjectionKind::ToolSettings,
                    aura_projection_contract::ProjectionScope::Global,
                ) && let Ok(mut observed) = observed.lock()
                {
                    observed.push((revision.value(), payload.settings.max_concurrent_tool_calls));
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
        });
        (handle, stop)
    };

    // Three rapid real Tools changes and three rapid real Models changes, with no waiting between
    // them: the consumer has to reflect the newest last-known-good state.
    let (_first_models, first_provider) = maintenance.create_model_provider("S04 B Rapid 1");
    let _ = maintenance.set_max_concurrent_tool_calls(2);
    let (_second_models, second_provider) = maintenance.create_model_provider("S04 B Rapid 2");
    let _ = maintenance.set_max_concurrent_tool_calls(4);
    let (last_models_revision, third_provider) = maintenance.create_model_provider("S04 B Rapid 3");
    let _ = maintenance.set_max_concurrent_tool_calls(6);

    assert!(
        wait_until(Duration::from_secs(30), || maintenance.tool_settings_projection().is_some_and(
            |change| matches!(
                change,
                aura_projection_contract::ProjectionChange::ToolSettings { ref payload, .. }
                    if payload.settings.max_concurrent_tool_calls == 6
            )
        )),
        "the newest Tools value really reaches the projection"
    );
    assert!(
        wait_until(Duration::from_secs(30), || maintenance
            .model_settings_projection()
            .is_some_and(|change| matches!(
                change,
                aura_projection_contract::ProjectionChange::ModelSettings { ref payload, .. }
                    if payload.models_revision.value() == last_models_revision
                        && payload.registry.providers.contains_key(&third_provider)
            ))),
        "the newest Models configuration really reaches the projection"
    );
    let (sampler_handle, stop) = sampler;
    stop.store(1, Ordering::Release);
    runtime.block_on(sampler_handle).expect("the sampler ends");

    let samples = observed.lock().map(|observed| observed.clone()).unwrap_or_default();
    assert!(samples.len() >= 3, "the sampler observed the projection: {}", samples.len());
    for window in samples.windows(2) {
        let (before_revision, before_value) = window[0];
        let (after_revision, after_value) = window[1];
        assert!(
            after_revision >= before_revision,
            "projection revisions never move backwards: {before_revision} -> {after_revision}"
        );
        assert!(
            after_value >= before_value,
            "the Tools value never flows backwards: {before_value} -> {after_value} ({samples:?})"
        );
    }
    // Every provider created by the rapid Models changes is in the newest registry, so none of
    // the three changes was dropped.
    let aura_projection_contract::ProjectionChange::ModelSettings { payload, .. } =
        maintenance.model_settings_projection().expect("a committed ModelSettings projection")
    else {
        panic!("expected a ModelSettings projection")
    };
    for provider in [first_provider, second_provider, third_provider] {
        assert!(
            payload.registry.providers.contains_key(&provider),
            "a rapid Models change was dropped: {provider}"
        );
    }
    println!(
        "S04C-CFG-4 samples={} final_tool_value=6 final_models_revision={} \
         providers_reflected=3 monotonic=true",
        samples.len(),
        payload.models_revision.value()
    );
}

#[path = "session_settlement_tests.rs"]
mod session_settlement_tests;
