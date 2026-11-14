use super::*;
use WorkspaceLifecycleOperationKind::{Archive, PermanentDelete, Relocate};
use WorkspaceLifecycleState::{Blocked, Cancelled, Completed, Failed};
use aura_domain_common::{RendererInstanceId, Revision, WorkspaceLifecycleOperationId};
use aura_workspace_domain::{CatalogFreshness, WorkspaceCreateDisposition, WorkspaceState};

fn lifecycle(
    operation: WorkspaceLifecycleOperationKind,
    state: WorkspaceLifecycleState,
    workspace: WorkspaceId,
) -> Value {
    serde_json::to_value(WorkspaceLifecycleSummary {
        approval_follow_up: Vec::new(),
        operation_id: WorkspaceLifecycleOperationId::new(),
        workspace_id: workspace,
        renderer_instance_id: RendererInstanceId::new(),
        operation,
        lifecycle_revision: Revision::new(3),
        state,
        ack_deadline_at: None,
        preflight: None,
        error_code: None,
    })
    .expect("a lifecycle summary serializes")
}

fn summary(workspace: WorkspaceId) -> WorkspaceSummary {
    WorkspaceSummary {
        workspace_id: workspace,
        display_name: "Project".into(),
        selected_root: r"D:\Dev\Project".into(),
        canonical_root: r"D:\Dev\Project".into(),
        root_identity: None,
        state: WorkspaceState::Online,
        revision: Revision::new(2),
        catalog_freshness: CatalogFreshness::Fresh,
        authorization_epoch: Revision::new(2),
    }
}

#[test]
fn only_a_committed_move_withdraws_file_approvals() {
    let workspace = WorkspaceId::new();
    let moved = lifecycle(Relocate, Completed, workspace);
    for method_name in [method::WORKSPACE_LIFECYCLE_ACK, method::WORKSPACE_LIFECYCLE_RESOLVE] {
        assert_eq!(
            WorkspaceFollowUp::of(method_name, &moved),
            Some(WorkspaceFollowUp::Relocated(workspace)),
            "{method_name}"
        );
    }
    // Stopped for open edits, failed or withdrawn: the Workspace is still at its folder.
    for state in [Blocked, Failed, Cancelled] {
        let value = lifecycle(Relocate, state, workspace);
        assert_eq!(WorkspaceFollowUp::of(method::WORKSPACE_LIFECYCLE_ACK, &value), None);
        assert_eq!(WorkspaceFollowUp::of(method::WORKSPACE_LIFECYCLE_RESOLVE, &value), None);
    }
    // Archiving and deleting complete on the same answers and move nothing.
    for operation in [Archive, PermanentDelete] {
        let value = lifecycle(operation, Completed, workspace);
        assert_eq!(WorkspaceFollowUp::of(method::WORKSPACE_LIFECYCLE_ACK, &value), None);
    }
    // The relocate request itself only begins the move.
    assert_eq!(WorkspaceFollowUp::of(method::WORKSPACE_RELOCATE, &moved), None);
    assert_eq!(WorkspaceFollowUp::of(method::WORKSPACE_LIFECYCLE_PREPARE, &moved), None);
    assert_eq!(
        WorkspaceFollowUp::of(method::WORKSPACE_LIFECYCLE_ACK, &serde_json::json!({})),
        None
    );
}

#[test]
fn a_restored_or_created_workspace_checks_its_suspended_grants_again() {
    let workspace = WorkspaceId::new();
    let restored = serde_json::to_value(summary(workspace)).expect("summary serializes");
    assert_eq!(
        WorkspaceFollowUp::of(method::WORKSPACE_RESTORE, &restored),
        Some(WorkspaceFollowUp::Restored(workspace))
    );
    let created = serde_json::to_value(WorkspaceCreateResult {
        disposition: WorkspaceCreateDisposition::RestoredArchived,
        workspace: summary(workspace),
    })
    .expect("create result serializes");
    assert_eq!(
        WorkspaceFollowUp::of(method::WORKSPACE_CREATE, &created),
        Some(WorkspaceFollowUp::Restored(workspace))
    );
    assert_eq!(WorkspaceFollowUp::of(method::WORKSPACE_RESTORE, &created), None);
}
