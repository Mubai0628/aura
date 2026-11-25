//! Which
//! conversations have a turn under way, for the sidebar: running, or waiting for the user (an
//! approval, a question, a secure input a terminal asked for, or a decision about the context);
//! and how the turns a caller saw under way ended, for the notifications of a conversation the
//! user is not looking at.
use aura_domain_common::{EventId, SessionId, TurnTaskId};
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
    /// A secure input a terminal asked for, typed in Aura's own form.
    SensitiveInput,
    /// A decision about a context too large for the model.
    Context,
}

impl AgentSessionWaitingFor {
    /// Of two waits, the one a conversation shows: an approval, then a question, then a secure
    /// input, then a decision about the context.
    #[must_use]
    pub fn first(self, other: Self) -> Self {
        if other.rank() < self.rank() { other } else { self }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Approval => 0,
            Self::Question => 1,
            Self::SensitiveInput => 2,
            Self::Context => 3,
        }
    }
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentPeerResultKind {
    Dispatch,
    Question,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentPeerResultState {
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

/// A result its requestor has not looked at yet. The result body stays behind the
/// delivery receipt; a notice never starts a turn or consumes collaboration capacity.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentPeerResultNotice {
    /// The requestor conversation that should see this notice.
    pub session_id: SessionId,
    pub delivery_id: EventId,
    pub kind: AgentPeerResultKind,
    pub target_session_id: SessionId,
    pub target_title: String,
    pub state: AgentPeerResultState,
}

/// Only the conversations with a turn under way; every other one is idle.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionsActivity {
    pub sessions: Vec<AgentSessionActivity>,
    /// Of the turns the request named, those that have ended.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ended: Vec<AgentTurnEnded>,
    /// Results the requestor has not opened, newest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peer_results: Vec<AgentPeerResultNotice>,
}

impl AgentSessionsActivity {
    /// Marks `sessions` as waiting for `waiting` too, for a wait Core keeps outside the turns
    /// table; a wait that shows first stays shown.
    pub fn waiting_also(&mut self, sessions: &[SessionId], waiting: AgentSessionWaitingFor) {
        for session in sessions {
            match self.sessions.iter_mut().find(|entry| entry.session_id == *session) {
                Some(entry) => {
                    entry.state = AgentSessionActivityState::Waiting;
                    entry.waiting_for =
                        Some(entry.waiting_for.map_or(waiting, |known| known.first(waiting)));
                }
                None => self.sessions.push(AgentSessionActivity {
                    session_id: *session,
                    state: AgentSessionActivityState::Waiting,
                    turn_task_ids: Vec::new(),
                    waiting_for: Some(waiting),
                }),
            }
        }
    }
}
