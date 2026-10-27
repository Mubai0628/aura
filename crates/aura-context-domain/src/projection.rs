use aura_domain_common::ToolCallId;
use aura_domain_common::ToolId;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    AgentRunId, ContentHash, ContextProjectionSnapshotId, ModelProfileId, ModelStepId,
    PromptBundleSnapshotId, Revision, RuntimeInstanceId, SessionId, SkillInstanceId, TaskLineageId,
    UtcTimestamp, WorkspaceId,
};
use aura_security_contract::{ModelChannelDataDisposition, NetworkDataClass};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ContextCompressibility, ContextPrivacyClass, ContextRetentionException, ContextSectionKind,
};
/// The coverage state the activity Inspector view reports. It is re-used from the
/// activity contract rather than restated here, so the two cannot drift apart.
pub use aura_activity_domain::ActivityCoverageState;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextUsageKind {
    SystemAndRules,
    ToolsAndResults,
    Conversation,
    BackgroundAndCompaction,
}

impl ContextUsageKind {
    pub const ORDER: [Self; 4] = [
        Self::SystemAndRules,
        Self::ToolsAndResults,
        Self::Conversation,
        Self::BackgroundAndCompaction,
    ];
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextUsageSegmentView {
    pub kind: ContextUsageKind,
    pub label: String,
    pub estimated_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextUsageSummaryView {
    pub runtime_effective_window_tokens: u64,
    pub used_input_tokens: u64,
    pub estimated: bool,
    pub segments: Vec<ContextUsageSegmentView>,
}

impl ContextUsageSummaryView {
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.segments.len() == ContextUsageKind::ORDER.len()
            && self
                .segments
                .iter()
                .zip(ContextUsageKind::ORDER)
                .all(|(segment, kind)| segment.kind == kind)
            && self
                .segments
                .iter()
                .try_fold(0_u64, |total, segment| total.checked_add(segment.estimated_tokens))
                == Some(self.used_input_tokens)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorBudgetView {
    pub runtime_effective_window_tokens: u64,
    pub estimated_input_tokens: u64,
    pub output_reserve_tokens: u64,
    pub safety_reserve_tokens: u64,
    pub request_input_budget_tokens: u64,
    pub estimator_id: String,
    pub estimator_version: u32,
    pub exact: bool,
    pub policy: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptInspectorSectionPlacement {
    InstructionEnvelope,
    Conversation,
    CurrentUserPart,
    Attachment,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorSectionView {
    pub kind: ContextSectionKind,
    pub physical_ordinal: u8,
    pub content_hash: ContentHash,
    pub estimated_tokens: u64,
    pub character_count: u64,
    pub byte_count: u64,
    pub changed_from_previous_step: bool,
    pub source_kinds: Vec<String>,
    pub compressibility: ContextCompressibility,
    pub privacy_class: ContextPrivacyClass,
    pub is_empty_marker: bool,
    pub placement: PromptInspectorSectionPlacement,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorToolView {
    pub tool_id: ToolId,
    pub canonical_name: String,
    pub schema_hash: CanonicalHashV1,
    pub estimated_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorSkillView {
    pub skill_instance_id: SkillInstanceId,
    pub content_hash: ContentHash,
    pub estimated_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorAttachmentView {
    pub display_name: String,
    pub media_type: String,
    pub byte_length: u64,
    pub content_hash: ContentHash,
    pub estimated_tokens: Option<u64>,
    pub resolved: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorCacheUsageView {
    #[serde(default)]
    pub actual_input_tokens: Option<u64>,
    pub actual_cached_input_tokens: Option<u64>,
    pub stable_prefix_estimate_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorTaskLineageView {
    pub lineage_id: TaskLineageId,
    pub revision: Revision,
    pub state: String,
    pub source_count: u32,
    pub completion_policy: String,
    pub completion_gate: String,
    pub source_recheck_required: bool,
    pub preview_characters: u64,
    pub preview_estimated_tokens: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCompactionStateView {
    NotRequired,
    Required,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorCompactionView {
    pub state: ContextCompactionStateView,
    pub source_context_snapshot_id: Option<ContextProjectionSnapshotId>,
    pub retained_recent_tokens: u64,
    pub retention_exceptions: Vec<ContextRetentionException>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorEgressClassView {
    pub data_class: NetworkDataClass,
    pub disposition: ModelChannelDataDisposition,
    pub item_count: u32,
    pub byte_length: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorModelEgressView {
    pub policy_version: u32,
    pub manifest_hash: ContentHash,
    pub classes: Vec<PromptInspectorEgressClassView>,
    pub unresolved_source_kinds: Vec<String>,
    pub attachments_resolved: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOverflowDiagnosticView {
    pub request_input_budget_tokens: u64,
    pub estimated_input_tokens: u64,
    pub non_compressible_tokens: u64,
    pub compressible_history_tokens: u64,
    pub user_reducible_tokens: u64,
    pub overflow_tokens: u64,
    pub error_code: String,
    pub remediation: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorProjection {
    pub session_id: SessionId,
    pub revision: Revision,
    pub active_agent_run_id: Option<AgentRunId>,
    pub active_model_step_id: Option<ModelStepId>,
    pub prompt_bundle_snapshot_id: Option<PromptBundleSnapshotId>,
    pub context_snapshot_id: Option<ContextProjectionSnapshotId>,
    pub model_profile_id: Option<ModelProfileId>,
    pub budget: Option<PromptInspectorBudgetView>,
    pub sections: Vec<PromptInspectorSectionView>,
    pub tools: Vec<PromptInspectorToolView>,
    pub skills: Vec<PromptInspectorSkillView>,
    pub attachments: Vec<PromptInspectorAttachmentView>,
    pub context_usage: Option<ContextUsageSummaryView>,
    pub cache_usage: PromptInspectorCacheUsageView,
    pub active_task_lineage: Option<PromptInspectorTaskLineageView>,
    pub compaction: PromptInspectorCompactionView,
    pub model_egress: Option<PromptInspectorModelEgressView>,
    pub overflow: Option<ContextOverflowDiagnosticView>,
    pub active_tool_call_ids: Vec<ToolCallId>,
    /// The frozen run-information activity projection for this step.
    ///
    /// It is reported from the **same** frozen snapshot the activity fragment was built
    /// from, so the Inspector can never describe a picture different from the one that was
    /// actually sent. It is `None` when this step had no activity fragment at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<PromptInspectorActivityView>,
    /// Set on the step whose preparation cleared older tool results or
    /// compacted history before its request went out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduction: Option<PromptInspectorReductionView>,
}

/// What one
/// step's preparation gave back - the request's estimated input when it first did not fit (or
/// when the user asked for a compaction), how many older tool results were cleared, and whether
/// history was compacted. The size after is the step's own context meter.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorReductionView {
    pub input_tokens_before: u64,
    pub cleared_tool_results: u32,
    pub compacted: bool,
}

/// The Inspector's real projection of the frozen activity snapshot.
///
/// Every field is a fact the fragment was built from; nothing here is recomputed from a
/// second read, so "the Inspector and the request body agree" is structural rather than a
/// property someone has to remember to maintain.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorActivityView {
    /// The instant the index was sampled.
    pub sampled_at: UtcTimestamp,
    /// The runtime the snapshot came from.
    pub runtime_instance_id: RuntimeInstanceId,
    /// The index revision at that instant.
    pub revision: Revision,
    /// Coverage of this snapshot against the reader's scope.
    pub coverage: ActivityCoverageState,
    /// Rows the model actually saw.
    pub rows_shown: u32,
    /// Rows in the reader's scope before any cap.
    pub rows_in_scope: u64,
    /// Rows omitted for any reason, summed across the coverage counters.
    pub omitted: u64,
    /// True when a body was cut to fit its byte bound.
    pub truncated: bool,
    /// The content hash of the exact body that became the fragment. It is what makes "the
    /// Inspector shows the bytes that were sent" checkable rather than asserted.
    pub body_hash: ContentHash,
    /// Whether an activity fragment was placed in the request at all.
    pub fragment_present: bool,
}

/// What the conversation's
/// finished requests sent, and what their Providers said they read from the cache.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCacheUsageView {
    /// Finished requests of the conversation.
    pub steps: u64,
    /// Those whose Provider reported its cache use.
    pub reported_steps: u64,
    /// The input of the reported requests, so the share is of what was reported.
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
}

/// Which session runs a task that a step's task state names only by its number.
///
/// It is kept beside the step's frozen inspector for the person reading it; the model never
/// receives it.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInspectorActivitySourceView {
    /// The task's `another_task` number in the step's task state.
    pub another_task: u32,
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
}

/// A historical inspector is bound to the exact execution and context that
/// produced that step, independent of the session's current configuration.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentPromptInspection {
    pub projection: PromptInspectorProjection,
    pub execution: aura_model_domain::ModelExecutionSnapshot,
    pub context: crate::ContextProjectionSnapshot,
    /// The whole conversation's cache use when the step was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_cache: Option<SessionCacheUsageView>,
    /// Which session runs each other task the step's task state numbered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity_sources: Vec<PromptInspectorActivitySourceView>,
}
