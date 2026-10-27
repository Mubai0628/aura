//! A workspace whose index did not enter every folder (a huge tree, or memory low while it was
//! walked) still answers lookups in full: fs.find and fs.search read those folders from disk,
//! and only what their budget or the page's room did not reach is named as skipped.
use std::{sync::Arc, time::Duration};

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{
    CatalogBuildPhase, CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions,
};
use aura_workspace_domain::{
    AgentFileFindInput, AgentFileFindMatchMode, FsSearchCompletion, FsSearchPattern,
    FsSearchTextRequest, SessionSummary, WorkspaceCreateCommand,
};

struct Harness {
    workspace: Arc<WorkspaceRuntime>,
    persistence: Arc<PersistenceCoordinator>,
    _root: tempfile::TempDir,
    _data: tempfile::TempDir,
    workspace_id: WorkspaceId,
    session_id: SessionId,
}

/// Four folders of five files, one more file deeper in the third, and an index that was capped
/// before it entered any of the folders.
fn harness() -> Harness {
    let root = tempfile::tempdir().expect("workspace root");
    for folder in 0..4 {
        let directory = root.path().join(format!("folder-{folder}"));
        std::fs::create_dir(&directory).expect("folder");
        for file in 0..5 {
            std::fs::write(directory.join(format!("file-{file}.txt")), "plain\n").expect("file");
        }
    }
    let deep = root.path().join("folder-2").join("deep");
    std::fs::create_dir(&deep).expect("deep folder");
    std::fs::write(deep.join("needle.txt"), "a needle in the folder\n").expect("needle");
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
    let workspace = WorkspaceRuntime::open(
        Arc::clone(&persistence),
        WorkspaceRuntimeOptions {
            catalog: CatalogOptions::with_index_file_limit(64 * 1024 * 1024),
            watcher_coalesce: Duration::from_millis(20),
            watcher_poll_interval: Duration::from_secs(1),
            watcher_rescan_attempts: 1,
            ..WorkspaceRuntimeOptions::default()
        },
    )
    .expect("workspace runtime");
    let created = workspace
        .create(
            WorkspaceCreateCommand {
                selected_root: root.path().display().to_string(),
                display_name: "unindexed".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create workspace");
    let workspace_id = created.workspace.workspace_id;
    workspace.wait_until_indexed(Duration::from_secs(60)).expect("the first walk");
    let catalog = workspace.catalog(workspace_id).expect("catalog");
    catalog.cap_walks_for_test(4);
    assert!(workspace.rebuild_index(workspace_id).expect("asked"), "a capped walk starts");
    workspace.wait_until_indexed(Duration::from_secs(60)).expect("the capped walk");
    let status = catalog.build_status().expect("status");
    assert_eq!(status.phase, CatalogBuildPhase::Incomplete, "{status:?}");
    assert_eq!(status.unindexed_directories, 4, "{status:?}");
    let session_id = SessionId::new();
    let current = persistence.workspace(workspace_id).expect("current workspace");
    persistence
        .upsert_workspace_session(
            current,
            SessionSummary {
                session_id,
                workspace_id,
                title: "unindexed".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create session");
    Harness { workspace, persistence, _root: root, _data: data, workspace_id, session_id }
}

impl Harness {
    fn service(&self) -> Arc<FsToolService> {
        FsToolService::new(
            Arc::clone(&self.workspace),
            Arc::clone(&self.persistence),
            FsToolPolicy::default(),
        )
        .expect("fs service")
    }

    fn find(&self, name: &str, limit: u32) -> aura_workspace_domain::AgentFileFindResult {
        self.service()
            .find_for_agent(
                self.workspace_id,
                self.session_id,
                AgentFileFindInput {
                    path: WorkspacePath::parse(".").expect("root").into(),
                    file_names: vec![name.to_owned()],
                    recursive: true,
                    match_mode: AgentFileFindMatchMode::Exact,
                    case_sensitive: false,
                    limit,
                    cursor: None,
                },
            )
            .expect("find")
    }
}

#[test]
fn a_name_in_a_folder_the_index_did_not_enter_is_found() {
    let harness = harness();
    let found = harness.find("needle.txt", 20);
    assert_eq!(
        found.matches.iter().map(|entry| entry.path.as_str()).collect::<Vec<_>>(),
        vec!["folder-2/deep/needle.txt"]
    );
    assert!(found.complete, "{found:?}");
    assert!(found.skipped_scopes.is_empty(), "{:?}", found.skipped_scopes);

    // More matches than the page holds: the page is full, in path order, and says it is not
    // complete, naming where the rest is.
    let crowded = harness.find("file-0.txt", 2);
    assert_eq!(
        crowded.matches.iter().map(|entry| entry.path.as_str()).collect::<Vec<_>>(),
        vec!["folder-0/file-0.txt", "folder-1/file-0.txt"]
    );
    assert!(!crowded.complete);
    assert_eq!(crowded.skipped_scopes.len(), 4, "{:?}", crowded.skipped_scopes);
}

#[test]
fn text_in_a_folder_the_index_did_not_enter_is_searched() {
    let harness = harness();
    let result = harness
        .service()
        .search_text(FsSearchTextRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_id,
            pattern: FsSearchPattern::Literal { text: "needle".to_owned(), case_sensitive: true },
            root: WorkspacePath::parse(".").expect("root"),
            path_glob: None,
            include_ignored: false,
            include_generated: false,
            limit: 20,
        })
        .expect("search");
    assert_eq!(
        result.matches.iter().map(|found| found.path.as_str()).collect::<Vec<_>>(),
        vec!["folder-2/deep/needle.txt"]
    );
    assert_eq!(result.completion, FsSearchCompletion::Complete);
    assert!(result.skipped_scopes.is_empty(), "{:?}", result.skipped_scopes);
}
