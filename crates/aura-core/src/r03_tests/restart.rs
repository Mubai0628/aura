//! Same-process restart must reclaim the old generation before opening its files again.
use super::*;

impl Fixture {
    pub(crate) async fn restart(&self, runtime: RuntimeState) -> RuntimeState {
        self.stop(runtime).await;
        self.start().await
    }

    pub(crate) async fn stop(&self, mut runtime: RuntimeState) {
        let deadline = Instant::now() + Duration::from_secs(30);
        if let StartupMode::Ready(services) = &mut runtime.startup_mode {
            for task in [&mut services._p05_maintenance, &mut services._p07_maintenance] {
                if let Some(mut task) = task.take() {
                    task.0.abort();
                    let result = tokio::time::timeout_at(deadline.into(), &mut task.0)
                        .await
                        .expect("old maintenance task must really end before restart");
                    assert!(result.is_ok() || result.is_err_and(|error| error.is_cancelled()));
                }
            }
            if let Some(agent) = services.agent.get() {
                agent.quiesce_for_test(deadline).await;
            }
            if let Some(commands) = services.commands.get() {
                commands.shutdown();
            }
            tokio::time::timeout_at(deadline.into(), async {
                while services.sync.available() != services.sync.capacity() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("old blocking sections must really end before restart");
            let persistence = Arc::clone(&services.persistence);
            let health = tokio::task::spawn_blocking(move || {
                persistence.close_writer(deadline.saturating_duration_since(Instant::now()))
            })
            .await
            .expect("writer close task");
            assert!(
                health.joined && matches!(health.state, aura_persistence::WriterState::Stopped),
                "old writer must be stopped and joined: {health:?}"
            );
        }
        tokio::task::spawn_blocking(move || drop(runtime)).await.expect("old runtime drop");
    }
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn restart_waits_for_a_held_writer_and_its_background_owner() {
    let host = Arc::new(Fixture::new());
    let mut runtime = host.start().await;
    let StartupMode::Ready(services) = &mut runtime.startup_mode else { panic!("Core ready") };
    if let Some(mut task) = services._p07_maintenance.take() {
        task.0.abort();
        let _ = (&mut task.0).await;
    }
    let persistence = Arc::clone(&services.persistence);
    let hold = persistence.arm_writer_hold_once(
        aura_persistence::barrier::Point::WriterPostCommit,
        Some("sync"),
        30_000,
    );
    let ended = Arc::new(AtomicBool::new(false));
    let task_ended = Arc::clone(&ended);
    services._p07_maintenance =
        Some(RuntimeBackgroundTask(tokio::task::spawn_blocking(move || {
            persistence.commit_current_tool_global_list_projections().expect("real writer job");
            task_ended.store(true, Ordering::Release);
        })));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !hold.was_reached() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the real writer reached the hold");
    let restarting_host = Arc::clone(&host);
    let entered = Arc::new(tokio::sync::Notify::new());
    let task_entered = Arc::clone(&entered);
    let restarting = tokio::spawn(async move {
        task_entered.notify_one();
        restarting_host.restart(runtime).await
    });
    entered.notified().await;
    let started_while_held = tokio::time::timeout(Duration::from_secs(3), async {
        while host.start_count() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .is_ok();
    hold.release_all();
    let runtime = restarting.await.expect("restart task");
    assert!(
        !started_while_held,
        "a new generation started while the old writer still held its connection"
    );
    assert!(ended.load(Ordering::Acquire), "the background owner really ended");
    assert!(matches!(runtime.startup_mode, StartupMode::Ready(_)), "restart must be ready");
    host.stop(runtime).await;
}
