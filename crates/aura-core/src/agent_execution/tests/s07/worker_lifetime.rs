use super::*;
use std::time::{Duration, Instant};

struct Release(aura_persistence::barrier::Barrier);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release_all();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s07_n11_a_timed_out_download_worker_remains_managed_until_it_really_ends() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::WorkspaceReadWrite, &["http.download"]).await;
    let batch = f.admit(&[(
        "http.download",
        json!({"url":format!("{}/binary/10",server.url),"path":"held.txt"}),
    )]);
    let engine = f.service().agent_execution_required();
    let hold = Release(engine.store.arm_hold("agent.http.download_chunk", 20_000));
    f.runtime.sync.set_budget(Duration::from_millis(300));
    let execution = Arc::clone(&engine);
    let commands = f.service().commands_required().clone();
    let (_parent, cancel) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(async move { execution.execute_batch(batch, commands, cancel).await });
    let observed = hold.0.clone();
    assert!(
        tokio::task::spawn_blocking(move || observed.wait_for_arrivals(1, Duration::from_secs(5)))
            .await
            .expect("observe the real held staging worker")
    );
    let _terminal = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("the caller's Store wait is bounded")
        .expect("the tool batch does not panic");
    let tracked = engine
        .active_calls
        .session(f.turn.session_id)
        .expect("managed download workers")
        .iter()
        .any(|entry| !*entry.ended.borrow());
    f.service()
        .persistence
        .set_session_permission_mode(
            f.turn.session_id,
            1,
            PermissionMode::ReadOnly,
            CorrelationId::new(),
        )
        .expect("tighten the default while the worker is still held");
    let stopped = engine
        .stop_permissions_removed(
            f.turn.session_id,
            PermissionMode::WorkspaceReadWrite,
            Instant::now() + Duration::from_millis(100),
        )
        .await;
    hold.0.release_all();
    assert!(tracked, "a timed-out caller must not remove the real filesystem worker");
    assert!(
        stopped
            .expect_err("a held worker is not a confirmed stop")
            .to_string()
            .contains("stop_pending")
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while !engine
            .active_calls
            .session(f.turn.session_id)
            .expect("managed workers after release")
            .is_empty()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the released staging worker eventually retires");
    assert!(!f.root.join("held.txt").exists(), "an unknown staging wait must not publish later");
}
