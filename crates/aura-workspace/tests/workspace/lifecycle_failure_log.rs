//! A lifecycle operation that hits a failure sends its code, and the error beneath the code, to the
//! log the embedding process routes lifecycle records to.
use crate::runtime_support;

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use aura_domain_common::{
    CorrelationId, DeleteConfirmationId, RendererInstanceId, Revision, SessionId, WorkspaceId,
    WorkspaceLifecycleOperationId, WorkspacePath,
};
use aura_persistence::PersistenceCoordinator;
use aura_workspace::{WorkspaceRuntime, WorkspaceRuntimeOptions, route_lifecycle_log};
use aura_workspace_domain::{
    BeginWorkspaceLifecycleInput, BufferEditCommand, BufferOpenCommand,
    ResolveWorkspaceLifecycleInput, WorkspaceCreateCommand, WorkspaceLifecycleOperation,
    WorkspaceLifecycleResolution, WorkspaceLifecycleState,
};
use runtime_support::{begin_and_ack, persistence};
use rusqlite::Connection;
use serde_json::Value;

static RECORDS: Mutex<Vec<(&'static str, Value)>> = Mutex::new(Vec::new());

fn keep(event: &'static str, fields: Value) {
    RECORDS.lock().unwrap_or_else(PoisonError::into_inner).push((event, fields));
}

/// The records sent for one operation. The tests of this program run side by side, so each reads
/// only its own operation's records.
fn records_of(operation_id: WorkspaceLifecycleOperationId) -> Vec<(&'static str, Value)> {
    let operation_id = operation_id.to_string();
    RECORDS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|(_, fields)| fields["operation_id"] == operation_id.as_str())
        .cloned()
        .collect()
}

/// The one record sent for `operation_id`.
fn record_of(operation_id: WorkspaceLifecycleOperationId) -> Value {
    let records = records_of(operation_id);
    assert_eq!(records.len(), 1, "one record per failure: {records:?}");
    let (event, fields) = records.into_iter().next().expect("one record");
    assert_eq!(event, "workspace.lifecycle_failure");
    fields
}

struct Setup {
    data: tempfile::TempDir,
    root: tempfile::TempDir,
    persistence: Arc<PersistenceCoordinator>,
    runtime: Arc<WorkspaceRuntime>,
    workspace_id: WorkspaceId,
}

impl Setup {
    /// A workspace with one file, indexed, whose lifecycle failures are recorded here.
    fn new(name: &str) -> Self {
        route_lifecycle_log(keep);
        let data = tempfile::tempdir().expect("data root");
        let root = tempfile::tempdir().expect("workspace root");
        std::fs::write(root.path().join("main.rs"), "disk\n").expect("workspace file");
        let persistence = persistence(&data);
        let runtime = WorkspaceRuntime::open(
            Arc::clone(&persistence),
            WorkspaceRuntimeOptions {
                watcher_coalesce: Duration::from_millis(20),
                watcher_poll_interval: Duration::from_secs(1),
                watcher_rescan_attempts: 1,
                ..WorkspaceRuntimeOptions::default()
            },
        )
        .expect("workspace runtime");
        let workspace_id = runtime
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: name.to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create workspace")
            .workspace
            .workspace_id;
        runtime.wait_until_indexed(Duration::from_secs(60)).expect("the first walk");
        Self { data, root, persistence, runtime, workspace_id }
    }

    fn revision(&self) -> Revision {
        self.persistence.workspace(self.workspace_id).expect("workspace").revision
    }

    fn run(
        &self,
        operation: WorkspaceLifecycleOperation,
    ) -> aura_workspace_domain::WorkspaceLifecycleSummary {
        begin_and_ack(
            &self.runtime,
            self.workspace_id,
            self.revision(),
            RendererInstanceId::new(),
            operation,
        )
    }
}

#[test]
fn a_failed_deletion_records_the_store_error_beneath_its_code() {
    let setup = Setup::new("Deletion fails");
    assert_eq!(
        setup.run(WorkspaceLifecycleOperation::Archive).state,
        WorkspaceLifecycleState::Completed
    );
    Connection::open(setup.data.path().join("aura.sqlite3"))
        .expect("database")
        .execute_batch(
            "CREATE TRIGGER refuse_workspace_deletion BEFORE DELETE ON workspaces
             BEGIN SELECT RAISE(ABORT, 'injected workspace deletion failure'); END;",
        )
        .expect("trigger");

    let failed = setup.run(WorkspaceLifecycleOperation::PermanentDelete {
        delete_confirmation_id: DeleteConfirmationId::new(),
    });
    assert_eq!(failed.state, WorkspaceLifecycleState::Failed);
    assert_eq!(failed.error_code.as_deref(), Some("workspace.lifecycle_commit_failed"));

    let fields = record_of(failed.operation_id);
    assert_eq!(fields["workspace_id"], setup.workspace_id.to_string().as_str());
    assert_eq!(fields["operation"], "permanent_delete");
    assert_eq!(fields["state"], "failed");
    assert_eq!(fields["error_code"], "workspace.lifecycle_commit_failed");
    let reason = fields["reason"].as_str().expect("the error beneath the code");
    assert!(reason.contains("injected workspace deletion failure"), "{reason}");
}

#[test]
fn an_operation_whose_window_went_away_records_its_code_alone() {
    let setup = Setup::new("Window gone");
    let renderer_instance_id = RendererInstanceId::new();
    let begin = setup
        .runtime
        .begin_lifecycle(
            BeginWorkspaceLifecycleInput {
                workspace_id: setup.workspace_id,
                expected_workspace_revision: setup.revision(),
                renderer_instance_id,
                operation: WorkspaceLifecycleOperation::Archive,
            },
            CorrelationId::new(),
        )
        .expect("begin lifecycle");

    let ended = setup.runtime.renderer_unavailable(renderer_instance_id).expect("window gone");
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].state, WorkspaceLifecycleState::Failed);

    let fields = record_of(begin.operation_id);
    assert_eq!(fields["operation"], "archive");
    assert_eq!(fields["state"], "failed");
    assert_eq!(fields["error_code"], "workspace.editor_state_unconfirmed");
    assert!(fields.get("reason").is_none(), "no error beneath this code: {fields}");
}

#[test]
fn a_save_that_fails_records_why_the_operation_stays_blocked() {
    let setup = Setup::new("Save fails");
    let session_id = SessionId::new();
    let path = WorkspacePath::parse("main.rs").expect("path");
    let opened = setup
        .runtime
        .open_buffer(BufferOpenCommand {
            workspace_id: setup.workspace_id,
            session_id,
            path: path.clone(),
            writable: true,
        })
        .expect("open writable buffer");
    setup
        .runtime
        .edit_buffer(
            BufferEditCommand {
                workspace_id: setup.workspace_id,
                session_id,
                path,
                reservation_id: opened.reservation_id.expect("reservation"),
                reservation_epoch: opened.reservation_epoch,
                buffer_revision: Revision::new(opened.buffer_revision.value() + 1),
                base_core_revision: opened.buffer_revision,
                content: "unsaved user edit\n".to_owned(),
                active_editing: false,
                ime_composition_active: false,
            },
            CorrelationId::new(),
        )
        .expect("edit buffer");
    // The file changes on disk under the unsaved edit, so saving the edit is refused.
    std::fs::write(setup.root.path().join("main.rs"), "changed elsewhere\n").expect("change file");
    let renderer_instance_id = RendererInstanceId::new();
    let blocked = begin_and_ack(
        &setup.runtime,
        setup.workspace_id,
        setup.revision(),
        renderer_instance_id,
        WorkspaceLifecycleOperation::Archive,
    );
    assert_eq!(blocked.state, WorkspaceLifecycleState::Blocked);

    let still_blocked = setup
        .runtime
        .resolve_lifecycle(ResolveWorkspaceLifecycleInput {
            operation_id: blocked.operation_id,
            expected_lifecycle_revision: blocked.lifecycle_revision,
            renderer_instance_id,
            action: WorkspaceLifecycleResolution::SaveAllAndContinue,
            discard_confirmation_nonce: None,
        })
        .expect("save and continue");
    assert_eq!(still_blocked.state, WorkspaceLifecycleState::Blocked);
    assert_eq!(still_blocked.error_code.as_deref(), Some("workspace.lifecycle_save_failed"));

    let fields = record_of(blocked.operation_id);
    assert_eq!(fields["state"], "blocked");
    assert_eq!(fields["error_code"], "workspace.lifecycle_save_failed");
    let reason = fields["reason"].as_str().expect("why the save failed");
    assert!(reason.contains("main.rs"), "{reason}");
}
