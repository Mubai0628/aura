//! Managed-shell composition. Orchestrator supplies an AuthorizedDispatch;
//! diagnostic runner and Main identity cannot substitute for that authority.
mod composition;

mod agent_terminal;
pub(crate) use agent_terminal::sensitive::SensitiveWait;
pub(crate) mod background_observations;
mod control;
mod diagnostics;
pub(crate) mod managed_operations;
mod output;
mod permission_scope;
mod port;
mod process;
mod runtime;
mod selection;
pub(crate) mod terminal;
pub(crate) mod terminal_opening;
#[cfg(test)]
mod tests;
use aura_backend_api::ExecutionBackend;
use aura_backend_windows_user::compat::CompatExecutionBackend;
use aura_domain_common::{RuntimeInstanceId, SessionId};
use aura_security_contract::PermissionMode;
use std::sync::Arc;

pub(crate) struct CommandExecution {
    compat: Arc<CompatExecutionBackend>,
    persistence: Arc<aura_persistence::PersistenceCoordinator>,
    journal: Arc<crate::broker::BrokerJournal>,
    terminal_outputs:
        std::sync::Mutex<std::collections::BTreeMap<String, tokio::task::JoinHandle<()>>>,
    agent_terminals: Arc<agent_terminal::AgentTerminals>,
}
#[cfg(test)]
impl CommandExecution {
    pub(crate) fn native_terminal_for_test(
        &self,
        session: SessionId,
        id: &str,
    ) -> anyhow::Result<Arc<aura_backend_windows_user::compat::TerminalSession>> {
        self.compat.terminals.get(session, id)
    }
}
