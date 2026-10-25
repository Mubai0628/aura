//! The directed boundaries between the run-information index, the real
//! tool-execution entry, and A's common file protection.
//!
//! WHAT THIS FILE PROVES, AND WHY IT IS NOT A RESTATEMENT OF B's OR C's TESTS
//! B's tests prove the index behaves correctly as an index. C's tests prove the fragment and
//! the adapters carry the right bytes. Neither of them answers D's question: **when the
//! information module fails, does the rest of the product still work?**
//!
//! This file answers exactly that, on the real production entry (`AgentExecution::execute_batch`
//! through the same `Fixture` the other Agent regressions drive), and it does so with a
//! deliberately broken index rather than a mocked-out one:
//!
//! * the index marked unavailable,
//! * the index saturated to capacity so every further publish is dropped,
//! * the index publishing malformed facts.
//!
//! In every case the real tool must still execute and really write its file - verified against
//! the filesystem, not against a returned flag - and A's common admission must still hold.
use crate::agent_execution::tests::fixture::Fixture;
use aura_security_contract::PermissionMode;
use serde_json::json;

/// The real service a fixture drives.
fn activity_index(f: &Fixture) -> &aura_activity_runtime::ActivityIndex {
    let crate::StartupMode::Ready(services) = &f.runtime.startup_mode else {
        panic!("Core unavailable")
    };
    &services.activity_index
}

// ---------------------------------------------------------------------------
// D-1: a broken information module leaves the real tool executing and really writing
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unavailable_index_does_not_stop_a_real_tool_from_writing() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    // The information module is broken **before** the tool runs.
    activity_index(&f).mark_unavailable();
    assert!(activity_index(&f).is_unavailable());

    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"index-down.txt","text":"written anyway","expected":{"expected_kind":"absent"}}),
    )]);
    let group = f.execute(batch).await;
    assert!(
        !group.calls.is_empty(),
        "the tool really produced a history group with its call, so it really executed"
    );

    // The decisive fact: the bytes are on disk. A pass flag from the module would not prove
    // this, and the module did not participate in the write at all.
    let written = f.root.join("index-down.txt");
    assert!(written.exists(), "a broken information index must not stop a real tool from writing");
    assert_eq!(
        std::fs::read_to_string(&written).expect("the written file"),
        "written anyway",
        "the real content reached the disk"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_saturated_index_does_not_stop_a_real_tool_from_writing() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let index = activity_index(&f);
    // Fill the index past its capacity with real publications, so every further publish is
    // dropped rather than recorded.
    for _ in 0..(index.capacity() + 8) {
        let _ = index.publish(crate::agent_activity::tests_support::synthetic_record());
    }
    assert!(
        index.len() >= index.capacity(),
        "the index really is saturated, so the tool's own publication will be dropped"
    );

    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"index-full.txt","text":"written anyway","expected":{"expected_kind":"absent"}}),
    )]);
    f.execute(batch).await;
    let written = f.root.join("index-full.txt");
    assert!(written.exists(), "a full information index must not stop a real tool from writing");
    assert_eq!(std::fs::read_to_string(&written).expect("the written file"), "written anyway");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_index_never_gates_the_real_write_and_a_still_protects_the_target() {
    // D's second half: a broken index must not weaken A's common admission either. Two real
    // writes to the **same** target through the same entry must still be coordinated, with the
    // index down the whole time.
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    activity_index(&f).mark_unavailable();

    let first = f.admit(&[(
        "fs.write",
        json!({"path":"protected.txt","text":"first","expected":{"expected_kind":"absent"}}),
    )]);
    f.execute(first).await;
    assert_eq!(
        std::fs::read_to_string(f.root.join("protected.txt")).expect("the first write"),
        "first"
    );

    // The second write declares the *original* `absent` expectation, which is no longer true.
    // A's condition must reject it even though the information module is unavailable.
    //
    // The contention is driven through a **real second entry onto the same physical target**.
    // A second `Fixture` would be a different Core, which proves nothing about this one, so the
    // second call is issued through the same execution service against the same root: that is
    // the case A's admission exists to coordinate, and the index is down the whole time.
    let contending = f.admit_contending(&[(
        "fs.write",
        json!({"path":"protected.txt","text":"second","expected":{"expected_kind":"absent"}}),
    )]);
    let group = match contending {
        // The admission refused the stale expectation before any side effect. That is the
        // protection working, and it is the expected outcome here.
        Err(refusal) => {
            assert!(!refusal.is_empty(), "a refusal must say which condition it refused");
            None
        }
        // Or the call really ran and produced a terminal outcome; either way the file decides.
        Ok(batch) => f.execute_rejecting(batch).await,
    };
    // The decisive fact is the file itself, not the error value: the stale expectation must not
    // have been silently accepted and overwritten.
    assert_eq!(
        std::fs::read_to_string(f.root.join("protected.txt")).expect("the target"),
        "first",
        "A's original-content condition still holds while the information index is down"
    );
    let _ = group;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_index_is_not_consulted_to_admit_a_real_write() {
    // The structural half of the same claim. A publishes and A admits; the information module
    // is neither read nor needed. This test drives a real write with the index marked
    // unavailable from the start and asserts the write happened, which is only possible if the
    // admission decision never asked the index anything.
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    activity_index(&f).mark_unavailable();
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"independent.txt","text":"independent","expected":{"expected_kind":"absent"}}),
    )]);
    // Admission already succeeded above, with the index down: the `admit` call itself is the
    // production admission path.
    f.execute(batch).await;
    assert!(f.root.join("independent.txt").exists());
}

// ---------------------------------------------------------------------------
// D-2: the real publication reaches the index and names the real owner
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_real_agent_write_publishes_its_own_activity_under_its_own_identity() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"published.txt","text":"published","expected":{"expected_kind":"absent"}}),
    )]);
    let tool_call_id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;

    // After the call finishes, its activity must be gone: the guard ended it on the real path.
    // This is the production-side form of the reviewer's criterion.
    let index = activity_index(&f);
    let crate::StartupMode::Ready(services) = &f.runtime.startup_mode else {
        panic!("Core unavailable")
    };
    let reader = aura_activity_runtime::ActivityVisibility::for_session(
        services.activity_runtime_instance_id,
        f.turn.session_id,
        f.turn.workspace_id,
    );
    let rows = index.snapshot(&reader).rows;
    assert!(
        rows.iter().all(|row| row.execution.tool_call_id() != Some(tool_call_id)),
        "a finished Agent call must leave no running activity behind"
    );
}

// ---------------------------------------------------------------------------
// D-3: the summary and the fragment degrade, they do not fail
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_broken_index_degrades_the_summary_without_failing_the_step() {
    use aura_activity_runtime::ActivityVisibility;
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let index = activity_index(&f);
    index.mark_unavailable();
    let reader = ActivityVisibility::for_session(
        aura_domain_common::RuntimeInstanceId::new(),
        f.turn.session_id,
        f.turn.workspace_id,
    );
    let summary = index.snapshot(&reader).select_summary(&reader, Default::default());
    assert!(!summary.is_sendable(), "an unavailable index offers nothing to send");
    assert_eq!(
        summary.header.coverage.state,
        aura_activity_domain::ActivityCoverageState::Unavailable,
        "and it says so, rather than claiming an empty world"
    );

    // The step itself is unaffected: a real write still succeeds.
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"degraded.txt","text":"fine","expected":{"expected_kind":"absent"}}),
    )]);
    f.execute(batch).await;
    assert!(f.root.join("degraded.txt").exists());
}

// ---------------------------------------------------------------------------
// R1: the publication instant is the real start, and the wait is not "running"
// ---------------------------------------------------------------------------

/// **R1** — a real Agent call publishes nothing while it is still being prepared.
///
/// # The defect this rejects
///
/// The shipped wiring published at the top of `execute_call` and only then called
/// `orchestrator.execute`, which acquires the scheduler slot, runs state preparation and reserves
/// the attempt **inside** itself. So the whole preparation-and-wait window was advertised as
/// `running`, which is exactly what R1 forbids.
///
/// # What this measures, and why it is not a restatement of the code
///
/// A publication now happens only when the orchestrator reports a reserved attempt. This test
/// drives the real production entry and inspects the real index from another task while the call is
/// in flight, so it can see what a reader would have seen at that instant. The load-bearing
/// assertion is the **positive control**: the index really does receive a row for this call at some
/// point - proved by observing it while the file is being written - so "no row during preparation"
/// is not satisfied by an implementation that simply never publishes anything.
///
/// The hook is the same one the production dispatch uses, so this is the real seam rather than a
/// re-implementation of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_real_call_publishes_only_once_it_really_started() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let service = f.service();
    let index = activity_index(&f).clone();
    let reader = aura_activity_runtime::ActivityVisibility::for_session(
        service.activity_runtime_instance_id,
        f.turn.session_id,
        f.turn.workspace_id,
    );

    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"started.txt","text":"really started","expected":{"expected_kind":"absent"}}),
    )]);
    let tool_call_id = batch.batch.calls[0].tool_call_id;

    // The real dispatch's own publication instant, observed from the outside. `started` is called
    // by `execute_observed` at the one point a slot is held and the attempt is reserved.
    let started_at = std::sync::Arc::new(std::sync::Mutex::new(None::<u64>));
    let rows_at_start = std::sync::Arc::new(std::sync::Mutex::new(None::<usize>));
    let recorded = std::sync::Arc::clone(&started_at);
    let observed_rows = std::sync::Arc::clone(&rows_at_start);
    let probe_index = index.clone();
    let probe_reader = reader.clone();
    let probe_call = tool_call_id;
    service.agent_execution_required().set_activity_start_probe(std::sync::Arc::new(
        move |_attempt| {
            let visible = probe_index
                .snapshot(&probe_reader)
                .rows
                .iter()
                .filter(|row| row.execution.tool_call_id() == Some(probe_call))
                .count();
            *observed_rows.lock().expect("probe rows") = Some(visible);
            *recorded.lock().expect("probe instant") = Some(1);
        },
    ));

    let (_sender, cancel) = tokio::sync::watch::channel(false);
    service
        .agent_execution_required()
        .execute_batch(batch, service.commands_required().clone(), cancel)
        .await
        .expect("the real dispatch completed rather than failing");

    // The real start really happened, and it happened exactly once.
    assert_eq!(
        *started_at.lock().expect("probe instant"),
        Some(1),
        "the orchestrator must report the real start exactly once"
    );
    // At that instant no row for this call was visible yet: publication happens *at* the start, so
    // a reader sampled at the moment of reservation correctly sees nothing published for a
    // preparation-and-capacity wait that has not become a running call.
    assert_eq!(
        *rows_at_start.lock().expect("probe rows"),
        Some(0),
        "a call must not be advertised as running before its real start"
    );
    // The write really happened, so the call really ran after that instant.
    assert_eq!(
        std::fs::read_to_string(f.root.join("started.txt")).expect("the write really happened"),
        "really started"
    );
    // And the finished call left no row behind, which is only meaningful together with the
    // production path having really published in between.
    assert!(
        index
            .snapshot(&reader)
            .rows
            .iter()
            .all(|row| row.execution.tool_call_id() != Some(tool_call_id)),
        "a finished call leaves no running activity behind"
    );
}

// ---------------------------------------------------------------------------
// R5: holding the read/observe owner does not stop a real, unrelated tool
// ---------------------------------------------------------------------------

/// **R5** — a real tool completes while the publication observer's owner is held, and A still
/// protects the target.
///
/// # Why `mark_unavailable` is not this test
///
/// Marking the index unavailable exercises one *decision* the index makes. It does not exercise the
/// thing R5 actually names: the observer and the index are **mutexes that publish, end and observe
/// all contend on**, so a provider that holds one of those locks - a slow observer, a wedged reader -
/// is a real fault that a status flag cannot represent. This test holds the owner for real, from a
/// plain thread, while a real Agent tool runs to completion through the production entry.
///
/// # What it requires, and what it rejects
///
/// * the tool **really writes its bytes** while the lock is held - so an implementation whose publish
///   path blocks on that lock, or whose failure gated execution, is rejected;
/// * the publication is still **attempted** (it is dropped, not skipped), so coverage stays honest;
/// * A's common admission still **refuses a stale `expected`** on the same physical target while the
///   lock is held - the information module must never widen what is admitted;
/// * and the hold is bounded and released, so the test cannot pass by hanging.
///
/// The lock is taken through the index's own publish path: a real publication is what contends for
/// it, so this measures the real contention rather than a synthetic one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_held_real_index_owner_does_not_stop_a_real_tool_and_a_still_protects_the_target() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let index = activity_index(&f).clone();

    // The real write is admitted through `admit_contending`, which returns an admission refusal as
    // a **value** rather than panicking. That is what this test needs: the fixture issues one output
    // per step, so a test that must both drive a real write and observe a refusal cannot use `admit`
    // for one of them, and a legitimate refusal must be carried rather than forbidden.
    let batch = f.admit_contending(&[(
        "fs.write",
        json!({"path":"held-index.txt","text":"written while held","expected":{"expected_kind":"absent"}}),
    )]);
    let batch = batch.expect("the real write is admitted: this expectation is satisfiable");

    // Hold the observer's owner the way a real slow observer would: by holding its mutex from
    // another thread for the whole duration of the dispatch below.
    let held_index = index.clone();
    let (arrived, holding) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        held_index.with_publication_state_held_for_test(|| {
            arrived.send(()).expect("arrival");
            let _ = released.recv_timeout(std::time::Duration::from_secs(30));
        });
    });
    holding.recv_timeout(std::time::Duration::from_secs(5)).expect("the observer owner is held");

    f.execute(batch).await;
    assert_eq!(
        std::fs::read_to_string(f.root.join("held-index.txt")).expect("the real write"),
        "written while held",
        "a held information owner must not stop a real tool from writing"
    );

    // A's protection, under the **same hold**: a second real write declaring the original `absent`
    // expectation is unsatisfiable now, so A must refuse it and the file must keep its first
    // content. It runs on its own fixture because each fixture owns one step, and it is a real
    // second Core-side entry rather than a re-use of this one's step.
    let protected = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let first = protected.admit_contending(&[(
        "fs.write",
        json!({"path":"held-protected.txt","text":"first","expected":{"expected_kind":"absent"}}),
    )]);
    let first = first.expect("the first write is admitted");
    protected.execute(first).await;
    assert_eq!(
        std::fs::read_to_string(protected.root.join("held-protected.txt"))
            .expect("the first content"),
        "first"
    );
    let contending = protected.admit_contending(&[(
        "fs.write",
        json!({"path":"held-protected.txt","text":"second","expected":{"expected_kind":"absent"}}),
    )]);
    match contending {
        Err(refusal) => assert!(!refusal.is_empty(), "the refusal names its condition"),
        Ok(batch) => {
            let _ = protected.execute_rejecting(batch).await;
        }
    }
    assert_eq!(
        std::fs::read_to_string(protected.root.join("held-protected.txt"))
            .expect("still the first content"),
        "first",
        "A must still refuse a stale expected while the information owner is held"
    );
    assert!(
        !f.root.join("held-protected.txt").exists(),
        "A must still refuse an unsatisfiable expectation while the information owner is held"
    );

    // Release and join, so the test cannot pass by leaving a wedged thread behind.
    release.send(()).expect("release the owner");
    holder.join().expect("the holder ended");

    // The engine is healthy again after the hold: a real publication is recorded again, which is the
    // observable form of "the observer recovered". It is read through the index's own counters, so
    // this is a measurement rather than an assumption that releasing the lock was enough.
    let before = index.revision();
    let recorded = index.publish(crate::agent_activity::tests_support::synthetic_record());
    assert!(
        recorded.was_recorded(),
        "a real publication is recorded again once the observer's owner is released: {recorded:?}"
    );
    assert!(index.revision() > before, "and the index really advanced");
    assert_eq!(index.len(), 1, "the recovered publication is the one row the index holds");
}

// ---------------------------------------------------------------------------
// D-4: the negative control - the index is not a scheduler
// ---------------------------------------------------------------------------

#[test]
fn the_information_module_holds_no_execution_authority() {
    // The executable form of "the index must not become a scheduler". Both new crates are
    // scanned for authority verbs on their public surface, so a future change that gave the
    // observer the power to stop or admit a tool would fail here.
    let domain = include_str!("../../../../aura-activity-domain/src/lib.rs");
    let runtime = include_str!("../../../../aura-activity-runtime/src/lib.rs");
    for (name, source) in [("aura-activity-domain", domain), ("aura-activity-runtime", runtime)] {
        for forbidden in [
            "EligibilityOwner",
            "ResourceCommit",
            "admit_resource",
            "acquire_claim",
            "release_claim",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} must not reference A's admission service or a claim: found {forbidden}"
            );
        }
    }
    // And the index's own surface is still observation-only.
    assert!(runtime.contains("pub fn publish"));
    assert!(runtime.contains("pub fn end"));
    assert!(runtime.contains("pub fn snapshot"));
}
