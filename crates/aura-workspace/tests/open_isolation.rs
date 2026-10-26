//! A workspace whose stored index cannot be opened is left out of the runtime's open; every other
//! workspace opens and is projected, and the failed one is reported on its own.
mod runtime_support;

use std::sync::Arc;
use std::time::Duration;

use aura_domain_common::{CorrelationId, WorkspaceId};
use aura_workspace::{WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::WorkspaceCreateCommand;
use runtime_support::persistence;

fn add(runtime: &Arc<WorkspaceRuntime>, root: &std::path::Path, name: &str) -> WorkspaceId {
    std::fs::write(root.join("a.txt"), "a").expect("file");
    runtime
        .create(
            WorkspaceCreateCommand {
                display_name: name.to_owned(),
                selected_root: root.display().to_string(),
            },
            CorrelationId::new(),
        )
        .expect("workspace")
        .workspace
        .workspace_id
}

#[test]
fn a_workspace_whose_index_cannot_be_opened_leaves_the_others_open() {
    let data = tempfile::tempdir().expect("data root");
    let healthy_root = tempfile::tempdir().expect("healthy root");
    let broken_root = tempfile::tempdir().expect("broken root");
    let persistence = persistence(&data);
    let runtime =
        WorkspaceRuntime::open(Arc::clone(&persistence), WorkspaceRuntimeOptions::default())
            .expect("first runtime");
    let healthy = add(&runtime, healthy_root.path(), "Healthy");
    let broken = add(&runtime, broken_root.path(), "Broken");
    runtime.wait_until_indexed(Duration::from_secs(60)).expect("indexed");
    drop(runtime);

    // A stored entry the index can no longer read back.
    let database = rusqlite::Connection::open(data.path().join("aura.sqlite3")).expect("database");
    let changed = database
        .execute(
            "UPDATE workspace_catalog_entries SET relative_path='../outside.txt'
             WHERE workspace_id=?1 AND relative_path='a.txt'",
            [broken.to_string()],
        )
        .expect("damage");
    assert_eq!(changed, 1);
    drop(database);

    let runtime =
        WorkspaceRuntime::open(Arc::clone(&persistence), WorkspaceRuntimeOptions::default())
            .expect("the runtime opens without the damaged workspace");
    assert!(runtime.catalog(healthy).is_ok(), "the healthy workspace is open");
    assert!(runtime.catalog(broken).is_err(), "the damaged workspace is left out");
    assert_eq!(runtime.open_report().expect("report").online_workspaces, 1);
    let projection = runtime.workspace_files_projection().expect("projection");
    assert_eq!(projection.workspaces.len(), 2, "both are projected, the damaged one on its own");
}
