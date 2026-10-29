//! A read on a kept read-only connection knows which one it runs on, so what that connection
//! read can be kept with what the connection saw (`agent_application::history_memo`).
use super::super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_knows_the_kept_connection_it_runs_on() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("Core is ready")
    };
    let store = Arc::clone(services.agent_required().store());
    let lent = store
        .read(StoreStep::operation("test.lent_reader"), |_db| Ok(lent_reader()))
        .await
        .expect("the read");
    assert!(lent.is_some(), "the read ran on a kept read-only connection");
    assert_eq!(lent_reader(), None, "no read runs on this thread");
}
