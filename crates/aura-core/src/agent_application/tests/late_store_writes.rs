//! Writes that finish later than their callers expected, as on a busy machine where one long
//! background section keeps the store occupied. A step's record that outlives its caller's
//! bounded wait still commits under its own permit: the run follows it to its real end and goes
//! on, instead of failing the step it already has. A run's failure record that waited behind a
//! later change to the run is stamped when it really runs, so it is not refused as going back in
//! time; refused, it left the run standing at its tools with nothing running them.
use super::*;
use crate::agent_store::ReceiptState;
use std::time::Duration;

const LATE_RECORD_WRITE: Scenario = Scenario::Batch {
    calls: &[(
        "fs.write",
        r#"{"path":"late/answer.txt","text":"late\n","expected":{"expected_kind":"absent"}}"#,
    )],
};

fn agent(runtime: &crate::RuntimeState) -> Arc<crate::agent_application::AgentApplication> {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.agent_required()
}

async fn until(deadline: Duration, predicate: impl Fn() -> bool) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < deadline {
        if predicate() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    predicate()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_step_record_that_outlives_its_wait_is_followed_and_its_tool_still_runs() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(LATE_RECORD_WRITE).await;
    let (runtime, session, model) =
        prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let agent = agent(&runtime);
    // The record of the step is held past its caller's wait, then commits.
    runtime.sync.set_budget(Duration::from_millis(3_000));
    let hold = agent.store().arm_hold("agent.model.output_record", 60_000);
    let start = super::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    assert!(until(Duration::from_secs(30), || hold.was_reached()).await, "the record was held");
    assert!(
        until(Duration::from_secs(15), || agent.store().counters().unknown > 0).await,
        "its caller's wait ended before the record committed"
    );
    hold.release_all();
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let committed = agent.store().receipts().into_iter().any(|receipt| {
        receipt.operation == "agent.model.output_record"
            && receipt.state == ReceiptState::Settled { committed: true, code: None }
    });
    assert!(committed, "the late record really committed");
    let written = host.root.path().join("waiting-workspace").join("late").join("answer.txt");
    assert_eq!(std::fs::read_to_string(written).expect("the tool ran"), "late\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failure_record_queued_behind_a_later_change_still_ends_the_run() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Hold).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Normal).await;
    let run = start.run.agent_run_id;
    let request = AgentRunRequest { session_id: start.turn.session_id, agent_run_id: run };
    let agent = agent(&runtime);
    assert!(
        until(Duration::from_secs(20), || {
            !provider.requests.lock().expect("provider requests").is_empty()
        })
        .await,
        "the step is waiting for its answer"
    );
    let hold = agent.store().arm_hold("agent.drive_failure_record", 60_000);
    let failing = Arc::clone(&agent);
    let record = tokio::spawn(async move {
        failing.persist_drive_failure_for_test(run, "agent.test_failure").await;
    });
    assert!(until(Duration::from_secs(10), || hold.was_reached()).await, "the record is queued");
    // Another write changes the run while the record waits, as a late step record did.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let later = serde_json::to_value(UtcTimestamp::now()).expect("timestamp");
    {
        let crate::StartupMode::Ready(services) = &runtime.startup_mode else { unreachable!() };
        let journal = services.broker.lock().expect("journal");
        journal
            .database
            .connection()
            .execute(
                "UPDATE agent_runs SET payload=json_set(payload,'$.updated_at',?1) WHERE id=?2",
                rusqlite::params![later.as_str().expect("text"), run.to_string()],
            )
            .expect("a later change to the run");
    }
    hold.release_all();
    tokio::time::timeout(Duration::from_secs(20), record).await.expect("recorded").expect("task");
    let projection = agent.status(&request).await.expect("projection");
    assert!(
        matches!(projection.turn.state, TurnTaskState::RecoveryRequired { .. }),
        "the failure ended the run: {:?} {:?}",
        projection.turn.state,
        agent.store().diagnostics().of(run)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_queued_behind_a_later_change_still_stops_the_run() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Hold).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Normal).await;
    let run = start.run.agent_run_id;
    let request = AgentRunRequest { session_id: start.turn.session_id, agent_run_id: run };
    let agent = agent(&runtime);
    assert!(
        until(Duration::from_secs(20), || {
            !provider.requests.lock().expect("provider requests").is_empty()
        })
        .await,
        "the step is waiting for its answer"
    );
    let hold = agent.store().arm_hold("agent.interrupt", 60_000);
    let stopping = Arc::clone(&agent);
    let stop = tokio::spawn(async move { stopping.interrupt(run, "agent.cancelled", true).await });
    assert!(until(Duration::from_secs(10), || hold.was_reached()).await, "the stop is queued");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let later = serde_json::to_value(UtcTimestamp::now()).expect("timestamp");
    {
        let crate::StartupMode::Ready(services) = &runtime.startup_mode else { unreachable!() };
        let journal = services.broker.lock().expect("journal");
        journal
            .database
            .connection()
            .execute(
                "UPDATE agent_runs SET payload=json_set(payload,'$.updated_at',?1) WHERE id=?2",
                rusqlite::params![later.as_str().expect("text"), run.to_string()],
            )
            .expect("a later change to the run");
    }
    hold.release_all();
    let stopped = tokio::time::timeout(Duration::from_secs(20), stop).await.expect("ran");
    assert!(stopped.expect("task").is_ok(), "the stop was recorded");
    let projection = agent.status(&request).await.expect("projection");
    assert!(matches!(projection.turn.state, TurnTaskState::Cancelled), "{:?}", projection.turn);
}
