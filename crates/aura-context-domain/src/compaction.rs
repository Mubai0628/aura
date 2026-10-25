use aura_domain_common::{
    AgentRunId, ArtifactId, CommandInvocationId, ContentHash, ContextProjectionSnapshotId, EventId,
    ModelExecutionSnapshotId, ModelStepId, SessionId, SessionSequence, TransportAttemptId,
    UtcTimestamp,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ContextSourceRef;

pub const MANUAL_COMPACTION_FOCUS_MAX_CHARS: usize = 4_096;
pub const LARGE_TOOL_RESULT_THRESHOLD_CHARS: usize = 8_192;
pub const LARGE_TOOL_RESULT_PREFIX_CHARS: usize = 2_048;
pub const LARGE_TOOL_RESULT_SUFFIX_CHARS: usize = 1_024;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentCompactionRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub focus: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentCompactionStatus {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub request: Option<ManualCompactionRequestRecord>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualCompactionRequestState {
    Requested,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManualCompactionRequestRecord {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub command_invocation_id: CommandInvocationId,
    pub command_event_id: EventId,
    pub focus_artifact_id: Option<ArtifactId>,
    pub focus_hash: Option<ContentHash>,
    pub state: ManualCompactionRequestState,
    pub source_context_snapshot_id: Option<ContextProjectionSnapshotId>,
    pub compaction_sequence: Option<u32>,
    pub error_code: Option<String>,
    pub created_at: UtcTimestamp,
    pub terminal_at: Option<UtcTimestamp>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionModelAttemptState {
    Running,
    Completed,
    Failed,
    Cancelled,
    UnknownSideEffect,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionModelAttemptRecord {
    pub source_context_snapshot_id: ContextProjectionSnapshotId,
    pub compaction_sequence: u32,
    pub attempt_ordinal: u8,
    pub attempt_context_snapshot_id: ContextProjectionSnapshotId,
    pub model_step_id: ModelStepId,
    pub model_execution_snapshot_id: ModelExecutionSnapshotId,
    pub transport_attempt_id: Option<TransportAttemptId>,
    pub state: CompactionModelAttemptState,
    pub error_code: Option<String>,
    pub started_at: UtcTimestamp,
    pub terminal_at: Option<UtcTimestamp>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextRetentionException {
    OversizedAtomicSegment,
    InsufficientEligibleRecentHistory,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionAnchorKind {
    Event,
    Artifact,
    ToolCall,
    WorkspaceFile,
    TaskLineage,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionAnchor {
    pub anchor: String,
    pub kind: CompactionAnchorKind,
    pub source_ref: ContextSourceRef,
    pub source_hash: ContentHash,
    pub preview: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionAnchorManifest {
    pub anchors: Vec<CompactionAnchor>,
    pub manifest_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionSourceSpan {
    pub start_sequence: SessionSequence,
    pub end_sequence: SessionSequence,
    pub source_refs: Vec<ContextSourceRef>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StructuredCompactionRecord {
    pub source_context_snapshot_id: ContextProjectionSnapshotId,
    pub sequence: u32,
    pub source_span: CompactionSourceSpan,
    pub historical_summary_hash: ContentHash,
    pub active_turn_summary_hash: ContentHash,
    pub structured_markdown_hash: ContentHash,
    pub anchor_manifest_hash: ContentHash,
    pub retained_recent_tokens: u64,
    pub retention_exceptions: Vec<ContextRetentionException>,
}
