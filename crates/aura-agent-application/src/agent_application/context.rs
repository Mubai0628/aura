//! Assembly from durable User/Agent/Tool facts and the frozen prompt bundle.
use super::*;
pub mod sources;
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
use sources::{CONTROL_STATE, prompt_state, section, write_control_state};
pub(super) use sources::{core_tool, frozen_prompt, history_fragment};
mod assembly;
mod clearing;
pub(super) use clearing::rebuild as rebuild_history_fragments;
mod compilation;
mod continuation;
mod history;
mod new_results;
pub mod preparation;
mod prepare_step;
use prepare_step::Prepared;
pub(super) mod recovery;
mod skills;
pub use history::protocol_completion_reminder;
use history::*;
