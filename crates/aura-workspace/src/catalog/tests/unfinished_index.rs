//! A large index is written in batches; one whose batches did not all land (a quit or a crash
//! between them) is not trusted when the workspace opens again: the folder is walked as for a
//! new workspace and the index ends whole.
use aura_domain_common::{CorrelationId, RuntimeInstanceId, WorkspacePath};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace_domain::{CatalogFreshness, WorkspaceCreateCommand};
use std::sync::Arc;
use std::time::Duration;

fn open(store: &std::path::Path) -> (Arc<PersistenceCoordinator>, Arc<crate::WorkspaceRuntime>) {
    let PersistenceStartup::Ready(persistence) =
        PersistenceCoordinator::start(PersistenceOptions::new(
            store.join("store.db"),
            store.join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("open the store")
    else {
        panic!("the fixture store must be ready")
    };
    let persistence = Arc::new(persistence);
    let runtime = crate::WorkspaceRuntime::open(Arc::clone(&persistence), Default::default())
        .expect("open the workspace runtime");
    (persistence, runtime)
}

#[test]
fn an_index_left_unfinished_between_batches_is_walked_again_at_open() {
    let root = tempfile::tempdir().expect("workspace folder");
    // More entries than one batch takes.
    for folder in 0..30 {
        let directory = root.path().join(format!("folder-{folder:03}"));
        std::fs::create_dir(&directory).expect("folder");
        for file in 0..100 {
            std::fs::write(directory.join(format!("file-{file:03}.txt")), "some text\n")
                .expect("file");
        }
    }
    let data = tempfile::tempdir().expect("store folder");
    let last = WorkspacePath::parse("folder-029/file-099.txt").expect("path");
    let workspace = {
        let (persistence, runtime) = open(data.path());
        let created = runtime
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: "batches".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("add the workspace");
        let workspace = created.workspace.workspace_id;
        runtime.wait_until_indexed(Duration::from_secs(120)).expect("the first walk");
        assert_eq!(
            persistence.workspace(workspace).expect("workspace").catalog_freshness,
            CatalogFreshness::Fresh
        );
        assert!(runtime.catalog(workspace).expect("catalog").entry(&last).expect("read").is_some());
        runtime.stop_watchers().expect("stop the watchers");
        workspace
    };
    // A quit between batches leaves the index claimed and marked as still being written.
    rusqlite::Connection::open(data.path().join("store.db"))
        .expect("the store")
        .execute(
            "UPDATE workspaces SET catalog_freshness='rebuilding' WHERE workspace_id=?1",
            [workspace.to_string()],
        )
        .expect("mark the index unfinished");

    let (persistence, runtime) = open(data.path());
    let catalog = runtime.catalog(workspace).expect("catalog");
    assert_eq!(catalog.open_report().entries, 0, "the unfinished index is not used");
    runtime.wait_until_indexed(Duration::from_secs(120)).expect("the walk at open");
    assert!(catalog.entry(&last).expect("read").is_some(), "the walk found every file again");
    assert_eq!(
        persistence.workspace(workspace).expect("workspace").catalog_freshness,
        CatalogFreshness::Fresh
    );
    runtime.stop_watchers().expect("stop the watchers");
}
