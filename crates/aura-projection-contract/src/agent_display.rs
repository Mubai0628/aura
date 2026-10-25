//! Read models for the desktop. They reference durable identities, never raw
//! Provider evidence, credentials, or caller-supplied execution authority.
//!
//! This is the display contract's own Agent module: it owns the combination of
//! Agent turn facts with the Context domain's usage meter, so the Agent domain
//! keeps only its business identities and states. The owned types
//! were moved here unchanged from `aura-agent-domain`; their fields, types,
//! serde attributes, defaults, omission rules and schema names are the same.

use aura_agent_domain::{
    AgentBranchCopyingView, AgentBranchOrigin, AgentCompletionSummary, AgentFinalTextSource,
    AgentInboxProjection, AgentModeEpisodeView, AgentRelatedTurnRequest, AgentRunProjection,
    AgentTurnHistoryCursor, AgentUserMessageView, GoalState, PlanRecord, TodoState, TurnTask,
};
use aura_context_domain::ContextUsageSummaryView;
use aura_domain_common::{
    ContextProjectionSnapshotId, ModelProfileId, ModelStepId, Revision, RuntimeInstanceId,
    SessionId, ToolCallId, TurnTaskId, UtcTimestamp,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentAssistantView {
    pub model_step_id: ModelStepId,
    pub model_profile_id: Option<ModelProfileId>,
    pub context_snapshot_id: Option<ContextProjectionSnapshotId>,
    pub text: String,
    pub reasoning: String,
    pub text_total_bytes: u64,
    pub reasoning_total_bytes: u64,
    pub partial: bool,
    pub stream_sequence: Option<u64>,
    /// Opaque identity of the live visible body of this step. Text and reasoning
    /// come from one stream, so one identity describes both: equal values mean
    /// the body only grew, a different value means a retry restarted it and any
    /// bytes a client already holds belong to another body. `None` once the
    /// step's output is sealed and the sealed text is authoritative.
    #[serde(default)]
    pub stream_body_id: Option<String>,
    /// Stored output digest. Both visible sides belong to this sealed revision;
    /// the message source distinguishes assistant text from reasoning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed_revision: Option<String>,
    /// Frozen context meter copied from the prompt inspection. The ring uses this
    /// and does not load the full inspector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_usage: Option<ContextUsageSummaryView>,
    /// `compaction.state == running` from that same inspection, when recorded.
    #[serde(default, skip_serializing_if = "is_false")]
    pub context_compaction_running: bool,
    /// This
    /// step's context was compacted first — older history became a summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_compacted: Option<AgentContextCompactedView>,
    /// This step's request first gave older tool results' room back, with
    /// nothing summarized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_cleared: Option<AgentContextClearedView>,
    /// The provider's backup connection this step
    /// went through, because the one before it failed before answering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_connection: Option<String>,
    pub tool_call_ids: Vec<ToolCallId>,
    /// While the step runs, what it is writing into tool calls. Core keeps
    /// it in memory only; it is empty once the step ends and is never stored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_input: Vec<AgentToolInputView>,
    pub at: UtcTimestamp,
}
fn is_false(value: &bool) -> bool {
    !*value
}

/// A compaction just before a step: who asked for it and how much recent history stayed as it was.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentContextCompactedView {
    /// The user asked for it (`/compact`); otherwise Aura did when the context neared its limit.
    pub manual: bool,
    pub retained_recent_tokens: u64,
    /// The request's estimated input before it was made smaller, and after (the
    /// step's own meter); absent for steps prepared by older versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens_before: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens_after: Option<u64>,
    /// Older tool results cleared first, in the same preparation.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cleared_tool_results: u32,
}
fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Older tool results cleared just before a step, with nothing summarized - how
/// many, and the request's estimated input before and after (the step's own meter).
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentContextClearedView {
    pub cleared_tool_results: u32,
    pub input_tokens_before: u64,
    pub input_tokens_after: u64,
}

/// A tool call a running step is writing: the tool's name as the conversation shows it, the file
/// it names once that part of its input has arrived, and how much of its input has arrived.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolInputView {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub received_bytes: u64,
    pub complete: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTurnView {
    pub turn: TurnTask,
    pub user_messages: Vec<AgentUserMessageView>,
    pub assistant_messages: Vec<AgentAssistantView>,
    pub next_messages: Option<AgentTurnHistoryCursor>,
    pub todo: TodoState,
    pub plan: Option<PlanRecord>,
    pub goal: Option<GoalState>,
    pub source: Option<AgentRelatedTurnRequest>,
    pub diagnostic: Option<String>,
    /// Where this turn's final answer comes from. Absent on historical turns
    /// whose completion source predates this field; they render as a report.
    #[serde(default)]
    pub final_text_source: Option<AgentFinalTextSource>,
    /// Bounded facts of a completion report. The authoritative report stays in the
    /// turn record; this is only what a history poll needs before the body is paged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<AgentCompletionSummary>,
    /// A turn this branch carried from the conversation it was made from. It is shown and
    /// can be copied or branched from again; it cannot be continued, retried or implemented here.
    #[serde(default, skip_serializing_if = "is_false")]
    pub imported: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionProjection {
    pub session_id: SessionId,
    pub runtime_instance_id: RuntimeInstanceId,
    pub revision: Revision,
    pub active: Option<AgentRunProjection>,
    pub inbox: AgentInboxProjection,
    pub turns: Vec<AgentTurnView>,
    pub next_before_turn_task_id: Option<TurnTaskId>,
    #[serde(default)]
    pub mode_episode: Option<AgentModeEpisodeView>,
    /// Set on a branch: the conversation and turn it was made from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_origin: Option<AgentBranchOrigin>,
    /// Set while a branch's older turns are still being carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_copying: Option<AgentBranchCopyingView>,
}
