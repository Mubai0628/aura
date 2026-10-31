//! Assembly from durable User/Agent/Tool facts and the frozen prompt bundle.
use super::*;
pub mod sources;
mod window_rebuild;
use aura_context_domain::*;
use aura_context_runtime::*;
use aura_model_config::{CreateModelExecutionSnapshot, ModelExecutionCredentialBinding};
use aura_persistence_core::artifact;
use aura_persistence_v2::{
    agent::{checkpoint, inbox, lineage, model_context, session, turns, work},
    context, model, tool,
};
use aura_prompt_runtime::*;
use aura_security_contract::*;
pub(super) use sources::{core_tool, frozen_prompt, history_fragment};
use sources::{prompt_state, section};
pub(super) struct Prepared {
    pub request: super::provider::Request,
    pub profile: ModelProfileConfig,
}
mod assembly;
mod clearing;
mod compilation;
mod continuation;
mod history;
pub mod preparation;
mod skills;
pub use history::protocol_completion_reminder;
use history::*;
