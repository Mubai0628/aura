//! Read models for the desktop. They reference durable identities, never raw
//! Provider evidence, credentials, or caller-supplied execution authority.
//!
//! The three combining display types — `AgentAssistantView`, `AgentTurnView` and
//! `AgentSessionProjection` — live in the display contract
//! (`aura_projection_contract::agent_display`) because they join Agent turn facts
//! with the Context domain's usage meter. This module keeps the Agent domain's own
//! message read models, which depend on `common` only.
use crate::*;
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentHistoryRequest {
    pub session_id: SessionId,
    pub before_turn_task_id: Option<TurnTaskId>,
    pub within_turn: Option<AgentTurnHistoryCursor>,
    pub limit: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTurnHistoryCursor {
    pub turn_task_id: TurnTaskId,
    pub before_user_event_id: Option<EventId>,
    pub before_model_step_id: Option<ModelStepId>,
}
/// A bounded view, not an OriginalUserMessage whose content hash would imply
/// the complete text. The immutable source remains available via message.text.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentUserMessageView {
    pub event_id: EventId,
    pub text: String,
    pub total_bytes: u64,
    /// Identity of the complete immutable text, not of the bounded preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed_revision: Option<String>,
    pub origin: UserMessageOrigin,
    pub at: UtcTimestamp,
    /// The Skills the user chose with this message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<LoadedSkillEntry>,
    /// The pictures and files added to this message. `text` and `total_bytes` are then
    /// what the user typed; the model read them with the message ([`message_with_attachments`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AgentMessageAttachment>,
}

/// A history poll's view of one completion report: where it lives, how long it is,
/// and a short prefix. It is not the model's final answer.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentCompletionSummary {
    pub total_bytes: u64,
    pub preview: String,
    /// Blake3 of the report bytes, recorded when the turn completed. Older rows omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentResultRequest {
    pub session_id: SessionId,
    pub tool_call_id: ToolCallId,
    pub offset: u64,
    pub limit: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentResultView {
    pub tool_call_id: ToolCallId,
    pub artifact_id: ArtifactId,
    pub text: String,
    pub next_offset: Option<u64>,
    pub total_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentInspectorRequest {
    pub session_id: SessionId,
    pub model_step_id: ModelStepId,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRelatedTurnKind {
    ImplementPlan,
    Recover,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentRelatedTurnRequest {
    pub session_id: SessionId,
    pub source_turn_task_id: TurnTaskId,
    pub expected_revision: Revision,
    pub kind: AgentRelatedTurnKind,
    /// Explicit user adjustment of the recovered Goal's total limit. Omission
    /// carries the original limit; cumulative usage is never reset.
    pub goal_budget: Option<GoalBudget>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentQueryFileRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub query_id: UserQueryId,
    pub expected_revision: Revision,
    pub question_id: String,
    pub relative_path: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentQueryFileSelection {
    pub file_reference_id: FileReferenceId,
    pub relative_path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentMessageTextSource {
    User {
        event_id: EventId,
    },
    Assistant {
        model_step_id: ModelStepId,
    },
    Reasoning {
        model_step_id: ModelStepId,
    },
    /// The completion report stored on the turn, read a page at a time.
    Completion {
        turn_task_id: TurnTaskId,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentMessageTextRequest {
    pub session_id: SessionId,
    pub source: AgentMessageTextSource,
    pub offset: u64,
    pub limit: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentMessageTextPage {
    pub source: AgentMessageTextSource,
    pub text: String,
    pub next_offset: Option<u64>,
    pub total_bytes: u64,
    /// The body this page was read from, `None` for sealed text. A client that
    /// asked from a byte of another body must discard the page instead of
    /// appending it.
    pub body_id: Option<String>,
    /// Same immutable revision as the preview. Missing on live and legacy data
    /// without a recorded digest; clients must not cache those as sealed text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed_revision: Option<String>,
    /// The byte the returned text really starts at. It is the requested offset
    /// for sealed text, and the nearest character boundary at or before it for a
    /// live body that moved while the request was in flight.
    pub from_offset: u64,
}
