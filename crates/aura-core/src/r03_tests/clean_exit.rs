//! Main's quit asks Core to record a normal exit, and
//! the next start checks only the Store's structure before it serves, then its quick check while
//! it serves. A start after an exit nobody announced owes every page, index and key, and runs
//! that check while the Store serves too.
use super::*;
use aura_persistence_core::schema::PendingCheck;

fn marker(host: &Fixture) -> PathBuf {
    host.paths.data_dir.join("v2-execution").join("clean-exit.json")
}

fn pending_check(runtime: &RuntimeState) -> PendingCheck {
    let StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.broker.lock().expect("V2 store").pending_check()
}

async fn prepare_exit(runtime: &RuntimeState) -> bool {
    success(runtime, "runtime.prepare_exit", json!({})).await["marked_clean"]
        .as_bool()
        .expect("whether the exit was recorded")
}

#[tokio::test]
async fn an_announced_exit_lets_the_next_start_check_quickly_and_an_unannounced_one_does_not() {
    let host = Fixture::new();
    let runtime = host.start().await;
    // A Store made by this start was checked as it was made: its exit is recorded at once.
    assert!(prepare_exit(&runtime).await);
    assert!(marker(&host).exists());

    let runtime = host.restart(runtime).await;
    assert_eq!(pending_check(&runtime), PendingCheck::Quick);
    assert!(!marker(&host).exists(), "the start spent the marker");
    // The quick check waits until the start is over; a quit before it has run still records
    // the normal exit, and the next start runs it again.
    assert!(prepare_exit(&runtime).await);
    assert!(marker(&host).exists());

    // Core ended without being asked (a crash, or a kill from outside): everything is owed, and
    // Core serves while it is checked.
    std::fs::remove_file(marker(&host)).expect("an unannounced end leaves no marker");
    let runtime = host.restart(runtime).await;
    assert_eq!(pending_check(&runtime), PendingCheck::Full);
}
