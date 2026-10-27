//! A file the model creates in folders that do not exist yet gets those folders, inside the
//! workspace and in the same changeset; a folder that is a junction is not entered.
use std::{path::Path, sync::Arc, time::Duration};

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolError, FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    FsExpectedState, FsWriteFileRequest, LargeFileReadMode, LineEnding, SessionSummary,
    TextEncoding, WorkspaceCreateCommand, WorkspaceFileTarget,
};

struct Harness {
    fs: Arc<FsToolService>,
    root: tempfile::TempDir,
    _data: tempfile::TempDir,
    workspace_id: WorkspaceId,
    session: SessionId,
}

impl Harness {
    fn new(setup: impl FnOnce(&Path)) -> Self {
        let root = tempfile::tempdir().expect("workspace root");
        setup(root.path());
        let data = tempfile::tempdir().expect("data root");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            data.path().join("aura.sqlite3"),
            data.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence startup")
        {
            PersistenceStartup::Ready(value) => Arc::new(value),
            PersistenceStartup::RecoveryRequired(value) => panic!("{}", value.reason),
        };
        let workspace = WorkspaceRuntime::open(
            Arc::clone(&persistence),
            WorkspaceRuntimeOptions {
                catalog: CatalogOptions::with_index_file_limit(1024 * 1024),
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
                    display_name: "folders".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create workspace");
        workspace.wait_until_indexed(Duration::from_secs(60)).expect("the first walk");
        let workspace_id = created.workspace.workspace_id;
        let session = SessionId::new();
        persistence
            .upsert_workspace_session(
                persistence.workspace(workspace_id).expect("workspace"),
                SessionSummary {
                    session_id: session,
                    workspace_id,
                    title: "folders".to_owned(),
                    state: "idle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("session");
        let fs = FsToolService::new(
            workspace,
            persistence,
            FsToolPolicy {
                large_file_read_mode: LargeFileReadMode::RequireRange,
                full_text_read_limit_bytes: 1024 * 1024,
                small_range_line_limit: 500,
                small_range_character_limit: 65_536,
                default_binary_range_bytes: 4 * 1024,
                max_binary_range_bytes: 64 * 1024,
                max_changeset_operations: 64,
                max_search_results: 100,
                mutation_wait: Duration::from_secs(5),
            },
        )
        .expect("fs service");
        Self { fs, root, _data: data, workspace_id, session }
    }

    fn create(&self, path: &str, text: &str) -> Result<(), FsToolError> {
        self.fs
            .write_file_for_agent(
                FsWriteFileRequest {
                    workspace_id: self.workspace_id,
                    session_id: self.session,
                    target: WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
                    content: text.to_owned(),
                    expected: FsExpectedState::Absent,
                    // As Core creates a file: UTF-8 with LF.
                    encoding: Some(TextEncoding::Utf8),
                    line_ending: Some(LineEnding::Lf),
                },
                CorrelationId::new(),
            )
            .map(|_| ())
    }
}

#[test]
fn a_new_file_in_missing_folders_gets_them() {
    let harness = Harness::new(|root| std::fs::create_dir(root.join("crates")).expect("crates"));
    harness.create("crates/new-domain/src/lib.rs", "pub mod tools;\n").expect("written");
    let written = harness.root.path().join("crates/new-domain/src/lib.rs");
    assert_eq!(std::fs::read_to_string(written).expect("the file"), "pub mod tools;\n");
    harness.create("top/level.txt", "one\n").expect("a folder at the root");
    assert!(harness.root.path().join("top/level.txt").is_file());
}

#[test]
fn a_file_that_exists_is_still_refused_as_present() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("docs")).expect("docs");
        std::fs::write(root.join("docs/plan.md"), "plan\n").expect("plan");
    });
    assert!(harness.create("docs/plan.md", "other\n").is_err(), "expected absent, found a file");
    assert_eq!(
        std::fs::read_to_string(harness.root.path().join("docs/plan.md")).expect("plan"),
        "plan\n"
    );
}

#[test]
fn a_junction_on_the_way_is_not_entered() {
    let outside = tempfile::tempdir().expect("outside");
    let target = outside.path().to_path_buf();
    let harness = Harness::new(move |root| {
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(root.join("linked"))
            .arg(&target)
            .status()
            .expect("mklink");
        assert!(status.success(), "a junction for the test");
    });
    assert!(harness.create("linked/new/file.txt", "x\n").is_err(), "the junction is refused");
    assert!(!outside.path().join("new").exists(), "nothing was created outside the workspace");
}
