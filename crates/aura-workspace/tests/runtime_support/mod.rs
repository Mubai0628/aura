//! A registry and a workspace lifecycle driven the way the desktop drives it.
// Each test file uses the part it needs.
#![allow(dead_code)]
use std::sync::Arc;

use aura_domain_common::{
    CorrelationId, RendererInstanceId, Revision, RuntimeInstanceId, WorkspaceId,
};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::WorkspaceRuntime;
use aura_workspace_domain::{
    BeginWorkspaceLifecycleInput, EditorLifecycleTransitionAck, WorkspaceLifecycleOperation,
    WorkspaceLifecyclePrepareDeliveredInput, WorkspaceLifecycleSummary,
};

pub fn persistence(data: &tempfile::TempDir) -> Arc<PersistenceCoordinator> {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        data.path().join("aura.sqlite3"),
        data.path().join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("persistence startup")
    {
        PersistenceStartup::Ready(value) => Arc::new(value),
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    }
}

pub fn begin_and_ack(
    runtime: &Arc<WorkspaceRuntime>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: Revision,
    renderer_instance_id: RendererInstanceId,
    operation: WorkspaceLifecycleOperation,
) -> WorkspaceLifecycleSummary {
    let begin = runtime
        .begin_lifecycle(
            BeginWorkspaceLifecycleInput {
                workspace_id,
                expected_workspace_revision,
                renderer_instance_id,
                operation,
            },
            CorrelationId::new(),
        )
        .expect("begin lifecycle");
    let prepare =
        runtime.prepare_lifecycle_transition(begin.operation_id).expect("prepare transition");
    assert_eq!(prepare.lifecycle_revision, begin.lifecycle_revision);
    runtime
        .mark_lifecycle_prepare_delivered(WorkspaceLifecyclePrepareDeliveredInput {
            operation_id: begin.operation_id,
            expected_lifecycle_revision: begin.lifecycle_revision,
            renderer_instance_id,
        })
        .expect("prepare delivered");
    runtime
        .acknowledge_lifecycle(EditorLifecycleTransitionAck {
            operation_id: begin.operation_id,
            lifecycle_revision: begin.lifecycle_revision,
            renderer_instance_id,
            workspace_id,
            buffers: runtime
                .buffers()
                .current_lifecycle_states(workspace_id)
                .expect("current editor states"),
        })
        .expect("ack lifecycle")
}
