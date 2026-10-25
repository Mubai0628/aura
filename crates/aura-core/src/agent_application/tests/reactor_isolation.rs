//! The Agent's real storage work must not park the Core's reactor.
//!
//! WHY THESE CASES EXIST
//! A real Agent request to the local HTTP Provider fixture returned an SSE response and then waited
//! on the shared V2 journal mutex. The 100 ms timer scheduled on the same single Tokio worker did
//! not run at all while the mutex was held and fired about 1181 ms after it was released: the
//! reactor that has to deliver the heartbeat and answer another session was parked on storage.
//!
//! WHAT THESE CASES MEASURE, AND WHAT THEY PROVE
//! * the setup is the production one - the real Agent, the real Provider transport over the real
//!   local HTTP fixture, the real journal, authorisation and lifecycle. Nothing is substituted
//!   except the external Provider's answer, which is what a fixture is for.
//! * the journal is held by a plain thread, not by a task, so the hold is a real lock wait and not
//!   a simulated one.
//! * the timer runs on the **only** reactor worker of the runtime, so "the timer fired" is a
//!   statement about the reactor and not about a spare thread.
//! * the second case runs the identical sequence with the one Store owner forced to run its
//!   sections inline, which is the pre-boundary behaviour. It asserts the **counterfactual**: with
//!   the boundary removed the timer really is blocked. Together the two cases show the isolation
//!   comes from the boundary, not from the call sites merely looking asynchronous.
use super::*;
use provider_fixture::Scenario;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The observation window. Long enough that a 100 ms timer has many chances to fire.
const OBSERVE_MS: u64 = 1_000;
const TIMER_MS: u64 = 100;
/// A timer that really fired while the journal was held is far below this.
const TIMER_BOUND_MS: u64 = 500;

/// What one run of the sequence observed. `while_held` is `u64::MAX` when the timer never ran
/// inside the observation window, which is the failure this whole module exists to catch.
struct Observation {
    requests: usize,
    while_held: u64,
    after_release: u64,
}

fn until(within: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + within;
    while !ready() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

/// Drives the real production chain once and reports what the reactor did.
fn observe_a_real_provider_response_with_the_journal_held(inline_boundary: bool) -> Observation {
    // ONE worker on purpose: a parked worker is then directly observable as a timer that did not
    // run, instead of being absorbed by a spare thread.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("one reactor worker");
    let host = crate::r03_tests::Fixture::new();
    let provider = runtime.block_on(Provider::scenario(Scenario::LateAnswer));
    let (state, session, model) = runtime.block_on(waiting::prepare(&host, &provider));
    let crate::StartupMode::Ready(services) = &state.startup_mode else { panic!("Core ready") };
    if inline_boundary {
        // The injected pre-boundary behaviour. Compiled and reachable only from a regression.
        services.agent_required().probe_force_inline_store_boundary(true);
    }
    let started = runtime.block_on(waiting::start(&state, session, model, AgentTaskMode::Normal));
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    assert!(
        until(Duration::from_secs(20), || !provider.requests.lock().expect("requests").is_empty()),
        "the real local HTTP Provider fixture must be asked"
    );

    // The real shared journal, held by a plain thread: not a task, not a substitute lock.
    let journal = Arc::clone(&services.broker);
    let (arrived, held) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _lock = journal.lock().expect("real shared journal");
        arrived.send(()).expect("arrival");
        let _ = released.recv_timeout(Duration::from_secs(30));
    });
    held.recv_timeout(Duration::from_secs(5)).expect("the journal is really held");

    provider.release_window.notify_one();
    // Give the real HTTP response a chance to reach the transport's storage callbacks.
    std::thread::sleep(Duration::from_millis(200));

    let ticked = Arc::new(AtomicU64::new(u64::MAX));
    let tick = Arc::clone(&ticked);
    let began = Instant::now();
    let timer = runtime.spawn(async move {
        tokio::time::sleep(Duration::from_millis(TIMER_MS)).await;
        tick.store(
            u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX),
            Ordering::Release,
        );
    });
    std::thread::sleep(Duration::from_millis(OBSERVE_MS));
    let while_held = ticked.load(Ordering::Acquire);
    release.send(()).expect("release the real lock");
    holder.join().expect("holder ended");
    runtime.block_on(timer).expect("the timer really ended");
    let after_release = ticked.load(Ordering::Acquire);

    let requests = provider.requests.lock().expect("requests").len();
    // Stop the legitimate fixture run after the observation, so nothing is left admitted.
    let _ = runtime.block_on(services.agent_required().cancel(&request));
    runtime.block_on(host.stop(state));
    drop(runtime);
    Observation { requests, while_held, after_release }
}

/// R1 - the production boundary keeps the reactor free while a real Provider answer waits on the
/// real journal.
#[test]
fn s04_r1_a_real_provider_response_never_parks_the_reactor_on_the_shared_journal() {
    let observed = observe_a_real_provider_response_with_the_journal_held(false);
    println!(
        "S04-R1 reactor_isolation requests={} timer_while_held={} timer_after_release_ms={}",
        observed.requests, observed.while_held, observed.after_release
    );
    assert_eq!(observed.requests, 1, "exactly one real Provider request, never a replay");
    assert!(
        observed.while_held <= TIMER_BOUND_MS,
        "a real Provider response parked the only reactor worker until the shared journal was \
         released: timer_while_held={} (never ran = {}) after_release_ms={}",
        observed.while_held,
        u64::MAX,
        observed.after_release
    );
}

/// R1 - the counterfactual: with the same call sites but the boundary removed, the timer really is
/// blocked. This is what makes the case above a measurement instead of an assertion about syntax.
#[test]
fn s04_r1_the_isolation_comes_from_the_bounded_owner_not_from_the_call_sites() {
    let observed = observe_a_real_provider_response_with_the_journal_held(true);
    println!(
        "S04-R1 reactor_isolation_counterfactual inline=true requests={} timer_while_held={} \
         timer_after_release_ms={}",
        observed.requests, observed.while_held, observed.after_release
    );
    assert_eq!(observed.requests, 1, "the sequence itself is unchanged");
    assert!(
        observed.while_held > TIMER_BOUND_MS,
        "running the same sections inline must block the only reactor worker; measured \
         timer_while_held={} after_release_ms={}",
        observed.while_held,
        observed.after_release
    );
}

/// The permission-approval hold, as it was really measured.
struct ApprovalObservation {
    /// The reactor timer's own elapsed time, sampled while the approval section was parked.
    while_held: u64,
    /// How long the real `agent.approval.resolve` call took end to end.
    resolve_ms: u64,
    /// Whether the store's own hold point recorded the arrival at all.
    arrived: bool,
    /// Whether the approval really took effect: the run completed through the approved call.
    completed: bool,
}

/// How long the approval section is parked by the store's own hold point.
const APPROVAL_HOLD_MS: u64 = 900;

/// Drives the real permission-approval path once, with the approval section held at the Store
/// owner's own hold point, and reports what the reactor did.
///
/// WHY THIS IS A REAL HOLD POINT AND NOT A SCENARIO AROUND ONE
/// `AgentApplication::resolve_approval` runs ONE ordered synchronous critical section - the session
/// permission authority first, then the journal - under the step identity
/// `run=<agent_run_id>/agent.approval.resolve`. The hold below is armed on exactly that identity
/// through `AgentStore::arm_hold`, which is the same barrier the acceptance driver binds from
/// outside the process; the arrival is read back from the barrier's own records, so "the section
/// was really parked" is a measured fact rather than a timing guess. Everything else is the
/// production chain: the real approval route, the real Provider fixture, the real journal and the
/// real run loop.
fn observe_the_permission_approval_section_held(inline_boundary: bool) -> ApprovalObservation {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("one reactor worker");
    let host = crate::r03_tests::Fixture::new();
    let provider = runtime.block_on(Provider::scenario(Scenario::ExternalList));
    let (state, session, model) = runtime.block_on(waiting::prepare(&host, &provider));
    let crate::StartupMode::Ready(services) = &state.startup_mode else { panic!("Core ready") };
    if inline_boundary {
        services.agent_required().probe_force_inline_store_boundary(true);
    }
    // ReadOnly has no directory grant, so listing an external directory asks first: that is the
    // approval the section below resolves.
    let external = host.root.path().join("outside-approval-hold");
    std::fs::create_dir_all(&external).expect("Agent test setup and persisted assertion");
    std::fs::write(
        external.join(format!("{}.txt", provider_fixture::EXTERNAL_LIST_TOKEN)),
        "listed",
    )
    .expect("Agent test setup and persisted assertion");
    let started: AgentRunProjection = runtime.block_on(async {
        let value = crate::r03_tests::success(
            &state,
            "agent.start",
            json!(AgentStartRequest {
                session_id: session,
                model_profile_id: model,
                text: format!(
                    "List AURA_EXTERNAL_PATH={} and report its entries.",
                    external.display()
                ),
                mode: AgentTaskMode::Normal,
                goal_budget: None,
                explicit_backend_id: None,
                skill_ids: Vec::new(),
                reasoning_effort: None,
                attachments: Vec::new(),
            }),
        )
        .await;
        serde_json::from_value(value).expect("Agent test setup and persisted assertion")
    });
    let run = started.run.agent_run_id;
    let request = AgentRunRequest { session_id: session, agent_run_id: run };

    let status_with_approval = runtime.block_on(async {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let view: AgentRunProjection = serde_json::from_value(
                crate::r03_tests::success(&state, "agent.status", json!(&request)).await,
            )
            .expect("Agent test setup and persisted assertion");
            if let Some(id) = view.pending_approval_ids.first().copied() {
                return (view, id);
            }
            assert!(
                !view.turn.state.terminal(),
                "the run ended before the ReadOnly listing asked for approval"
            );
            assert!(Instant::now() < deadline, "no approval was ever requested");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    });
    let approval_id = status_with_approval.1;
    let approval = runtime.block_on(crate::r03_tests::success(
        &state,
        "agent.approval.get",
        json!({"session_id": session, "approval_id": approval_id}),
    ));

    // The store's own hold point, armed on the approval section's step identity.
    let barrier = services
        .agent_required()
        .store()
        .arm_hold(&format!("run={run}/agent.approval.resolve"), APPROVAL_HOLD_MS);

    let state = Arc::new(state);
    let resolve_state = Arc::clone(&state);
    let payload = json!({
        "session_id": session,
        "approval_id": approval_id,
        "expected_revision": approval["approval"]["revision"],
        "decision": "allow_session",
    });
    let began = Instant::now();
    let resolving = runtime.spawn(async move {
        crate::r03_tests::success(&resolve_state, "agent.approval.resolve", payload).await
    });
    let arrived = until(Duration::from_secs(10), || !barrier.arrivals().is_empty());

    let ticked = Arc::new(AtomicU64::new(u64::MAX));
    let tick = Arc::clone(&ticked);
    let timer_began = Instant::now();
    let timer = runtime.spawn(async move {
        tokio::time::sleep(Duration::from_millis(TIMER_MS)).await;
        tick.store(
            u64::try_from(timer_began.elapsed().as_millis()).unwrap_or(u64::MAX),
            Ordering::Release,
        );
    });
    runtime.block_on(timer).expect("the timer really ended");
    let while_held = ticked.load(Ordering::Acquire);
    runtime.block_on(resolving).expect("the approval section answered");
    let resolve_ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);

    // The approval really took effect: the run completes through the call the user allowed.
    let completed = runtime.block_on(async {
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            let view: AgentRunProjection = serde_json::from_value(
                crate::r03_tests::success(&state, "agent.status", json!(&request)).await,
            )
            .expect("Agent test setup and persisted assertion");
            if view.turn.state.terminal() {
                return matches!(view.turn.state, TurnTaskState::Completed { .. });
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    });
    // Let the run's own hand-over settle inside the runtime before the runtime is torn down: a
    // supervisor hand-over that is still pending when the runtime goes away has nothing to await,
    // and this keeps the observation free of teardown noise.
    runtime.block_on(async {
        tokio::time::sleep(Duration::from_millis(300)).await;
    });
    // The resolve task has ended, so this is the last owner of the runtime state.
    let state = Arc::try_unwrap(state).unwrap_or_else(|_| panic!("runtime state still shared"));
    runtime.block_on(host.stop(state));
    drop(runtime);
    ApprovalObservation { while_held, resolve_ms, arrived, completed }
}

/// R1 - the permission-approval section is a real hold point, and holding it does not park the
/// reactor that has to answer the rest of the process.
#[test]
fn s04_r1_the_permission_approval_section_is_held_without_parking_the_reactor() {
    let observed = observe_the_permission_approval_section_held(false);
    println!(
        "S04-R1 approval_hold arrived={} timer_while_held={} resolve_ms={} completed={}",
        observed.arrived, observed.while_held, observed.resolve_ms, observed.completed
    );
    assert!(
        observed.arrived,
        "the approval section was never reached: the hold point is not on the production path"
    );
    assert!(
        observed.resolve_ms >= APPROVAL_HOLD_MS,
        "the section must really have been parked for its hold budget; measured resolve_ms={}",
        observed.resolve_ms
    );
    assert!(
        observed.while_held <= TIMER_BOUND_MS,
        "holding the permission-approval section parked the only reactor worker: \
         timer_while_held={} (never ran = {})",
        observed.while_held,
        u64::MAX
    );
    assert!(
        observed.completed,
        "the approval must really take effect through the bounded section, not merely return"
    );
}

/// R1 - the counterfactual for the approval path: with the boundary forced inline, the same section
/// on the same hold point really does park the reactor. Without this the case above would be an
/// assertion about syntax rather than a measurement.
#[test]
fn s04_r1_the_approval_isolation_comes_from_the_bounded_owner_not_from_the_call_sites() {
    let observed = observe_the_permission_approval_section_held(true);
    println!(
        "S04-R1 approval_hold_counterfactual inline=true arrived={} timer_while_held={} \
         resolve_ms={} completed={}",
        observed.arrived, observed.while_held, observed.resolve_ms, observed.completed
    );
    assert!(observed.arrived, "the hold point is reached on this path too");
    assert!(
        observed.while_held > TIMER_BOUND_MS,
        "running the approval section inline must block the only reactor worker; measured \
         timer_while_held={} resolve_ms={}",
        observed.while_held,
        observed.resolve_ms
    );
}
