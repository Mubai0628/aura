//! A Workspace lifecycle operation that fails leaves its code, and the error beneath the code, in
//! Core's own log.
use super::workspace_moves::{commit_without_follow_up, run_without_follow_up};
use super::*;
use aura_domain_common::DeleteConfirmationId;
use aura_workspace_domain::{WorkspaceLifecycleOperation, WorkspaceLifecycleState};

#[tokio::test]
async fn a_failed_workspace_deletion_leaves_the_store_error_in_cores_log() {
    let host = Fixture::new();
    let log = host.root.path().join("core.jsonl");
    // Set before the start, so the route covers the services the start opens.
    let _route = crate::CoreLogRoute::to(log.clone());
    let runtime = host.start().await;
    let StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let folder = host.root.path().join("doomed");
    fs::create_dir(&folder).expect("the Workspace folder");
    let created = success(
        &runtime,
        method::WORKSPACE_CREATE,
        json!({"selected_root":folder,"display_name":"Doomed"}),
    )
    .await;
    let workspace: WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("the created Workspace");
    commit_without_follow_up(
        services,
        workspace.workspace_id,
        WorkspaceLifecycleOperation::Archive,
    );
    rusqlite::Connection::open(services.persistence.database_path())
        .expect("open the Store")
        .execute_batch(
            "CREATE TRIGGER refuse_workspace_deletion BEFORE DELETE ON workspaces
             BEGIN SELECT RAISE(ABORT, 'injected workspace deletion failure'); END;",
        )
        .expect("make the deletion fail");

    let failed = run_without_follow_up(
        services,
        workspace.workspace_id,
        WorkspaceLifecycleOperation::PermanentDelete {
            delete_confirmation_id: DeleteConfirmationId::new(),
        },
    );
    assert_eq!(failed.state, WorkspaceLifecycleState::Failed, "{failed:?}");

    let operation_id = failed.operation_id.to_string();
    let text = fs::read_to_string(&log).expect("read Core's log");
    let entry = text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|entry| {
            entry["event"] == "workspace.lifecycle_failure"
                && entry["fields"]["operation_id"] == operation_id.as_str()
        })
        .expect("the failed deletion is in Core's log");
    assert_eq!(entry["component"], "core");
    assert_eq!(entry["fields"]["error_code"], "workspace.lifecycle_commit_failed");
    assert!(
        entry["fields"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("injected workspace deletion failure")),
        "{entry}"
    );
}
