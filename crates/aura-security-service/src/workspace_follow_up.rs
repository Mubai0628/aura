//! What the security service does once a Workspace command has committed: a restored or new
//! Workspace checks its suspended grants again, and a moved one withdraws its file approvals.
use aura_domain_common::{CorrelationId, WorkspaceId};
use aura_ipc::method;
use aura_workspace_domain::{
    WorkspaceCreateResult, WorkspaceLifecycleOperationKind, WorkspaceLifecycleState,
    WorkspaceLifecycleSummary, WorkspaceSummary,
};
use serde_json::Value;

use crate::P04Services;

#[cfg(test)]
mod tests;

/// A committed Workspace change the security service acts on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceFollowUp {
    Restored(WorkspaceId),
    Relocated(WorkspaceId),
}

impl WorkspaceFollowUp {
    /// The follow-up a Workspace command's successful answer calls for. A move completes on the
    /// lifecycle answer that commits it, either at once or after the user settled open edits.
    #[must_use]
    pub fn of(method_name: &str, value: &Value) -> Option<Self> {
        fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Option<T> {
            serde_json::from_value(value.clone()).ok()
        }
        match method_name {
            method::WORKSPACE_RESTORE => parse(value)
                .map(|workspace: WorkspaceSummary| Self::Restored(workspace.workspace_id)),
            method::WORKSPACE_CREATE => parse(value)
                .map(|result: WorkspaceCreateResult| Self::Restored(result.workspace.workspace_id)),
            method::WORKSPACE_LIFECYCLE_ACK | method::WORKSPACE_LIFECYCLE_RESOLVE => parse(value)
                .filter(|summary: &WorkspaceLifecycleSummary| {
                    summary.operation == WorkspaceLifecycleOperationKind::Relocate
                        && summary.state == WorkspaceLifecycleState::Completed
                })
                .map(|summary| Self::Relocated(summary.workspace_id)),
            _ => None,
        }
    }
}

impl P04Services {
    pub fn follow_workspace(&self, follow_up: WorkspaceFollowUp, correlation_id: CorrelationId) {
        match follow_up {
            WorkspaceFollowUp::Restored(workspace) => {
                self.on_workspace_restored(workspace, correlation_id);
            }
            WorkspaceFollowUp::Relocated(workspace) => {
                self.on_workspace_relocated(workspace, correlation_id);
            }
        }
    }
}
