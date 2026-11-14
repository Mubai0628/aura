//! The main agent answering a sub-agent's question first, when the settings let it: what it is
//! shown, how it answers, and what the sub-agent keeps of it. A question that only deletes or
//! writes in the sub-agent's own worktree and temporary folder goes to the main agent for at most
//! [`REVIEW_SECONDS`]; its answer covers that one call, or, where the user's card would offer it,
//! the sub-agent's later deletions in its own folders too. Everything else asks the user.
use aura_domain_common::{ApprovalRequestId, UtcTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const APPROVE: &str = "agents.approve";

/// How long the main agent has to answer before the user is asked instead.
pub const REVIEW_SECONDS: u32 = 120;

/// The reviews a sub-agent keeps, newest last.
pub const REVIEWS_KEPT: usize = 32;

/// An answer to a sub-agent's question.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "AgentApprovalAnswer")]
pub enum ApprovalAnswer {
    /// This one call runs.
    Allow,
    /// This call runs, and the sub-agent's later deletions in its own worktree and temporary
    /// folder ask no more until it is merged or discarded, as the user's card allows them. Only
    /// for a question whose `allow_workspace` is true.
    AllowWorkspace,
    /// It does not run.
    Deny,
    /// The user decides.
    AskUser,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentsApproveInput {
    /// The sub-agent, as agents.spawn named it (A1, A2, …).
    pub agent: String,
    /// The question, as agents.wait or agents.status gave it.
    pub approval_id: ApprovalRequestId,
    pub answer: ApprovalAnswer,
    /// Why, in a sentence the user reads afterwards.
    #[schemars(length(min = 1, max = 500))]
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentsApproveResult {
    pub agent: String,
    pub approval_id: ApprovalRequestId,
    pub answer: ApprovalAnswer,
}

/// A question of a sub-agent's for the main agent to answer first.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentApprovalRequestView {
    pub agent: String,
    pub approval_id: ApprovalRequestId,
    /// The tool the sub-agent called.
    pub tool: String,
    /// What the call does: the command, or the file it changes; cut to 2000 characters.
    pub request: String,
    /// What the sub-agent said the call is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    /// It deletes files for good.
    pub deletes: bool,
    /// allow_workspace may answer it.
    #[serde(default)]
    pub allow_workspace: bool,
    /// Seconds left before the user is asked instead.
    pub seconds_left: u32,
}

/// Who answered a sub-agent's question in the user's place.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "AgentApprovalAnsweredBy")]
pub enum AnsweredBy {
    MainAgent,
    /// The settings' presets.
    Settings,
}

/// A question of the sub-agent's that went to the main agent, or that the settings answered.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AgentApprovalReview")]
pub struct Review {
    pub approval_id: ApprovalRequestId,
    /// When agents.wait or agents.status first gave it to the main agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presented_at: Option<UtcTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered: Option<Answered>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AgentApprovalAnswered")]
pub struct Answered {
    pub by: AnsweredBy,
    pub answer: ApprovalAnswer,
    pub reason: String,
    pub at: UtcTimestamp,
}

/// Records `change` on the review of `approval` in `reviews`, adding it when new and keeping the
/// newest [`REVIEWS_KEPT`].
pub fn note_review(
    reviews: &mut Vec<Review>,
    approval: ApprovalRequestId,
    change: impl FnOnce(&mut Review),
) {
    let at = match reviews.iter().position(|review| review.approval_id == approval) {
        Some(at) => at,
        None => {
            reviews.push(Review { approval_id: approval, presented_at: None, answered: None });
            reviews.len() - 1
        }
    };
    change(&mut reviews[at]);
    let over = reviews.len().saturating_sub(REVIEWS_KEPT);
    reviews.drain(..over);
}
