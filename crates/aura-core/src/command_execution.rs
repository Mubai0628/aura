//! Command execution is aura-agent-execution's. Core keeps the tests that run a started Core;
//! they read the module's names from here.
pub(crate) use aura_agent_execution::command_execution::*;
#[cfg(test)]
mod tests;

pub(crate) mod terminal_opening {
    #[cfg(test)]
    use crate::{AsyncPriorityQueue, JsonRpcRequest, RequestLedger};
    pub(crate) use aura_agent_execution::command_execution::terminal_opening::*;
    #[cfg(test)]
    use aura_domain_common::{CorrelationId, RuntimeInstanceId, ToolWireVersionV1};
    #[cfg(test)]
    use aura_ipc::{JsonRpcNotification, WireFrame};
    #[cfg(test)]
    use aura_runtime_contract::CancelDisposition;
    #[cfg(test)]
    use aura_windows_user_contract::{TerminalOpeningPhaseV1 as Phase, TerminalOpeningProgressV1};
    #[cfg(test)]
    use std::sync::{Arc, Mutex, Weak};
    #[cfg(test)]
    #[allow(clippy::unwrap_used)]
    #[path = "tests.rs"]
    mod tests;
}
