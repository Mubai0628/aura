use aura_domain_common::ToolBatchId;
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{
    AgentRunId, ArtifactId, ContentHash, ContextProjectionSnapshotId, EndpointId, EventId,
    ModelExecutionSnapshotId, ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId,
    SessionId, SessionSequence, TurnTaskId, UtcTimestamp,
};
use aura_model_domain::{
    CompiledInstructionEnvelope, LogicalConversationItem, LogicalInstructionBlock, ModelApiFormat,
    ModelAttachmentSnapshot, ModelOutputRequest, ProviderContinuationState,
};
use aura_security_contract::InvocationIdentity;
use aura_tool_contract::ToolSchemaSetSnapshotV1;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ContextOverflowDiagnosticView, ContextProjectionSnapshot, ModelEgressManifestV1};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderContinuationBindingV1 {
    pub state: ProviderContinuationState,
    pub provider_id: ProviderId,
    pub api_format: ModelApiFormat,
    pub endpoint_id: EndpointId,
    pub model_profile_id: ModelProfileId,
    pub endpoint_fingerprint: ContentHash,
    pub previous_model_execution_id: ModelExecutionSnapshotId,
    pub provider_response_ref: ArtifactId,
    pub request_evidence_hash: CanonicalSha256HashV1,
    pub response_evidence_hash: CanonicalSha256HashV1,
    pub assistant_response_event_id: EventId,
    pub tool_batch_id: ToolBatchId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
// This wire-authoritative trigger deliberately keeps the frozen public field
// types unboxed; it is persisted and transferred, not used as a hot-loop value.
#[allow(clippy::large_enum_variant)]
pub enum ContextPreparationTriggerV1 {
    UserSubmission {
        user_event_id: EventId,
    },
    /// One task can consume multiple original user events without forging a
    /// merged message or losing their source identities.
    UserBatch {
        user_event_ids: Vec<EventId>,
    },
    /// Core progression after a durable assistant response. This does not
    /// represent a new user message or an invented Tool batch.
    CoreContinuation {
        originating_user_event_id: EventId,
        previous_model_step_id: ModelStepId,
        assistant_response_event_id: EventId,
        checkpoint_hash: CanonicalSha256HashV1,
    },
    ToolContinuation {
        originating_user_event_id: EventId,
        previous_model_step_id: ModelStepId,
        assistant_response_event_id: EventId,
        tool_batch_id: ToolBatchId,
        history_atomic_group_ref: crate::HistoryAtomicGroupRefV1,
        provider_continuation: Option<ProviderContinuationBindingV1>,
    },
}

impl ContextPreparationTriggerV1 {
    #[must_use]
    pub const fn provider_continuation(&self) -> Option<&ProviderContinuationBindingV1> {
        match self {
            Self::UserSubmission { .. }
            | Self::UserBatch { .. }
            | Self::CoreContinuation { .. } => None,
            Self::ToolContinuation { provider_continuation, .. } => provider_continuation.as_ref(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPreparationState {
    CollectingSources,
    ResolvingPromptBundle,
    ResolvingAgentsInstructions,
    BuildingSections,
    EstimatingBudget,
    CompactionRequired,
    Compacting,
    RebuildingAfterCompaction,
    Ready,
    AwaitingContextRemediation,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct ContextPreparationRequest {
    pub invocation_identity: InvocationIdentity,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub model_step_id: ModelStepId,
    pub model_profile_id: ModelProfileId,
    pub runtime_effective_window_tokens: u64,
    pub output_request: ModelOutputRequest,
    pub history_cutoff: SessionSequence,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedLogicalContext {
    pub model_step_id: ModelStepId,
    pub context_snapshot_id: ContextProjectionSnapshotId,
    pub prompt_bundle_snapshot_id: PromptBundleSnapshotId,
    pub instruction_blocks: Vec<LogicalInstructionBlock>,
    pub instruction_envelope: CompiledInstructionEnvelope,
    pub conversation_items: Vec<LogicalConversationItem>,
    pub tool_schema_set: ToolSchemaSetSnapshotV1,
    pub attachments: Vec<ModelAttachmentSnapshot>,
    pub provider_continuation: Option<ProviderContinuationState>,
    pub output_request: ModelOutputRequest,
    pub model_egress_manifest: ModelEgressManifestV1,
    pub snapshot: ContextProjectionSnapshot,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRemediationPlan {
    pub overflow: ContextOverflowDiagnosticView,
    pub actions: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextRemediationState {
    Awaiting,
    Cleared,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextRemediationClearReason {
    RecheckSucceeded,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRemediationRecord {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub source_context_snapshot_id: ContextProjectionSnapshotId,
    pub plan: ContextRemediationPlan,
    pub state: ContextRemediationState,
    pub required_at: UtcTimestamp,
    pub cleared_at: Option<UtcTimestamp>,
    pub clear_reason: Option<ContextRemediationClearReason>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPreparationFailure {
    pub code: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextPreparationOutcome {
    Prepared { context: Box<PreparedLogicalContext> },
    AwaitingRemediation { plan: ContextRemediationPlan },
    Interrupted { failure: ContextPreparationFailure },
}
