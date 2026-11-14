//! A Workspace move committed through the Workspace's own lifecycle owner, as the move route
//! commits it, without the security follow-up the route runs afterwards: the state a Core that
//! stopped right after the commit leaves behind. The folder is either moved on its drive or
//! copied, and the Workspace pointed at the copy.
use crate::RuntimeServices;
use aura_domain_common::{CorrelationId, RendererInstanceId, WorkspaceId};
use aura_workspace_domain::{
    BeginWorkspaceLifecycleInput, EditorLifecycleTransitionAck, WorkspaceLifecycleOperation,
    WorkspaceLifecyclePrepareDeliveredInput, WorkspaceLifecycleState, WorkspaceLifecycleSummary,
};
use std::path::{Path, PathBuf};

/// Commits a lifecycle operation through the Workspace's own owner, as the route does, without
/// the route's security follow-up afterwards.
pub(crate) fn commit_without_follow_up(
    services: &RuntimeServices,
    workspace_id: WorkspaceId,
    operation: WorkspaceLifecycleOperation,
) {
    let done = run_without_follow_up(services, workspace_id, operation);
    assert_eq!(done.state, WorkspaceLifecycleState::Completed, "{done:?}");
}

/// Runs a lifecycle operation through the Workspace's own owner, as the route does, and returns
/// where it ended, without the route's security follow-up afterwards.
pub(crate) fn run_without_follow_up(
    services: &RuntimeServices,
    workspace_id: WorkspaceId,
    operation: WorkspaceLifecycleOperation,
) -> WorkspaceLifecycleSummary {
    let runtime = &services.p03_required().workspace;
    let renderer_instance_id = RendererInstanceId::new();
    let current = services.persistence.workspace(workspace_id).expect("read the Workspace");
    let begin = runtime
        .begin_lifecycle(
            BeginWorkspaceLifecycleInput {
                workspace_id,
                expected_workspace_revision: current.revision,
                renderer_instance_id,
                operation,
            },
            CorrelationId::new(),
        )
        .expect("begin the lifecycle operation");
    runtime.prepare_lifecycle_transition(begin.operation_id).expect("prepare the editors");
    runtime
        .mark_lifecycle_prepare_delivered(WorkspaceLifecyclePrepareDeliveredInput {
            operation_id: begin.operation_id,
            expected_lifecycle_revision: begin.lifecycle_revision,
            renderer_instance_id,
        })
        .expect("deliver the prepare");
    runtime
        .acknowledge_lifecycle(EditorLifecycleTransitionAck {
            operation_id: begin.operation_id,
            lifecycle_revision: begin.lifecycle_revision,
            renderer_instance_id,
            workspace_id,
            buffers: runtime
                .buffers()
                .current_lifecycle_states(workspace_id)
                .expect("read the editors"),
        })
        .expect("acknowledge the lifecycle operation")
}

/// Moves the Workspace's folder `root` on its drive and commits the move with no withdrawal
/// after it. Returns where the folder is now.
pub(crate) fn move_without_withdrawal(
    services: &RuntimeServices,
    workspace_id: WorkspaceId,
    root: &Path,
) -> PathBuf {
    services
        .p03_required()
        .workspace
        .stop_watchers()
        .expect("stop watching the folder before it moves");
    let moved = root.with_file_name("moved-agent-workspace");
    rename(root, &moved);
    commit_without_follow_up(
        services,
        workspace_id,
        WorkspaceLifecycleOperation::Relocate {
            selected_target_root: moved.display().to_string(),
            confirmed_same_logical_project: false,
        },
    );
    moved
}

/// Copies the Workspace's folder `root` beside it and points the Workspace at the copy, as a user
/// does who says the copy is the same project, with no withdrawal after it. The folder at `root`
/// stays, with every file an approval given there names. Returns the copy.
pub(crate) fn point_at_copy_without_withdrawal(
    services: &RuntimeServices,
    workspace_id: WorkspaceId,
    root: &Path,
) -> PathBuf {
    let copy = root.with_file_name("copied-agent-workspace");
    copy_folder(root, &copy);
    commit_without_follow_up(
        services,
        workspace_id,
        WorkspaceLifecycleOperation::Relocate {
            selected_target_root: copy.display().to_string(),
            confirmed_same_logical_project: true,
        },
    );
    copy
}

fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir(to).expect("create the copy");
    for entry in std::fs::read_dir(from).expect("list the Workspace folder") {
        let entry = entry.expect("read the Workspace folder");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("read what the entry is").is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy the file");
        }
    }
}

/// A folder Core held a moment ago can stay busy for a short while.
fn rename(from: &Path, to: &Path) {
    let mut attempts = 0;
    while let Err(error) = std::fs::rename(from, to) {
        attempts += 1;
        assert!(attempts < 40, "move the Workspace folder: {error}");
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}
