use aura_domain_common::ToolCallId;
use aura_domain_common::{
    AgentRunId, ContentHash, ContextProjectionSnapshotId, EventId, PromptBundleSnapshotId,
    SessionId, SessionSequence, TurnTaskId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    CompactionAnchorManifest, CompactionSourceSpan, ContextCompactionStateView,
    ContextOverflowDiagnosticView, ContextRetentionException, ContextSectionKind,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSearchMode {
    Natural,
    ExactPhrase,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSearchFilters {
    pub source_types: Vec<String>,
    pub turn_task_id: Option<TurnTaskId>,
    pub agent_run_id: Option<AgentRunId>,
    pub include_reasoning: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSearchInput {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub query: String,
    pub mode: ContextSearchMode,
    pub filters: ContextSearchFilters,
    pub limit: u32,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSearchMatch {
    pub source_type: String,
    pub source_id: String,
    pub sequence: SessionSequence,
    pub excerpt: String,
    pub match_start: u64,
    pub match_end: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSearchResult {
    pub matches: Vec<ContextSearchMatch>,
    pub next_cursor: Option<String>,
    pub index_complete_through: SessionSequence,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextEventSelector {
    Event { event_id: EventId },
    SequenceRange { start: SessionSequence, end: SessionSequence },
    TurnTask { turn_task_id: TurnTaskId },
    HistoryAtomicGroup { group_ref: crate::HistoryAtomicGroupRefV1 },
    AtomicGroup { atomic_group_id: String },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextReadEventsInput {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub selectors: Vec<ContextEventSelector>,
    pub character_limit: u64,
    pub cursor: Option<String>,
}

/// Core injects session/run; a model may select only already readable events.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentContextReadEventsInput {
    pub selectors: Vec<ContextEventSelector>,
    /// Characters per page, default 8192, at most 262144. An event's text is never cut to fit a
    /// page: an event longer than this comes with an empty content and its length in
    /// content_chars.
    #[serde(default = "default_character_limit")]
    pub character_limit: u64,
    pub cursor: Option<String>,
    /// With a single event selector and no cursor: read that event's text from this character
    /// on, character_limit characters at most; content_next_offset continues it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_offset: Option<u64>,
}
fn default_character_limit() -> u64 {
    8_192
}
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentContextReadToolResultInput {
    pub tool_call_id: ToolCallId,
    /// Also return the call's input. Default false.
    #[serde(default)]
    pub include_input: bool,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextReadToolResultInput {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub tool_call_id: ToolCallId,
    pub include_input: bool,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextReadEventView {
    pub event_id: EventId,
    pub session_sequence: SessionSequence,
    pub event_type: String,
    pub actor: String,
    pub history_atomic_group_ref: Option<crate::HistoryAtomicGroupRefV1>,
    pub atomic_group_id: Option<String>,
    pub legacy_atomic_group_verification: Option<crate::LegacyHistoryAtomicGroupVerificationV1>,
    pub content: String,
    /// The hash of the event's whole text, also when `content` is a part of it or empty.
    pub content_hash: ContentHash,
    /// The length in characters of the event's whole text, set when `content` is not all of it:
    /// empty for an event longer than the page's character_limit, or the part content_offset read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_chars: Option<u64>,
    /// Where a part read with content_offset starts in the event's text, in characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_offset: Option<u64>,
    /// The content_offset that reads on after this part, when the text goes on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_next_offset: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextReadEventsResult {
    pub events: Vec<ContextReadEventView>,
    pub next_cursor: Option<String>,
    pub character_count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextReadToolResultView {
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub input: Option<String>,
    pub result: String,
    pub is_error: bool,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextReadToolResultResult {
    pub value: ContextReadToolResultView,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextReadCompactionInput {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub source_context_snapshot_id: ContextProjectionSnapshotId,
    pub sequence: u32,
}

/// Session/run authority is supplied by the Agent's admitted invocation.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentContextReadCompactionInput {
    pub source_context_snapshot_id: ContextProjectionSnapshotId,
    pub sequence: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextReadCompactionResult {
    pub source_context_snapshot_id: ContextProjectionSnapshotId,
    pub sequence: u32,
    pub historical_snapshot: String,
    pub active_turn_summary: String,
    pub structured_compaction: String,
    pub source_span: CompactionSourceSpan,
    pub anchor_manifest: CompactionAnchorManifest,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSectionUsageView {
    pub kind: ContextSectionKind,
    pub estimated_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskLineageContextStatusView {
    pub lineage_id: aura_domain_common::TaskLineageId,
    pub revision: aura_domain_common::Revision,
    pub state: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextStatusView {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub current_context_snapshot_id: Option<ContextProjectionSnapshotId>,
    pub prompt_bundle_snapshot_id: Option<PromptBundleSnapshotId>,
    pub runtime_effective_window_tokens: u64,
    pub request_input_budget_tokens: u64,
    pub estimated_input_tokens: u64,
    pub section_usage: Vec<ContextSectionUsageView>,
    pub compaction_state: ContextCompactionStateView,
    pub retention_exceptions: Vec<ContextRetentionException>,
    pub active_task_lineage: Option<TaskLineageContextStatusView>,
    pub overflow: Option<ContextOverflowDiagnosticView>,
}
