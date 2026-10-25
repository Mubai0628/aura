use crate::AgentError;
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskMode {
    Normal,
    Plan,
    Goal,
}
/// What a turn is for. Only `/init` differs from an ordinary turn: it is a one-off
/// task that runs as Normal and leaves the session's Plan or Goal mode untouched.
// So does `/compact` while nothing runs:
// Core compacts the conversation before the turn's first step, and the model only confirms it.
// (Plain comments here, so the contract stays the enum it was, one value longer.)
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskPurpose {
    #[default]
    Conversation,
    WorkspaceInit,
    Compaction,
}
impl AgentTaskPurpose {
    pub fn is_conversation(&self) -> bool {
        *self == Self::Conversation
    }
    /// A one-off command task: Normal, idle-only, never bound to the session's mode episode.
    pub fn is_command(&self) -> bool {
        matches!(self, Self::WorkspaceInit | Self::Compaction)
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum TurnTaskState {
    Pending,
    Running,
    AwaitingApproval,
    AwaitingQuery { query_id: UserQueryId },
    Completed { final_report: String },
    Failed { reason: String },
    Cancelled,
    RecoveryRequired { reason: String },
}
impl TurnTaskState {
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. }
                | Self::Failed { .. }
                | Self::Cancelled
                | Self::RecoveryRequired { .. }
        )
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TurnTask {
    pub turn_task_id: TurnTaskId,
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    pub root_user_event_id: EventId,
    pub lineage_id: TaskLineageId,
    pub lineage_resolution_required: bool,
    pub agent_run_id: AgentRunId,
    pub mode: AgentTaskMode,
    pub state: TurnTaskState,
    pub revision: Revision,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentRunState {
    PreparingContext,
    AwaitingContextRemediation { model_step_id: ModelStepId },
    ModelRunning { model_step_id: ModelStepId },
    ToolsRunning { tool_batch_id: ToolBatchId },
    AwaitingApproval { tool_batch_id: ToolBatchId },
    AwaitingQuery { query_id: UserQueryId },
    Completed,
    Cancelled,
    RecoveryRequired { episode: u32 },
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentRun {
    pub agent_run_id: AgentRunId,
    pub turn_task_id: TurnTaskId,
    pub parent_agent_run_id: Option<AgentRunId>,
    pub state: AgentRunState,
    pub revision: Revision,
    pub next_model_ordinal: u32,
    pub prompt_bundle_snapshot_id: PromptBundleSnapshotId,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "trigger", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentContinuation {
    Initial { user_event_id: EventId },
    ToolBatch { tool_batch_id: ToolBatchId, history_group_hash: jcs::CanonicalHashV1 },
    CompletionReminder { previous_model_step_id: ModelStepId },
    QueryAnswered { query_id: UserQueryId, user_event_id: EventId },
    Recovered { recovery_episode: u32 },
    Compacted { context_snapshot_id: ContextProjectionSnapshotId },
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelStepState {
    Preparing,
    Running {
        execution_snapshot_id: ModelExecutionSnapshotId,
    },
    Completed {
        assistant_event_id: EventId,
        output_artifact_id: ArtifactId,
        tool_batch_id: Option<ToolBatchId>,
    },
    Failed {
        error_artifact_id: ArtifactId,
    },
    Cancelled,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelStep {
    pub model_step_id: ModelStepId,
    pub agent_run_id: AgentRunId,
    pub ordinal: u32,
    pub continuation: AgentContinuation,
    pub state: ModelStepState,
    pub created_at: UtcTimestamp,
    pub finished_at: Option<UtcTimestamp>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryEpisode {
    pub agent_run_id: AgentRunId,
    pub ordinal: u32,
    pub reason: String,
    pub unresolved_operation_ids: Vec<SecurityOperationId>,
    pub not_executed_tool_call_ids: Vec<ToolCallId>,
    pub evidence_refs: Vec<ArtifactId>,
    pub created_at: UtcTimestamp,
    pub resolved_at: Option<UtcTimestamp>,
}
impl TurnTask {
    pub fn transition(
        &mut self,
        expected: Revision,
        state: TurnTaskState,
        at: UtcTimestamp,
    ) -> Result<(), AgentError> {
        if self.revision != expected {
            return Err(AgentError::RevisionConflict);
        }
        // Resumption creates an explicitly related TurnTask; terminal identities
        // are never reset to replay their old Tool batches.
        if self.state.terminal() || at < self.updated_at || matches!(state, TurnTaskState::Pending)
        {
            return Err(AgentError::InvalidTransition);
        }
        if let TurnTaskState::Completed { final_report } = &state
            && final_report.trim().is_empty()
        {
            return Err(AgentError::InvalidInput);
        }
        self.revision =
            Revision::new(expected.value().checked_add(1).ok_or(AgentError::RevisionConflict)?);
        self.state = state;
        self.updated_at = at;
        Ok(())
    }
}
