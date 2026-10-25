use super::*;
use std::time::{Duration, Instant};

struct Gate(Arc<aura_fs_tools::test_faults::ScanGate>);
impl Drop for Gate {
    fn drop(&mut self) {
        self.0.release();
    }
}

async fn search() -> (Fixture, AdmittedBatch, Gate) {
    let name = format!("{}-held.txt", CorrelationId::new());
    let f =
        Fixture::with_exposure_setup(PermissionMode::WorkspaceReadWrite, &["fs.search"], |root| {
            std::fs::write(root.join(&name), "held search marker")
                .expect("write the fixture content for this permission boundary");
        })
        .await;
    let gate = Gate(aura_fs_tools::test_faults::arm_blocking_gate(&name));
    let batch = f.admit(&[(
        "fs.search",
        json!({"root":".",
        "pattern":{"kind":"literal","text":"marker","case_sensitive":true},
        "path_glob":null,"include_ignored":false,"include_generated":false,"limit":100}),
    )]);
    (f, batch, gate)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s06_live_read_mode_loosen_does_not_wait_for_a_running_search() {
    let (f, batch, gate) = search().await;
    let engine = f.service().agent_execution_required();
    let commands = f.service().commands_required().clone();
    let (_parent, cancel) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(async move { engine.execute_batch(batch, commands, cancel).await });
    let observed = Arc::clone(&gate.0);
    assert!(
        tokio::task::spawn_blocking(move || observed.wait_for_parked(1, Duration::from_secs(3)))
            .await
            .expect("the blocking observer waits for the real file reader without panicking")
    );
    let old = f
        .service()
        .persistence
        .session_permission_state(f.turn.session_id)
        .expect("read the current session permission revision");
    let release = Arc::clone(&gate.0);
    let bounded = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        release.release();
    });
    let began = Instant::now();
    let response = crate::r03_tests::request(
        &f.runtime,
        "permission.set_session_mode",
        json!({
            "session_id":f.turn.session_id,"expected_revision":old.revision,"mode":"full_access",
        }),
    )
    .await;
    let elapsed = began.elapsed();
    bounded.join().expect("join the fixture worker after its real execution settles");
    task.await
        .expect("the managed file batch task settles without panicking")
        .expect("the released file reader completes its real tool batch");
    assert!(response.get("error").is_none(), "{response}");
    assert!(
        elapsed < Duration::from_millis(250),
        "a running read held the mode setter for {elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s06_live_caller_cancellation_does_not_claim_a_blocked_file_worker_has_ended() {
    let (f, batch, gate) = search().await;
    let engine = f.service().agent_execution_required();
    let execution = Arc::clone(&engine);
    let commands = f.service().commands_required().clone();
    let (parent, cancel) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(async move { execution.execute_batch(batch, commands, cancel).await });
    let observed = Arc::clone(&gate.0);
    assert!(
        tokio::task::spawn_blocking(move || observed.wait_for_parked(1, Duration::from_secs(3)))
            .await
            .expect("the asynchronous fixture operation completes within its budget")
    );
    parent.send_replace(true);
    // Model the run owner's bounded interruption after requesting cancellation.
    // The detached backend still owns a real blocked OS read.
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    let active = engine
        .active_calls
        .session(f.turn.session_id)
        .expect("inspect the managed calls for this session");
    let still_tracked = active.iter().any(|call| !*call.ended.borrow());
    gate.0.release();
    assert!(still_tracked, "the actual blocked worker disappeared from managed tracking");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s06_live_stop_deadline_includes_the_authority_lookup() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"prepared.txt","text":"no effect",
        "expected":{"expected_kind":"absent"}}),
    )]);
    let engine = f.service().agent_execution_required();
    let (_parent, cancel) = tokio::sync::watch::channel(false);
    let (_lifetime, _cancel) = engine
        .active_calls
        .register(
            batch.calls.values().next().expect("the admitted batch contains the requested call"),
            cancel,
        )
        .expect("register the managed execution with its cancellation owner");
    f.service()
        .persistence
        .set_session_permission_mode(
            f.turn.session_id,
            1,
            PermissionMode::ReadOnly,
            CorrelationId::new(),
        )
        .expect("the admitted batch contains the requested call");
    let journal = Arc::clone(&f.service().broker);
    let (started, entered) = tokio::sync::oneshot::channel();
    let hold = std::thread::spawn(move || {
        let _lock = journal.lock().expect("lock the existing authority store for inspection");
        started.send(()).expect("signal that the bounded-stop fixture has acquired its lock");
        std::thread::sleep(Duration::from_millis(300));
    });
    entered.await.expect("the asynchronous fixture operation completes within its budget");
    let began = Instant::now();
    let result = engine
        .stop_permissions_removed(
            f.turn.session_id,
            PermissionMode::WorkspaceReadWrite,
            began + Duration::from_millis(40),
        )
        .await;
    let elapsed = began.elapsed();
    hold.join().expect("join the fixture worker after its real execution settles");
    assert!(
        result
            .expect_err("an unconfirmed managed stop must report stop_pending")
            .to_string()
            .contains("stop_pending")
    );
    assert!(
        elapsed < Duration::from_millis(150),
        "authority lookup escaped the stop deadline: {elapsed:?}"
    );
    assert!(!f.root.join("prepared.txt").exists());
}
