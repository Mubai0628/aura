//! The tools' input and result types. The tool definitions take their schemas from here.
use crate::{MemberState, Report, TaskSheet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How a message reached a sub-agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// Added to the turn it is working on.
    AddedToTurn,
    /// Started a new turn.
    NewTurn,
}

pub const SPAWN: &str = "agents.spawn";
pub const STATUS: &str = "agents.status";
pub const WAIT: &str = "agents.wait";
pub const SEND: &str = "agents.send";
pub const RESULT: &str = "agents.result";
pub const STOP: &str = "agents.stop";
pub const REPORT: &str = "agent.report";

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentsSpawnInput {
    /// A short name for the task, shown to the user.
    pub name: String,
    pub task: TaskSheet,
    /// The model, by name or id: one the user allowed for sub-agents. By default this
    /// conversation's model when it is allowed, else the first allowed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Only to narrow its tools: the ones it may use, by name, from this conversation's. Left out,
    /// it has every tool of this conversation but the main agent's own (agents.*, the user's
    /// desktop and Chrome, asking the user, ending the turn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// It works in a worktree of its own (a copy of the repository on a new branch); otherwise in
    /// this workspace. What it may change is its permission mode, which the user sets.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub worktree: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentsSpawnResult {
    /// The handle the other agents.* tools take.
    pub agent: String,
    pub name: String,
    pub model: String,
    /// The tools it was narrowed to; left out when it has every tool a sub-agent may have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// The permission mode it starts in.
    pub mode: aura_security_contract::PermissionMode,
    pub state: MemberState,
    /// The branch of its worktree, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentsStatusInput {
    /// One sub-agent's handle; all of them by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentsStatusResult {
    pub agents: Vec<AgentStatusView>,
    /// Files two or more sub-agents changed, each in its own worktree: merging them may conflict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlaps: Vec<String>,
    /// Questions of theirs for you to answer first with agents.approve.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approvals: Vec<crate::AgentApprovalRequestView>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentStatusView {
    pub agent: String,
    pub name: String,
    pub model: String,
    pub state: MemberState,
    /// A turn of it is under way.
    pub working: bool,
    /// The tool it runs now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// It waits for the user to approve something.
    pub waiting_for_approval: bool,
    pub reports: u32,
    /// Reports not yet given to you by agents.wait.
    pub unread_reports: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_report: Option<ReportView>,
    /// The branch of its worktree, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// How its turn under way is going.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<AgentProgressView>,
}

/// How a working sub-agent's turn is going.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentProgressView {
    /// Seconds its turn has run.
    pub turn_seconds: u64,
    /// The tool it runs now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Seconds its model's answer under way has taken so far, waiting for its connection or
    /// arriving.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_seconds: Option<u64>,
    /// Seconds since its provider last sent anything for that answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_seconds: Option<u64>,
    /// How many of its turn's steps were asked again after the provider failed them.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub retries: u32,
    /// The latest of those failures, in words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<String>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentsWaitInput {
    /// The handles to wait for; by default every sub-agent that is running.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
    /// How long to wait at most: 600 seconds by default, 1800 at most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentsWaitResult {
    /// No new report came in time.
    pub timed_out: bool,
    /// The reports not given to you before, oldest first.
    pub reports: Vec<ReportView>,
    /// The ones waited for that are still working.
    pub working: Vec<String>,
    /// How their turns are going, by handle.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub progress: std::collections::BTreeMap<String, AgentProgressView>,
    /// Files two or more sub-agents changed, each in its own worktree: merging them may conflict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlaps: Vec<String>,
    /// Questions of the sub-agents for you to answer first with agents.approve.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approvals: Vec<crate::AgentApprovalRequestView>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentsSendInput {
    pub agent: String,
    /// Facts or an answer for it. It does not widen what it may do.
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentsSendResult {
    pub agent: String,
    pub delivered: Delivery,
    pub state: MemberState,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentsResultInput {
    pub agent: String,
    /// Where to start reading its final answer, in characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// How many characters of it: 8000 by default, 32000 at most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentsResultResult {
    pub agent: String,
    pub name: String,
    pub state: MemberState,
    /// Its newest final report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<ReportView>,
    /// Its last turn's final answer, from offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    pub answer_chars: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentsStopInput {
    pub agent: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentsStopResult {
    pub agent: String,
    pub state: MemberState,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentReportResult {
    pub sequence: u32,
    /// What to do now.
    pub next: String,
}

/// A report as the main agent reads it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ReportView {
    pub agent: String,
    pub sequence: u32,
    pub report: Report,
    /// Aura wrote it: the turn ended without a final report.
    pub by_turn_end: bool,
    pub at: String,
}
