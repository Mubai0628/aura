//! A folder `.gitignore` excludes is not entered by the index, yet the file tree and the model
//! still see what is in it: both listings read it from disk.
use std::sync::Arc;

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspacePath};
use aura_fs_tools::{FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::WorkspaceRuntime;
use aura_workspace_domain::{
    AgentFileListInput, FsListRequest, LargeFileReadMode, SessionSummary, WorkspaceCreateCommand,
    WorkspaceFileTarget,
};

fn path(value: &str) -> WorkspacePath {
    WorkspacePath::parse(value).expect("workspace path")
}

#[test]
fn the_file_tree_and_the_model_list_an_ignored_folder_from_disk() {
    let root = tempfile::tempdir().expect("workspace root");
    std::fs::write(root.path().join(".gitignore"), "target/\n").expect(".gitignore");
    std::fs::create_dir_all(root.path().join("target/debug")).expect("ignored folders");
    std::fs::write(root.path().join("target/a.txt"), "built").expect("ignored file");
    let data = tempfile::tempdir().expect("data root");
    let PersistenceStartup::Ready(persistence) =
        PersistenceCoordinator::start(PersistenceOptions::new(
            data.path().join("aura.sqlite3"),
            data.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence startup")
    else {
        panic!("the store must be ready")
    };
    let persistence = Arc::new(persistence);
    let workspace =
        WorkspaceRuntime::open(Arc::clone(&persistence), Default::default()).expect("runtime");
    let workspace_id = workspace
        .create(
            WorkspaceCreateCommand {
                selected_root: root.path().display().to_string(),
                display_name: "ignored listing".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create workspace")
        .workspace
        .workspace_id;
    workspace.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
    let session_id = SessionId::new();
    persistence
        .upsert_workspace_session(
            persistence.workspace(workspace_id).expect("workspace"),
            SessionSummary {
                session_id,
                workspace_id,
                title: "listing".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("session");
    let fs = FsToolService::new(
        Arc::clone(&workspace),
        Arc::clone(&persistence),
        FsToolPolicy {
            large_file_read_mode: LargeFileReadMode::RequireRange,
            full_text_read_limit_bytes: 1024,
            small_range_line_limit: 8,
            small_range_character_limit: 256,
            default_binary_range_bytes: 4 * 1024,
            max_binary_range_bytes: 64 * 1024,
            max_changeset_operations: 64,
            max_search_results: 1000,
            mutation_wait: std::time::Duration::from_secs(5),
        },
    )
    .expect("fs service");
    assert!(
        workspace
            .catalog(workspace_id)
            .expect("catalog")
            .entry(&path("target/a.txt"))
            .expect("read")
            .is_none(),
        "the index does not enter the ignored folder"
    );
    let tree = fs
        .list(FsListRequest::first_page(
            workspace_id,
            session_id,
            WorkspaceFileTarget { path: path("target") },
        ))
        .expect("list for the file tree");
    let names = tree.entries.iter().map(|entry| entry.path.as_str()).collect::<Vec<_>>();
    assert_eq!(names, ["target/a.txt", "target/debug"]);
    assert!(tree.entries.iter().all(|entry| entry.ignored));
    let model = fs
        .list_for_agent(
            workspace_id,
            session_id,
            AgentFileListInput { path: path("target").into(), limit: 1, cursor: None },
        )
        .expect("first page for the model");
    assert_eq!(model.entries.len(), 1);
    assert_eq!(model.entries[0].path, path("target/a.txt"));
    let rest = fs
        .list_for_agent(
            workspace_id,
            session_id,
            AgentFileListInput { path: path("target").into(), limit: 1, cursor: model.next_cursor },
        )
        .expect("second page for the model");
    assert_eq!(rest.entries.len(), 1);
    assert_eq!(rest.entries[0].path, path("target/debug"));
}
