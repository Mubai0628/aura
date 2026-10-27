//! A file another program holds open: a log a server started from the Shell keeps writing, for
//! one. fs.read reads it as far as it is written; fs.write refuses, and its refusal keeps the
//! operating system's sharing violation, which Core names to the model as a file in use.
#![cfg(windows)]
use std::{io::Write, os::windows::fs::OpenOptionsExt, sync::Arc, time::Duration};

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    FsExpectedState, FsReadTextRequest, FsWriteFileRequest, SessionSummary, TextReadRange,
    WorkspaceCreateCommand, WorkspaceFileTarget,
};

/// What PowerShell's Start-Process -RedirectStandardOutput leaves others: reading and writing.
const FILE_SHARE_READ_WRITE: u32 = 3;
const ERROR_SHARING_VIOLATION: i32 = 32;

struct Harness {
    workspace: Arc<WorkspaceRuntime>,
    persistence: Arc<PersistenceCoordinator>,
    root: tempfile::TempDir,
    _data: tempfile::TempDir,
    workspace_id: WorkspaceId,
    session_id: SessionId,
}

fn harness() -> Harness {
    let root = tempfile::tempdir().expect("workspace root");
    std::fs::write(root.path().join("server.log"), "line one\n").expect("log");
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
                display_name: "in use".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create workspace");
    workspace.wait_until_indexed(Duration::from_secs(60)).expect("the first walk");
    let workspace_id = created.workspace.workspace_id;
    let session_id = SessionId::new();
    let current = persistence.workspace(workspace_id).expect("current workspace");
    persistence
        .upsert_workspace_session(
            current,
            SessionSummary {
                session_id,
                workspace_id,
                title: "in use".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create session");
    Harness { workspace, persistence, root, _data: data, workspace_id, session_id }
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

    fn read(&self) -> Result<aura_workspace_domain::FsReadTextResult, aura_fs_tools::FsToolError> {
        self.service().read_text(FsReadTextRequest {
            workspace_id: self.workspace_id,
            session_id: self.session_id,
            target: WorkspaceFileTarget { path: WorkspacePath::parse("server.log").expect("path") },
            range: TextReadRange::Full,
            explicit_encoding: None,
        })
    }

    /// The log held open for appending as Start-Process holds a redirected output.
    fn writer(&self) -> std::fs::File {
        std::fs::OpenOptions::new()
            .append(true)
            .share_mode(FILE_SHARE_READ_WRITE)
            .open(self.root.path().join("server.log"))
            .expect("the server's handle")
    }
}

#[test]
fn a_log_another_program_is_writing_is_read_as_far_as_it_is_written() {
    let harness = harness();
    let mut server = harness.writer();
    server.write_all(b"line two\n").expect("the server writes");
    server.flush().expect("flush");
    let read = harness.read().expect("read while the server writes");
    assert_eq!(read.text, "line one\nline two\n");
    drop(server);
}

#[test]
fn a_write_to_a_file_another_program_holds_names_the_sharing_violation() {
    let harness = harness();
    let server = harness.writer();
    let refused = harness
        .service()
        .write_file(
            FsWriteFileRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_id,
                target: WorkspaceFileTarget {
                    path: WorkspacePath::parse("server.log").expect("path"),
                },
                content: "replaced\n".to_owned(),
                expected: FsExpectedState::FileHash {
                    hash: aura_domain_common::ContentHash::blake3(b"line one\n"),
                },
                encoding: None,
                line_ending: None,
            },
            CorrelationId::new(),
        )
        .expect_err("a file another program holds is not replaced");
    // The staged write stops at the file before any step applies; its cause ends with the code
    // Core's refusal names to the model.
    let aura_fs_tools::FsToolError::ChangesetFailed { applied_steps: 0, ref cause, .. } = refused
    else {
        panic!("a refusal before any step applied: {refused:?}")
    };
    assert!(cause.ends_with(&format!("(os error {ERROR_SHARING_VIOLATION})")), "{cause}");
    drop(server);
}
