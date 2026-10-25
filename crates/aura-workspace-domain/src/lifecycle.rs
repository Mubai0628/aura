use aura_domain_common::SecretNonce;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use aura_domain_common::{
    ContentHash, DeleteConfirmationId, EditorReservationId, RendererInstanceId, Revision,
    SessionId, UtcTimestamp, WorkspaceId, WorkspaceLifecycleOperationId, WorkspacePath,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceLifecycleState {
    Preparing,
    Blocked,
    Saving,
    Discarding,
    Committing,
    Completed,
    Cancelled,
    Failed,
}

impl WorkspaceLifecycleState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceLifecycleOperationKind {
    Archive,
    Relocate,
    PermanentDelete,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "operation_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceLifecycleOperation {
    Archive,
    Relocate { selected_target_root: String, confirmed_same_logical_project: bool },
    PermanentDelete { delete_confirmation_id: DeleteConfirmationId },
}

impl WorkspaceLifecycleOperation {
    #[must_use]
    pub const fn kind(&self) -> WorkspaceLifecycleOperationKind {
        match self {
            Self::Archive => WorkspaceLifecycleOperationKind::Archive,
            Self::Relocate { .. } => WorkspaceLifecycleOperationKind::Relocate,
            Self::PermanentDelete { .. } => WorkspaceLifecycleOperationKind::PermanentDelete,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BeginWorkspaceLifecycleInput {
    pub workspace_id: WorkspaceId,
    pub expected_workspace_revision: Revision,
    pub renderer_instance_id: RendererInstanceId,
    pub operation: WorkspaceLifecycleOperation,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BeginWorkspaceLifecycleResult {
    pub operation_id: WorkspaceLifecycleOperationId,
    pub lifecycle_revision: Revision,
    pub state: WorkspaceLifecycleState,
    pub ack_deadline_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareEditorLifecycleTransition {
    pub operation_id: WorkspaceLifecycleOperationId,
    pub lifecycle_revision: Revision,
    pub workspace_id: WorkspaceId,
    pub operation: WorkspaceLifecycleOperationKind,
    pub deadline_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceLifecycleOperationQuery {
    pub operation_id: WorkspaceLifecycleOperationId,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceLifecyclePrepareDeliveredInput {
    pub operation_id: WorkspaceLifecycleOperationId,
    pub expected_lifecycle_revision: Revision,
    pub renderer_instance_id: RendererInstanceId,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RendererLifecycleUnavailableInput {
    pub renderer_instance_id: RendererInstanceId,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "save_state", deny_unknown_fields)]
pub enum EditorSaveState {
    Idle,
    Saving,
    Failed { error_code: String },
    Conflict,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditorBufferLifecycleState {
    pub session_id: SessionId,
    pub canonical_relative_path: WorkspacePath,
    pub reservation_id: Option<EditorReservationId>,
    pub reservation_epoch: Revision,
    pub buffer_revision: Revision,
    pub core_acknowledged_revision: Revision,
    pub logical_hash: ContentHash,
    pub disk_hash: Option<ContentHash>,
    pub is_dirty: bool,
    pub is_active_editing: bool,
    pub ime_composition_active: bool,
    pub save_state: EditorSaveState,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditorLifecycleTransitionAck {
    pub operation_id: WorkspaceLifecycleOperationId,
    pub lifecycle_revision: Revision,
    pub renderer_instance_id: RendererInstanceId,
    pub workspace_id: WorkspaceId,
    pub buffers: Vec<EditorBufferLifecycleState>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditorStateRef {
    pub session_id: SessionId,
    pub canonical_relative_path: WorkspacePath,
    pub reservation_id: Option<EditorReservationId>,
    pub reservation_epoch: Revision,
    pub buffer_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "blocker_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceLifecycleBlocker {
    DirtyBuffer {
        session_id: SessionId,
        path: WorkspacePath,
        buffer_revision: Revision,
    },
    ActiveEditing {
        session_id: SessionId,
        path: WorkspacePath,
        ime_composition_active: bool,
    },
    PendingBufferSynchronization {
        session_id: SessionId,
        path: WorkspacePath,
        renderer_revision: Revision,
        core_revision: Revision,
    },
    SaveInProgress {
        session_id: SessionId,
        path: WorkspacePath,
    },
    SaveFailed {
        session_id: SessionId,
        path: WorkspacePath,
        error_code: String,
    },
    FileVersionConflict {
        session_id: SessionId,
        path: WorkspacePath,
    },
    EditorStateUnconfirmed {
        session_id: Option<SessionId>,
        path: Option<WorkspacePath>,
    },
    DeferredMutationExists {
        session_id: SessionId,
        path: WorkspacePath,
    },
    RuntimeObjectsActive {
        count: u64,
    },
    RuntimeLeasesActive {
        count: u64,
    },
    MutationOwnershipActive {
        count: u64,
    },
    UnfinishedJournals {
        count: u64,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceLifecyclePreflight {
    pub workspace_id: WorkspaceId,
    pub operation_id: WorkspaceLifecycleOperationId,
    pub operation: WorkspaceLifecycleOperationKind,
    pub lifecycle_revision: Revision,
    pub blockers: Vec<WorkspaceLifecycleBlocker>,
    pub releasable_editor_states: Vec<EditorStateRef>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceLifecycleResolution {
    SaveAllAndContinue,
    ReturnToEditing,
    DiscardDirtyBuffersAndContinue,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveWorkspaceLifecycleInput {
    pub operation_id: WorkspaceLifecycleOperationId,
    pub expected_lifecycle_revision: Revision,
    pub renderer_instance_id: RendererInstanceId,
    pub action: WorkspaceLifecycleResolution,
    pub discard_confirmation_nonce: Option<SecretNonce>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssueDiscardConfirmationInput {
    pub operation_id: WorkspaceLifecycleOperationId,
    pub expected_lifecycle_revision: Revision,
    pub renderer_instance_id: RendererInstanceId,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiscardBufferBinding {
    pub session_id: SessionId,
    pub canonical_relative_path: WorkspacePath,
    pub reservation_id: EditorReservationId,
    pub reservation_epoch: Revision,
    pub buffer_revision: Revision,
    pub logical_hash: ContentHash,
    pub disk_hash: Option<ContentHash>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiscardConfirmation {
    pub confirmation_nonce: SecretNonce,
    pub operation_id: WorkspaceLifecycleOperationId,
    pub lifecycle_revision: Revision,
    pub workspace_id: WorkspaceId,
    pub affected_buffers: Vec<DiscardBufferBinding>,
    pub expires_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceLifecycleSummary {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approval_follow_up: Vec<crate::EditorApprovalFollowUp>,
    pub operation_id: WorkspaceLifecycleOperationId,
    pub workspace_id: WorkspaceId,
    pub renderer_instance_id: RendererInstanceId,
    pub operation: WorkspaceLifecycleOperationKind,
    pub lifecycle_revision: Revision,
    pub state: WorkspaceLifecycleState,
    pub ack_deadline_at: Option<UtcTimestamp>,
    pub preflight: Option<WorkspaceLifecyclePreflight>,
    pub error_code: Option<String>,
}
