#![forbid(unsafe_code)]
//! Agent task and completion facts. No network, scheduler, SQL or model SDK.

#[derive(
    Clone, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema, PartialEq, Eq,
)]
#[serde(deny_unknown_fields)]
pub struct EmptyToolInput {}
mod activity;
mod application;
mod attachment_files;
mod attachments;
mod branch;
mod clipboard;
mod command_tools;
mod commands;
mod completion;
mod continuation;
mod correction;
mod desktop;
mod inbox;
mod lifecycle;
mod lineage;
pub mod media;
mod mode_episode;
mod opening;
mod query_reply;
mod reasoning;
mod reconciliation;
mod skill_tools;
mod work;
pub use activity::*;
pub use application::*;
pub use attachment_files::*;
pub use attachments::*;
pub use branch::*;
pub use clipboard::*;
pub use command_tools::*;
pub use commands::*;
pub use completion::*;
pub use continuation::*;
pub use correction::*;
pub use desktop::*;
pub use inbox::*;
pub use lifecycle::*;
pub use lineage::*;
pub use media::*;
pub use mode_episode::*;
pub use opening::*;
pub use query_reply::*;
pub use reasoning::*;
pub use reconciliation::*;
pub use skill_tools::*;
pub use work::*;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AgentError {
    #[error("agent.identity_mismatch")]
    IdentityMismatch,
    #[error("agent.revision_conflict")]
    RevisionConflict,
    #[error("agent.invalid_transition")]
    InvalidTransition,
    #[error("task_lineage.source_invalid")]
    SourceInvalid,
    #[error("task_lineage.source_read_required")]
    SourceReadRequired,
    #[error("task_lineage.completion_review_required")]
    CompletionReviewRequired,
    #[error("task_lineage.strict_completion_not_satisfied")]
    StrictNotSatisfied,
    #[error("task_lineage.completion_blocked")]
    CompletionBlocked,
    #[error("task_lineage.limitations_not_disclosed")]
    LimitationsNotDisclosed,
    #[error("agent.invalid_input")]
    InvalidInput,
}
