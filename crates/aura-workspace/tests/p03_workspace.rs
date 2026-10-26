mod runtime_support;

use std::{sync::Arc, time::Duration};

use aura_domain_common::{
    CorrelationId, DeferredMutationId, MutationJournalId, RendererInstanceId, Revision,
    SecretNonce, SessionId, UtcTimestamp, WorkspacePath,
};
use aura_persistence::{DeferredMutationRecord, PersistenceError};
use aura_workspace::{CatalogOptions, WorkspaceError, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    BeginWorkspaceLifecycleInput, BufferEditCommand, BufferOpenCommand,
    IssueDiscardConfirmationInput, ResolveWorkspaceLifecycleInput, SessionSummary,
    WorkspaceCreateCommand, WorkspaceCreateDisposition, WorkspaceLifecycleOperation,
    WorkspaceLifecyclePrepareDeliveredInput, WorkspaceLifecycleResolution, WorkspaceLifecycleState,
    WorkspaceState,
};
use runtime_support::{begin_and_ack, persistence};

#[test]
fn relocate_requires_identity_or_confirmation_and_invalidates_old_path_state() {
    let original = tempfile::tempdir().expect("original root");
    let replacement = tempfile::tempdir().expect("replacement root");
    let alias_parent = tempfile::tempdir().expect("alias parent");
    let data = tempfile::tempdir().expect("data root");
    std::fs::write(original.path().join("old.txt"), "old").expect("old file");
    std::fs::write(replacement.path().join("new.txt"), "new").expect("new file");
    let persistence = persistence(&data);
    let runtime = WorkspaceRuntime::open(
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
    let created = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: original.path().display().to_string(),
                display_name: "Original".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create workspace");
    assert_eq!(created.disposition, WorkspaceCreateDisposition::Created);
    let workspace_id = created.workspace.workspace_id;
    // The first walk runs after the create returns and records the index on the workspace.
    runtime.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
    assert_eq!(
        created.workspace.selected_root,
        persistence.workspace(workspace_id).expect("created workspace").selected_root
    );
    let session_id = SessionId::new();
    persistence
        .upsert_workspace_session(
            persistence.workspace(workspace_id).expect("current workspace"),
            SessionSummary {
                session_id,
                workspace_id,
                title: "Relocate".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("session");
    runtime
        .open_buffer(BufferOpenCommand {
            workspace_id,
            session_id,
            path: WorkspacePath::parse("old.txt").expect("old path"),
            writable: true,
        })
        .expect("open old buffer");
    let file_reference_id = persistence
        .create_file_reference(
            session_id,
            workspace_id,
            &WorkspacePath::parse("old.txt").expect("old path"),
        )
        .expect("file reference");
    let before = persistence.workspace(workspace_id).expect("before relocate");
    let renderer_instance_id = RendererInstanceId::new();

    assert!(matches!(
        runtime.begin_lifecycle(
            BeginWorkspaceLifecycleInput {
                workspace_id,
                expected_workspace_revision: before.revision,
                renderer_instance_id,
                operation: WorkspaceLifecycleOperation::Relocate {
                    selected_target_root: replacement.path().display().to_string(),
                    confirmed_same_logical_project: false,
                },
            },
            CorrelationId::new(),
        ),
        Err(WorkspaceError::RelocateConfirmationRequired)
    ));
    persistence.acquire_mutation_ownership(workspace_id, session_id).expect("mutation ownership");
    let blocked = begin_and_ack(
        &runtime,
        workspace_id,
        before.revision,
        renderer_instance_id,
        WorkspaceLifecycleOperation::Relocate {
            selected_target_root: replacement.path().display().to_string(),
            confirmed_same_logical_project: true,
        },
    );
    assert_eq!(blocked.state, WorkspaceLifecycleState::Blocked);
    assert!(blocked.preflight.as_ref().is_some_and(|preflight| {
        preflight.blockers.iter().any(|blocker| {
            matches!(
                blocker,
                aura_workspace_domain::WorkspaceLifecycleBlocker::MutationOwnershipActive {
                    count: 1
                }
            )
        })
    }));
    let cancelled = runtime
        .resolve_lifecycle(ResolveWorkspaceLifecycleInput {
            operation_id: blocked.operation_id,
            expected_lifecycle_revision: blocked.lifecycle_revision,
            renderer_instance_id,
            action: WorkspaceLifecycleResolution::ReturnToEditing,
            discard_confirmation_nonce: None,
        })
        .expect("return to editing");
    assert_eq!(cancelled.state, WorkspaceLifecycleState::Cancelled);
    assert!(
        persistence
            .release_mutation_ownership(workspace_id, session_id)
            .expect("release ownership")
    );

    let completed = begin_and_ack(
        &runtime,
        workspace_id,
        before.revision,
        renderer_instance_id,
        WorkspaceLifecycleOperation::Relocate {
            selected_target_root: replacement.path().display().to_string(),
            confirmed_same_logical_project: true,
        },
    );
    assert_eq!(completed.state, WorkspaceLifecycleState::Completed);
    runtime
        .wait_until_indexed(std::time::Duration::from_secs(60))
        .expect("the relocated workspace's first walk");
    let relocated = persistence.workspace(workspace_id).expect("relocated workspace");
    assert_eq!(relocated.workspace_id, workspace_id);
    assert!(relocated.authorization_epoch.value() > before.authorization_epoch.value());
    assert!(runtime.buffers().statuses(workspace_id).expect("buffers").is_empty());
    assert!(
        persistence
            .resolve_file_reference(session_id, file_reference_id)
            .expect("resolve old reference")
            .is_none()
    );
    assert!(
        runtime
            .catalog(workspace_id)
            .expect("catalog")
            .entry(&WorkspacePath::parse("new.txt").expect("new path"))
            .expect("entry")
            .is_some()
    );
    assert!(
        runtime
            .catalog(workspace_id)
            .expect("catalog")
            .entry(&WorkspacePath::parse("old.txt").expect("old path"))
            .expect("entry")
            .is_none()
    );

    let duplicate = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: replacement.path().display().to_string(),
                display_name: "Duplicate".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("same root lookup");
    assert_eq!(duplicate.disposition, WorkspaceCreateDisposition::Existing);
    assert_eq!(duplicate.workspace.workspace_id, workspace_id);

    let alias = alias_parent.path().join("replacement-link");
    junction::create(replacement.path(), &alias).expect("replacement junction");
    let same_object_completed = begin_and_ack(
        &runtime,
        workspace_id,
        relocated.revision,
        renderer_instance_id,
        WorkspaceLifecycleOperation::Relocate {
            selected_target_root: alias.display().to_string(),
            confirmed_same_logical_project: false,
        },
    );
    assert_eq!(same_object_completed.state, WorkspaceLifecycleState::Completed);
    let same_object = persistence.workspace(workspace_id).expect("same object relocate");
    assert_eq!(same_object.workspace_id, workspace_id);
    assert_eq!(same_object.selected_root, alias.display().to_string());
}

#[test]
fn archive_requires_every_persistent_workspace_blocker_to_clear() {
    let root = tempfile::tempdir().expect("workspace root");
    let data = tempfile::tempdir().expect("data root");
    std::fs::write(root.path().join("main.rs"), "fn main() {}\n").expect("workspace file");
    let persistence = persistence(&data);
    let runtime = WorkspaceRuntime::open(
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
    let workspace = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: root.path().display().to_string(),
                display_name: "Archive blockers".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create workspace")
        .workspace;
    let workspace_id = workspace.workspace_id;
    // The first walk runs after the create returns and records the index on the workspace.
    runtime.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
    let workspace = persistence.workspace(workspace_id).expect("current workspace");
    let session_id = SessionId::new();
    persistence
        .upsert_workspace_session(
            workspace,
            SessionSummary {
                session_id,
                workspace_id,
                title: "Archive blockers".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("session");

    persistence
        .register_workspace_runtime_object(workspace_id, "managed_process", "process-1")
        .expect("runtime object");
    let lease_id = persistence
        .acquire_runtime_lease("workspace", workspace_id.to_string())
        .expect("runtime lease");
    persistence.acquire_mutation_ownership(workspace_id, session_id).expect("mutation ownership");
    let deferred_path = WorkspacePath::parse("main.rs").expect("deferred path");
    persistence
        .record_deferred_mutation(DeferredMutationRecord {
            deferred_mutation_id: DeferredMutationId::new(),
            workspace_id,
            session_id,
            path: deferred_path.clone(),
            expected_hash: None,
            reason: "active editor".to_owned(),
            created_at: UtcTimestamp::now(),
        })
        .expect("deferred mutation");
    let journal_id = MutationJournalId::new();
    persistence
        .begin_workspace_journal(
            journal_id,
            workspace_id,
            session_id,
            "fs.apply_changeset",
            serde_json::json!({ "applied_steps": 0 }),
        )
        .expect("workspace journal");

    let blockers = persistence.workspace_blockers(workspace_id).expect("blockers");
    assert_eq!(blockers.running_objects, 1);
    assert_eq!(blockers.runtime_leases, 1);
    assert_eq!(blockers.mutation_ownership, 1);
    assert_eq!(blockers.deferred_mutations, 1);
    assert_eq!(blockers.unfinished_journals, 1);
    let renderer_instance_id = RendererInstanceId::new();
    let blocked = begin_and_ack(
        &runtime,
        workspace_id,
        persistence.workspace(workspace_id).expect("current workspace").revision,
        renderer_instance_id,
        WorkspaceLifecycleOperation::Archive,
    );
    assert_eq!(blocked.state, WorkspaceLifecycleState::Blocked);
    assert_eq!(blocked.preflight.as_ref().expect("aggregated preflight").blockers.len(), 5);
    assert!(matches!(
        persistence.register_workspace_runtime_object(
            workspace_id,
            "managed_process",
            "process-after-barrier",
        ),
        Err(PersistenceError::WorkspaceBusy)
    ));
    assert!(matches!(
        persistence.acquire_runtime_lease("workspace", workspace_id.to_string()),
        Err(PersistenceError::WorkspaceBusy)
    ));
    assert!(matches!(
        persistence.record_deferred_mutation(DeferredMutationRecord {
            deferred_mutation_id: DeferredMutationId::new(),
            workspace_id,
            session_id,
            path: deferred_path.clone(),
            expected_hash: None,
            reason: "after barrier".to_owned(),
            created_at: UtcTimestamp::now(),
        }),
        Err(PersistenceError::WorkspaceBusy)
    ));
    assert!(matches!(
        persistence.begin_workspace_journal(
            MutationJournalId::new(),
            workspace_id,
            session_id,
            "fs.write_file",
            serde_json::json!({ "applied_steps": 0 }),
        ),
        Err(PersistenceError::WorkspaceBusy)
    ));

    assert!(
        persistence
            .finish_workspace_runtime_object(workspace_id, "managed_process", "process-1", false,)
            .expect("finish runtime object")
    );
    assert!(persistence.release_runtime_lease(&lease_id).expect("release lease"));
    assert!(
        persistence
            .release_mutation_ownership(workspace_id, session_id)
            .expect("release ownership")
    );
    assert_eq!(
        persistence
            .resolve_deferred_mutations_for_path(
                workspace_id,
                &deferred_path,
                CorrelationId::new(),
            )
            .expect("resolve deferred mutation"),
        1
    );
    persistence
        .update_workspace_journal(
            journal_id,
            "completed",
            serde_json::json!({ "applied_steps": 0 }),
        )
        .expect("complete journal");
    assert!(persistence.workspace_blockers(workspace_id).expect("clear blockers").is_clear());

    let completed = runtime
        .resolve_lifecycle(ResolveWorkspaceLifecycleInput {
            operation_id: blocked.operation_id,
            expected_lifecycle_revision: blocked.lifecycle_revision,
            renderer_instance_id,
            action: WorkspaceLifecycleResolution::SaveAllAndContinue,
            discard_confirmation_nonce: None,
        })
        .expect("continue after blockers clear");
    assert_eq!(completed.state, WorkspaceLifecycleState::Completed);
    let archived = persistence.workspace(workspace_id).expect("archived workspace");
    assert_eq!(archived.state, WorkspaceState::Archived);
}

#[test]
fn prepare_timeout_fails_safely_and_preserves_clean_editor_state() {
    let root = tempfile::tempdir().expect("workspace root");
    let data = tempfile::tempdir().expect("data root");
    std::fs::write(root.path().join("main.rs"), "fn main() {}\n").expect("workspace file");
    let persistence = persistence(&data);
    let runtime = WorkspaceRuntime::open(
        Arc::clone(&persistence),
        WorkspaceRuntimeOptions {
            lifecycle_prepare_ack_timeout: Duration::from_millis(25),
            watcher_coalesce: Duration::from_millis(20),
            watcher_poll_interval: Duration::from_secs(1),
            watcher_rescan_attempts: 1,
            ..WorkspaceRuntimeOptions::default()
        },
    )
    .expect("workspace runtime");
    let workspace = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: root.path().display().to_string(),
                display_name: "Prepare timeout".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create workspace")
        .workspace;
    let workspace_id = workspace.workspace_id;
    // The first walk runs after the create returns and records the index on the workspace.
    runtime.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
    let session_id = SessionId::new();
    let opened = runtime
        .open_buffer(BufferOpenCommand {
            workspace_id,
            session_id,
            path: WorkspacePath::parse("main.rs").expect("path"),
            writable: true,
        })
        .expect("open writable buffer");
    let renderer_instance_id = RendererInstanceId::new();
    let current_workspace = persistence.workspace(workspace_id).expect("current workspace");
    let begin = runtime
        .begin_lifecycle(
            BeginWorkspaceLifecycleInput {
                workspace_id,
                expected_workspace_revision: current_workspace.revision,
                renderer_instance_id,
                operation: WorkspaceLifecycleOperation::Archive,
            },
            CorrelationId::new(),
        )
        .expect("begin lifecycle");
    runtime.prepare_lifecycle_transition(begin.operation_id).expect("prepare transition");
    runtime
        .mark_lifecycle_prepare_delivered(WorkspaceLifecyclePrepareDeliveredInput {
            operation_id: begin.operation_id,
            expected_lifecycle_revision: begin.lifecycle_revision,
            renderer_instance_id,
        })
        .expect("prepare delivered");

    std::thread::sleep(Duration::from_millis(100));

    let failed =
        persistence.workspace_lifecycle_operation(begin.operation_id).expect("failed operation");
    assert_eq!(failed.summary.state, WorkspaceLifecycleState::Failed);
    assert_eq!(failed.summary.error_code.as_deref(), Some("workspace.editor_prepare_ack_timeout"));
    let reopened = runtime
        .open_buffer(BufferOpenCommand {
            workspace_id,
            session_id,
            path: WorkspacePath::parse("main.rs").expect("path"),
            writable: true,
        })
        .expect("timeout must unfreeze editor");
    assert_eq!(reopened.reservation_id, opened.reservation_id);
    assert_eq!(reopened.reservation_epoch, opened.reservation_epoch);
    assert_eq!(
        persistence.workspace(workspace_id).expect("workspace").state,
        WorkspaceState::Online
    );
}

#[test]
fn discard_nonce_is_exact_and_completed_lifecycle_rejects_late_updates() {
    let root = tempfile::tempdir().expect("workspace root");
    let data = tempfile::tempdir().expect("data root");
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
    let workspace = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: root.path().display().to_string(),
                display_name: "Discard nonce".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create workspace")
        .workspace;
    let workspace_id = workspace.workspace_id;
    // The first walk runs after the create returns and records the index on the workspace.
    runtime.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
    let session_id = SessionId::new();
    let path = WorkspacePath::parse("main.rs").expect("path");
    let opened = runtime
        .open_buffer(BufferOpenCommand {
            workspace_id,
            session_id,
            path: path.clone(),
            writable: true,
        })
        .expect("open writable buffer");
    let reservation_id = opened.reservation_id.expect("reservation");
    let edited = runtime
        .edit_buffer(
            BufferEditCommand {
                workspace_id,
                session_id,
                path: path.clone(),
                reservation_id,
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
    let renderer_instance_id = RendererInstanceId::new();
    let current_workspace = persistence.workspace(workspace_id).expect("current workspace");
    let blocked = begin_and_ack(
        &runtime,
        workspace_id,
        current_workspace.revision,
        renderer_instance_id,
        WorkspaceLifecycleOperation::Archive,
    );
    assert_eq!(blocked.state, WorkspaceLifecycleState::Blocked);
    let confirmation = runtime
        .issue_discard_confirmation(IssueDiscardConfirmationInput {
            operation_id: blocked.operation_id,
            expected_lifecycle_revision: blocked.lifecycle_revision,
            renderer_instance_id,
        })
        .expect("discard confirmation");
    assert_eq!(format!("{:?}", confirmation.confirmation_nonce), "SecretNonce([REDACTED])");

    let invalid = runtime.resolve_lifecycle(ResolveWorkspaceLifecycleInput {
        operation_id: blocked.operation_id,
        expected_lifecycle_revision: confirmation.lifecycle_revision,
        renderer_instance_id,
        action: WorkspaceLifecycleResolution::DiscardDirtyBuffersAndContinue,
        discard_confirmation_nonce: Some(SecretNonce::from_bytes([0xA5; 32])),
    });
    assert!(matches!(invalid, Err(WorkspaceError::DiscardConfirmationInvalid)));

    let completed = runtime
        .resolve_lifecycle(ResolveWorkspaceLifecycleInput {
            operation_id: blocked.operation_id,
            expected_lifecycle_revision: confirmation.lifecycle_revision,
            renderer_instance_id,
            action: WorkspaceLifecycleResolution::DiscardDirtyBuffersAndContinue,
            discard_confirmation_nonce: Some(confirmation.confirmation_nonce.clone()),
        })
        .expect("discard and continue");
    assert_eq!(completed.state, WorkspaceLifecycleState::Completed);
    assert_eq!(std::fs::read(root.path().join("main.rs")).expect("disk unchanged"), b"disk\n");

    let late = runtime.edit_buffer(
        BufferEditCommand {
            workspace_id,
            session_id,
            path,
            reservation_id,
            reservation_epoch: opened.reservation_epoch,
            buffer_revision: Revision::new(edited.status.buffer_revision.value() + 1),
            base_core_revision: edited.status.buffer_revision,
            content: "late renderer update\n".to_owned(),
            active_editing: false,
            ime_composition_active: false,
        },
        CorrelationId::new(),
    );
    assert!(matches!(
        late,
        Err(WorkspaceError::ReservationRevoked {
            reservation_id: revoked,
            current_epoch,
        }) if revoked == reservation_id && current_epoch.value() > opened.reservation_epoch.value()
    ));
}

#[test]
fn renderer_disconnect_cancels_blocked_operation_and_preserves_editor_state() {
    let root = tempfile::tempdir().expect("workspace root");
    let data = tempfile::tempdir().expect("data root");
    std::fs::write(root.path().join("main.rs"), "fn main() {}\n").expect("workspace file");
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
    let workspace = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: root.path().display().to_string(),
                display_name: "Renderer disconnect".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create workspace")
        .workspace;
    let workspace_id = workspace.workspace_id;
    // The first walk runs after the create returns and records the index on the workspace.
    runtime.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
    let session_id = SessionId::new();
    let path = WorkspacePath::parse("main.rs").expect("path");
    let opened = runtime
        .open_buffer(BufferOpenCommand {
            workspace_id,
            session_id,
            path: path.clone(),
            writable: true,
        })
        .expect("open writable buffer");
    runtime
        .buffers()
        .set_active_editing(workspace_id, session_id, &path, true)
        .expect("activate editor");
    let current_workspace = persistence.workspace(workspace_id).expect("current workspace");
    let renderer_instance_id = RendererInstanceId::new();
    let blocked = begin_and_ack(
        &runtime,
        workspace_id,
        current_workspace.revision,
        renderer_instance_id,
        WorkspaceLifecycleOperation::Archive,
    );
    assert_eq!(blocked.state, WorkspaceLifecycleState::Blocked);

    let cancelled =
        runtime.renderer_unavailable(renderer_instance_id).expect("renderer unavailable");
    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled[0].state, WorkspaceLifecycleState::Cancelled);
    assert_eq!(cancelled[0].error_code.as_deref(), Some("workspace.renderer_unavailable"));
    let status = runtime
        .buffers()
        .statuses(workspace_id)
        .expect("preserved buffers")
        .into_iter()
        .next()
        .expect("buffer");
    assert!(status.active_editing);
    assert_eq!(status.reservation_id, opened.reservation_id);
    runtime
        .buffers()
        .set_active_editing(workspace_id, session_id, &path, false)
        .expect("operation cancellation unfreezes editor");
}
