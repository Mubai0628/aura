//! A conversation's sub-agents as the desktop shows them.
use crate::{MemberState, ReportStatus};
use aura_domain_common::{ApprovalRequestId, ModelProfileId, SessionId, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How much of a report's summary the list carries.
pub const LIST_SUMMARY_CHARS: usize = 400;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamListRequest {
    pub session_id: SessionId,
}

/// The user stops one sub-agent of a conversation.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamStopRequest {
    /// The conversation that started it.
    pub session_id: SessionId,
    /// Its handle: `A1`, `A2`…
    pub agent: String,
}

/// One sub-agent of a conversation, for its worktree's folder.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamWorktreeRequest {
    /// The conversation that started it.
    pub session_id: SessionId,
    /// Its handle: `A1`, `A2`…
    pub agent: String,
}

/// The folder of a sub-agent's worktree still in use, for Main to open.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamWorktreeFolder {
    pub path: String,
}

/// What a sub-agent's commands may do without asking, one kind at a time.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "AgentTeamAllowanceKind")]
pub enum AllowanceKind {
    Deletions,
    OutsideWrites,
    Downloads,
}

/// The user takes back one thing a sub-agent's commands could do without asking.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamAllowanceRevokeRequest {
    /// The conversation that started it.
    pub session_id: SessionId,
    /// Its handle: `A1`, `A2`…
    pub agent: String,
    pub allowance: AllowanceKind,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamView {
    /// The conversation's own agent, first in the list.
    pub lead: AgentTeamLeadView,
    pub members: Vec<AgentTeamMemberView>,
}

/// The conversation's own agent.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamLeadView {
    /// The model the conversation uses, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_profile_id: Option<ModelProfileId>,
    /// Its name, while it is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub working: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// What the conversation's answers used so far, as their providers counted.
    pub tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamMemberView {
    pub agent: String,
    pub name: String,
    /// The conversation it works in.
    pub session_id: SessionId,
    /// The workspace that conversation works in: its worktree's, or the starting conversation's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    pub model_profile_id: ModelProfileId,
    pub model: String,
    pub state: MemberState,
    pub working: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Approvals it waits for the user to decide, in the conversation that started it.
    pub approvals: Vec<ApprovalRequestId>,
    /// Approvals it waits for the main agent to decide first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviewing: Vec<ApprovalRequestId>,
    /// What its answers used so far, as their providers counted.
    pub tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_report: Option<ReportStatus>,
    /// The newest report's summary, cut to [`LIST_SUMMARY_CHARS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The newest report's question for the main agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// The worktree it changes files in, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<crate::AgentWorktreeView>,
    /// Its permission mode now, which the user may change while it works.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<aura_security_contract::PermissionMode>,
    /// What the user lets its commands do without asking.
    #[serde(default)]
    pub allowed: crate::Allowances,
    /// Its questions the main agent or the settings answered, newest last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviews: Vec<crate::Review>,
}
