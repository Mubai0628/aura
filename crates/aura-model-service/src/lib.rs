//! The model authority in Core: providers, endpoints and profiles and their commands,
//! availability tests, model discovery, the configuration journal, transport records and the
//! settings projections; the provider credentials it owns; and the model channel authorization.
#![forbid(unsafe_code)]

pub mod model_channel;
#[cfg(test)]
mod model_channel_tests;
pub mod p05;
pub mod p05_credential;

use anyhow::Result;
use aura_core_broker::broker::completion::DispatchOutcome;

/// Narrow P05 boundary used by the Session owner while atomically creating a
/// Session, its initial model binding and the N-A channel authorization.
pub trait InitialSessionModelBindingResolver {
    fn resolve_initial_session_model_binding(
        &self,
    ) -> Result<aura_model_domain::InitialSessionModelBinding>;
}

impl DispatchOutcome for crate::p05::P05CommandDispatch {
    fn reply_value(&self) -> Option<serde_json::Value> {
        Some(self.value.clone())
    }
}
