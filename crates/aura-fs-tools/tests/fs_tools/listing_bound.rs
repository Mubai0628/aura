//! A folder listing for the file tree comes a page at a time: each page holds at most what the tree
//! shows at once and says where the next one starts, so every entry of a huge folder can be
//! reached, also in a folder the index does not hold, and no page costs the whole folder.
use std::sync::Arc;

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspacePath};
use aura_fs_tools::{FsToolService, LISTED};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::WorkspaceRuntime;
use aura_workspace_domain::{
    FsListRequest, SessionSummary, WorkspaceCreateCommand, WorkspaceFileTarget,
};

#[test]
fn a_huge_folder_is_listed_page_by_page_and_every_entry_is_reached() {
    let root = tempfile::tempdir().expect("workspace root");
    std::fs::write(root.path().join(".gitignore"), "big/\n").expect(".gitignore");
    let big = root.path().join("big");
    std::fs::create_dir_all(&big).expect("folder");
    for index in 0..=10_000 {
        std::fs::write(big.join(format!("{index:05}.txt")), "").expect("file");
    }
    std::fs::write(root.path().join("small.txt"), "").expect("file");
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
                display_name: "listing bound".to_owned(),
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
    let fs =
        FsToolService::new(Arc::clone(&workspace), Arc::clone(&persistence), Default::default())
            .expect("fs service");
    let list = |path: &str, after: Option<WorkspacePath>| {
        fs.list(FsListRequest {
            after,
            ..FsListRequest::first_page(
                workspace_id,
                session_id,
                WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
            )
        })
    };
    let mut seen = Vec::new();
    let mut after = None;
    loop {
        let page = list("big", after).expect("a page of the ignored folder");
        assert!(page.entries.len() <= LISTED);
        seen.extend(page.entries.into_iter().map(|entry| entry.name));
        match page.next_after {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(seen.len(), 10_001, "every file once");
    assert!(seen.windows(2).all(|pair| pair[0] < pair[1]), "in order, no repeats");
    assert!(seen.iter().any(|name| name == "10000.txt"), "the last file is reachable");
    let root = list(".", None).expect("the root");
    assert!(root.next_after.is_none());
    assert!(root.entries.iter().any(|entry| entry.name == "small.txt"));
    let wire = serde_json::to_value(&root).expect("wire");
    assert!(wire.get("next_after").is_none(), "{wire}");
    let elsewhere = list("big", Some(WorkspacePath::parse("small.txt").expect("path")));
    assert!(elsewhere.is_err(), "a cursor names a child of the listed folder");
}
