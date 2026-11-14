//! The Agent application is aura-agent-application's. Core keeps its tests, which run a started
//! Core, and the continuation that follows Core's own startup (it reads RuntimeState); they read
//! the module's names from here.
// Declared before the imports, as in the moved module: a test file's own glob import of a name
// these imports also provide stays the binding that file uses.
#[cfg(test)]
mod tests;
pub(crate) use aura_agent_application::agent_application::*;
#[cfg(test)]
use aura_agent_domain::*;
#[cfg(test)]
use aura_core_broker::agent_store::StoreStep;
#[cfg(test)]
use aura_domain_common::*;
#[cfg(test)]
use aura_model_domain::*;
#[cfg(test)]
use aura_projection_contract::AgentSessionProjection;
#[cfg(test)]
use std::sync::{Arc, Mutex};

pub(crate) mod continuation {
    /// Once per process, after the startup that cut the turns off: Main passed `--continue-after`.
    pub(crate) fn after_startup(state: &crate::RuntimeState) {
        let (Some(cause), crate::StartupMode::Ready(services)) =
            (state.options.continue_after, &state.startup_mode)
        else {
            return;
        };
        // New work waits for a configuration the user has to repair, as a sent message would.
        if let Ok(agent) = services.agent()
            && services.agent_capability_available()
        {
            agent.continue_after_restart_once(cause);
        }
    }
}

#[cfg(test)]
mod context {
    use super::*;
    pub(crate) use aura_agent_application::agent_application::context::sources::history_fragment;
    pub(crate) use aura_agent_application::agent_application::context::*;
    use aura_context_runtime::*;
    use aura_persistence_core::artifact;

    mod preparation {
        use super::*;

        pub(super) mod screenshots {
            use super::*;
            pub(crate) use aura_agent_application::agent_application::context::preparation::screenshots::*;
            use aura_tool_contract::{
                ModelVisibleToolResultOutcomeV1, ModelVisibleToolSuccessPayloadV1,
            };
            // The tests file sits beside the moved module's old file, `context/`.
            #[path = "../../screenshots_tests.rs"]
            mod tests;
        }
    }
}
