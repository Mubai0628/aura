//! What sub-agents exchange, without a store or a runtime: the task sheet, the reports, a
//! sub-agent as it is recorded and shown, the team tools' inputs and results, the rules for a
//! sub-agent's calls and tool table, and the limits and permissions the user sets for them. The tool registry
//! carries the tools from here; the team itself (`aura-agent-team`) adds the store and the runtime.
mod limits;
mod member;
mod permissions;
pub mod policy;
mod report;
mod review;
mod sheet;
pub mod tools;
mod view;
mod worktree;

pub use limits::{AgentTeamLimits, AgentTeamSettingsSetRequest, AgentTeamSettingsView};
pub use member::{Allowances, Grant, Member, MemberDetails, MemberState, TurnEnd, member_handle};
pub use permissions::AgentTeamPermissions;
pub use report::{Report, ReportInput, ReportInputStatus, ReportStatus};
pub use review::*;
pub use sheet::{TaskSheet, session_reference};
pub use tools::*;
pub use view::{
    AgentTeamAllowanceRevokeRequest, AgentTeamLeadView, AgentTeamListRequest, AgentTeamMemberView,
    AgentTeamStopRequest, AgentTeamView, AgentTeamWorktreeFolder, AgentTeamWorktreeRequest,
    AllowanceKind, LIST_SUMMARY_CHARS,
};
pub use worktree::*;

/// Why a team call was refused, in the words the model and the user read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

impl Refusal {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

/// How a refused call is recorded, by its code: one whose own input is wrong is a request the
/// model can correct, and one naming a sub-agent the conversation does not have names a missing
/// target. Any other refusal stays the tool's failure.
#[must_use]
pub fn refusal_category(code: &str) -> Option<aura_domain_common::ErrorCategory> {
    use aura_domain_common::ErrorCategory::{NotFound, Validation};
    match code {
        "agents.input_invalid"
        | "agents.name_invalid"
        | "agents.send_invalid"
        | "agents.send_names_conversation"
        | "agents.task_incomplete"
        | "agents.task_names_conversation"
        | "agents.task_too_long"
        | "agents.title_invalid"
        | "agents.approve_invalid"
        | "agent.report_invalid"
        | "agent.report_final_answer" => Some(Validation),
        "agents.unknown_agent" | "agents.approval_not_reviewed" | "agents.no_worktree" => {
            Some(NotFound)
        }
        _ => None,
    }
}
