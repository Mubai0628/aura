//! Which
//! conversations have a turn under way, for the sidebar: running, or waiting for the user (an
//! approval, a question, or a decision about the context); and how the turns a caller saw under
//! way ended, for the notifications of a conversation the user is not looking at.
use aura_domain_common::{SessionId, TurnTaskId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionsActivityRequest {
    /// Turns the caller saw under way that are no longer: those that have ended come back in
    /// `ended`, with how. At most 64 are read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ended_turns: Vec<TurnTaskId>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionActivityState {
    Running,
    Waiting,
}

/// What a waiting conversation waits for.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionWaitingFor {
    Approval,
    Question,
    /// A decision about a context too large for the model.
    Context,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionActivity {
    pub session_id: SessionId,
    pub state: AgentSessionActivityState,
    /// The conversation's turns under way.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turn_task_ids: Vec<TurnTaskId>,
    /// What it waits for, while it waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_for: Option<AgentSessionWaitingFor>,
}

/// How a turn ended.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentTurnOutcome {
    Completed,
    Failed,
    Cancelled,
    /// Stopped where it cannot go on by itself; the user decides how it continues.
    RecoveryRequired,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTurnEnded {
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub outcome: AgentTurnOutcome,
}

/// Only the conversations with a turn under way; every other one is idle.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionsActivity {
    pub sessions: Vec<AgentSessionActivity>,
    /// Of the turns the request named, those that have ended.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ended: Vec<AgentTurnEnded>,
}
