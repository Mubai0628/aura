//! Which
//! conversations have a turn under way, for the sidebar: running, or waiting for the user (an
//! approval, a question, or a decision about the context).
use aura_domain_common::SessionId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionsActivityRequest {}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionActivityState {
    Running,
    Waiting,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionActivity {
    pub session_id: SessionId,
    pub state: AgentSessionActivityState,
}

/// Only the conversations with a turn under way; every other one is idle.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionsActivity {
    pub sessions: Vec<AgentSessionActivity>,
}
