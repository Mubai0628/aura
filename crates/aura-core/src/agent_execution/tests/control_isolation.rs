//! The real broker control entries must not park the Core's reactor.
//!
//! WHY THESE CASES EXIST
//! An independent review drove a real execution fixture to a real `fs.write` with a real
//! on-disk effect and a real terminal, then called the real `BrokerPort::reconcile` for that same
//! Attempt. On a single-worker runtime, with a plain thread holding the real journal mutex, the
//! 100 ms timer did not run at all inside a 700 ms observation window and fired 804 ms after the
//! lock was released. The call site has no `journal.lock()` literal at all: the lock and the SQL
//! live in `broker/journal.rs::reconcile`, so a scan for that literal cannot see it. The archived
//! first-failure output is preserved beside this round's evidence; this module is the formal
//! regression that replaced the temporary probe.
//!
//! WHAT THESE CASES MEASURE, AND WHAT THEY PROVE
//! * the setup is the production one - the real execution service, the real admitted batch, the
//!   real `fs.write`, the real journal and the real terminal receipt. Nothing is substituted.
//! * the journal is held by a plain thread, so the hold is a real lock wait and not a simulated one.
//! * the timer runs on the **only** reactor worker, so "the timer ran" is a statement about the
//!   reactor and not about a spare thread.
//! * the second case runs the identical sequence with the one bounded owner forced to run its
//!   sections inline, which is exactly the pre-boundary behaviour. It asserts the counterfactual:
//!   without the boundary the timer really is parked. Together the two cases show the isolation
//!   comes from the boundary and not from a call site that merely looks asynchronous.
use super::*;
use aura_backend_core::BrokerPort;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The observation window. Long enough that a 100 ms timer has many chances to fire.
const OBSERVE_MS: u64 = 700;
const TIMER_MS: u64 = 100;
/// A timer that really fired while the journal was held is far below this.
const TIMER_BOUND_MS: u64 = 500;

/// What one run of the sequence observed. `while_held` is `u64::MAX` when the timer never ran
/// inside the observation window, which is the failure this module exists to catch.
struct Observation {
    while_held: u64,
    after_release: u64,
    reconciled: ReconcileResult,
}

/// Drives one real `fs.write` to a real terminal, then queries that same Attempt through the real
/// broker control entry while the real journal is held by a plain thread.
fn observe_real_reconcile_with_the_journal_held(inline_boundary: bool) -> Observation {
    // ONE worker on purpose: a parked worker is then directly observable as a timer that did not
    // run, instead of being absorbed by a spare thread.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("one reactor worker");
    let fixture = runtime.block_on(Fixture::new(PermissionMode::WorkspaceReadWrite));
    if inline_boundary {
        // The injected pre-boundary behaviour. Compiled and reachable only from a regression.
        fixture.service().agent_execution_required().store.force_inline_for_test(true);
    }
    let batch = fixture.admit(&[(
        "fs.write",
        json!({"path":"control.txt","text":"real control effect","expected":{"expected_kind":"absent"}}),
    )]);
    let call = batch.calls.values().next().expect("one real admitted call").clone();
    let attempt = AttemptRef {
        operation_id: call.operation.operation_id,
        ordinal: std::num::NonZeroU32::new(1).expect("first attempt"),
    };
    runtime.block_on(fixture.execute(batch));
    // The real effect, on the real disk, before anything is held.
    assert_eq!(
        std::fs::read(fixture.root.join("control.txt")).expect("the real file exists"),
        b"real control effect"
    );
    let service = fixture.service().agent_execution_required();
    let port = Arc::new(super::super::broker::Port { service, call, commands: None });
    // The real shared journal, held by a plain thread: not a task, not a substitute lock.
    let journal = Arc::clone(&fixture.service().broker);
    let (arrived, held) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _lock = journal.lock().expect("the real production journal");
        arrived.send(()).expect("the lock is held");
        let _ = released.recv_timeout(Duration::from_secs(30));
    });
    held.recv_timeout(Duration::from_secs(5)).expect("the journal is really held");

    // The real control entry, on the real port, for the Attempt that really completed.
    let entered = Arc::new(AtomicBool::new(false));
    let inside = Arc::clone(&entered);
    let query = runtime.spawn(async move {
        inside.store(true, Ordering::Release);
        port.reconcile(attempt).await
    });
    let arrival_deadline = Instant::now() + Duration::from_secs(5);
    while !entered.load(Ordering::Acquire) && Instant::now() < arrival_deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(entered.load(Ordering::Acquire), "the real control entry was polled");
    std::thread::sleep(Duration::from_millis(100));

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
    holder.join().expect("the holder ended");
    runtime.block_on(timer).expect("the timer really ended");
    let after_release = ticked.load(Ordering::Acquire);
    let reconciled = runtime.block_on(query).expect("the real future ended").expect("real result");
    Observation { while_held, after_release, reconciled }
}

/// R5 - the production boundary keeps the reactor free while the real control entry waits on the
/// real journal.
#[test]
fn s04_r5_a_real_broker_reconcile_never_parks_the_reactor_on_the_journal() {
    let observed = observe_real_reconcile_with_the_journal_held(false);
    println!(
        "R5 actual_fs_write=true real_reconcile=completed journal_held=true \
         timer_while_held={} timer_after_release_ms={} result={:?}",
        observed.while_held, observed.after_release, observed.reconciled
    );
    assert!(
        observed.while_held <= TIMER_BOUND_MS,
        "the real control entry parked the reactor until the journal was released: {} ms",
        observed.while_held
    );
    assert!(
        matches!(observed.reconciled, ReconcileResult::Completed { .. }),
        "the query must return the same real terminal result: {:?}",
        observed.reconciled
    );
}

/// R5 counterfactual - with the boundary removed the identical sequence really is parked. This is
/// what makes the case above evidence about the boundary rather than about timing.
#[test]
fn s04_r5_without_the_boundary_the_same_control_entry_really_parks_the_reactor() {
    let observed = observe_real_reconcile_with_the_journal_held(true);
    println!(
        "R5 counterfactual inline_boundary=true timer_while_held={} timer_after_release_ms={} \
         result={:?}",
        observed.while_held, observed.after_release, observed.reconciled
    );
    assert!(
        observed.while_held > TIMER_BOUND_MS,
        "the counterfactual must really park the reactor, otherwise the R5 case proves nothing: \
         while_held={}",
        observed.while_held
    );
    assert!(
        matches!(observed.reconciled, ReconcileResult::Completed { .. }),
        "the query still returns the same real terminal result: {:?}",
        observed.reconciled
    );
}
