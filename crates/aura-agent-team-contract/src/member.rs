//! One sub-agent of a conversation, as the team keeps it.
use crate::TaskSheet;
use aura_domain_common::{ModelProfileId, SessionId, TurnTaskId, UtcTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "AgentTeamMemberState")]
pub enum MemberState {
    /// Working, or ready to work on what it was last sent.
    Running,
    /// Handed in a final report; it waits for the main agent.
    Reported,
    /// Stopped by the main agent or the user; it does nothing more.
    Stopped,
}

impl MemberState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Reported => "reported",
            Self::Stopped => "stopped",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [Self::Running, Self::Reported, Self::Stopped].into_iter().find(|s| s.as_str() == text)
    }
}

/// Its tools and where it works, fixed when it is started. What its calls may do is its
/// conversation's permission mode and rules.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// The tools the main agent narrowed it to, by name, besides `agent.report`; none when it has
    /// every tool a sub-agent may have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// It works in the starting conversation's workspace, with no worktree of its own.
    #[serde(rename = "read_only")]
    pub shared: bool,
}

/// What the user lets a sub-agent's commands do without asking, beyond its mode: from the
/// settings when it started, or the user's answer on one of its questions. They go with it.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AgentTeamMemberAllowances")]
pub struct Allowances {
    /// Its commands delete without asking where their targets lie in its worktree or temporary
    /// folder, or are left to a variable or the pipeline while the command stands there. Only a
    /// sub-agent with a worktree has it.
    #[serde(default)]
    pub deletions: bool,
    /// Its commands write outside its workspace without asking; a deletion there still asks.
    #[serde(default)]
    pub outside_writes: bool,
    /// Its downloads run without asking.
    #[serde(default)]
    pub downloads: bool,
}

impl Allowances {
    #[must_use]
    pub fn none(&self) -> bool {
        *self == Self::default()
    }
}

/// How a turn of a sub-agent's conversation ended.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TurnEnd {
    pub turn_task_id: TurnTaskId,
    /// `completed`, `failed`, `cancelled` or `recovery_required`.
    pub outcome: String,
    /// Its final answer, when it completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Why it did not complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The facts kept with a sub-agent besides its identity and state.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemberDetails {
    pub name: String,
    pub model_profile_id: ModelProfileId,
    pub model_name: String,
    pub sheet: TaskSheet,
    pub grant: Grant,
    /// The permission mode it started in; none in a record from before sub-agents had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<aura_security_contract::PermissionMode>,
    #[serde(default, skip_serializing_if = "Allowances::none")]
    pub allowed: Allowances,
    /// Its questions that went to the main agent or that the settings answered, newest last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviews: Vec<crate::Review>,
    /// The newest report the main agent has been given.
    #[serde(default)]
    pub reports_seen: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn: Option<TurnEnd>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub child_session_id: SessionId,
    pub parent_session_id: SessionId,
    /// `A1`, `A2`… within the starting conversation.
    pub handle: String,
    /// The `agents.spawn` call that started it.
    pub spawn_call: String,
    pub state: MemberState,
    pub revision: u64,
    pub details: MemberDetails,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

/// The handle of a conversation's `n`th sub-agent.
#[must_use]
pub fn member_handle(n: u32) -> String {
    format!("A{n}")
}
