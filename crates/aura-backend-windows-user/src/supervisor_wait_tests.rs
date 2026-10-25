//! Authority-claim wait races: the launch must end its `begin_launch` wait on
//! the live stop signal and the launch window themselves, not on the
//! authority answering. The rig and its doubles live in `super::tests`; this
//! module only holds the cases that pin the wait-ownership rules, including
//! the ones where the scripted gate is never released at all.
#![allow(clippy::unwrap_used)]
use super::tests::{Rig, drive_to_execute_until_ready, finished_observation, wait_until};
use super::*;
use std::sync::Arc;
use tokio::sync::Notify;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_during_begin_launch_wait_never_executes() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    // The authority claim is held open and never released. The wait itself
    // ends on the live stop signal: no Execute frame exists, no Cancel frame
    // follows, and the launch settles as a clean NotStarted::Cancelled.
    let gate = Arc::new(Notify::new());
    rig.port.configure_begin_launch(Some(gate.clone()), None);
    let task = rig.spawn_run(300);
    drive_to_execute_until_ready(&rig).await;
    rig.cancelled.send(true).unwrap();
    let evidence = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("the claim wait ends on its own; the gate is never released")
        .unwrap()
        .expect("cancelled launch settles");
    assert!(matches!(evidence, TerminationEvidence::Confirmed { .. }));
    assert_eq!(rig.runner.execute_count(), 0, "no Execute frame after the stop signal");
    assert_eq!(rig.runner.cancel_count(), 0, "no Cancel frame without an Execute");
    assert_eq!(rig.port.begin_launch_calls(), 1);
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Cancelled));
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launch_window_expiring_during_begin_launch_wait_settles_expired() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    // The claim is held open, the clock jumps past the launch window and no
    // stop signal is sent. The wait ends on the window itself: Expired, with
    // no Execute and no Cancel frame.
    let gate = Arc::new(Notify::new());
    rig.port.configure_begin_launch(Some(gate.clone()), None);
    let task = rig.spawn_run(60);
    drive_to_execute_until_ready(&rig).await;
    rig.advance_clock_ms(120_000);
    let evidence = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("the claim wait ends on its own; the gate is never released")
        .unwrap()
        .expect("expired launch settles");
    assert!(matches!(evidence, TerminationEvidence::Confirmed { .. }));
    assert_eq!(rig.runner.execute_count(), 0);
    assert_eq!(rig.runner.cancel_count(), 0);
    assert_eq!(rig.port.begin_launch_calls(), 1);
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Expired));
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_after_begin_launch_wait_returns_keeps_conservative_semantics() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    // The claim returns on its own; the stop signal arrives only after the
    // Execute frame is on the wire. That late-stop case is unchanged: exactly
    // one Execute, one Cancel follows it, and the launch settles once through
    // the direct-observation path (the sibling case in supervisor_tests.rs).
    let gate = Arc::new(Notify::new());
    rig.port.configure_begin_launch(Some(gate.clone()), None);
    let task = rig.spawn_run(300);
    drive_to_execute_until_ready(&rig).await;
    rig.port.allow_begin_launch();
    gate.notify_waiters();
    wait_until("execute frame", || rig.runner.execute_count() == 1).await;
    rig.cancelled.send(true).unwrap();
    wait_until("cancel frame", || rig.runner.cancel_count() == 1).await;
    let process_id = std::process::id();
    rig.runner.send(RunnerCoreMessage::Started {
        attempt: rig.request.binding.attempt.clone(),
        process_id,
    });
    rig.runner.send(RunnerCoreMessage::Completed {
        observation: Box::new(finished_observation(&rig, process_id)),
    });
    let evidence = task.await.unwrap().expect("launch settles once");
    assert!(matches!(evidence, TerminationEvidence::Confirmed { .. }));
    assert_eq!(rig.runner.execute_count(), 1, "exactly one Execute, never retried");
    assert_eq!(rig.runner.cancel_count(), 1, "exactly one Cancel follows it");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, None);
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}
