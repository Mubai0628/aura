//! A new conversation that starts from one finished turn of another. It carries the
//! conversation up to that turn — what the user sees and what the model is sent — and nothing that
//! acts: no operations, approvals, grants, queued messages, terminals or processes.
use aura_domain_common::{SessionId, TurnTaskId, UtcTimestamp, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Branch this conversation at one of its finished turns. An authenticated user command; a model
/// tool cannot call it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionBranchRequest {
    pub session_id: SessionId,
    /// The finished turn the branch continues from; it and every turn before it are carried.
    pub turn_task_id: TurnTaskId,
}

/// Where a branch came from: shown above its first own turn.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentBranchOrigin {
    pub source_session_id: SessionId,
    /// The conversation's title when the branch was made.
    pub source_title: String,
    pub source_turn_task_id: TurnTaskId,
    /// The turn's number in that conversation, counting from 1.
    pub turn_number: u32,
    pub created_at: UtcTimestamp,
}

/// How far a branch's copy is while its older turns are still being
/// carried after it opened with its newest ones.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentBranchCopyingView {
    pub copied_turns: u32,
    pub total_turns: u32,
    /// The copy stopped before every turn was carried.
    pub failed: bool,
}

/// The new conversation.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionBranchResult {
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub origin: AgentBranchOrigin,
}
