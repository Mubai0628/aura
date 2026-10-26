//! A workspace's file index is walked again when the file tree asks for it.
use super::*;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_workspace_index_is_walked_again_on_request() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let root = host.root.path().join("rebuild-index");
    std::fs::create_dir(&root).expect("workspace folder");
    std::fs::write(root.join("a.txt"), "a").expect("a file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root": root, "display_name": "Rebuild index"}),
    )
    .await;
    let workspace = created["workspace"]["workspace_id"].clone();
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let files = Arc::clone(&services.p03().expect("workspace services").workspace);
    let waited = Arc::clone(&files);
    tokio::task::spawn_blocking(move || waited.wait_until_indexed(Duration::from_secs(60)))
        .await
        .expect("the wait")
        .expect("the first walk");
    let started = crate::r03_tests::success(
        &runtime,
        "agent.workspace.rebuild_index",
        json!({"workspace_id": workspace}),
    )
    .await;
    assert_eq!(started["workspace_id"], workspace);
    assert_eq!(started["started"], true);
    tokio::task::spawn_blocking(move || files.wait_until_indexed(Duration::from_secs(60)))
        .await
        .expect("the wait")
        .expect("the requested walk");
}
