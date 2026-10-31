use super::*;
use aura_core_base::{readiness, release::CoreReleaseContext};
use aura_core_broker::agent_store;

impl CommandExecution {
    pub fn open(
        persistence: Arc<aura_persistence::PersistenceCoordinator>,
        journal: Arc<crate::broker::BrokerJournal>,
        store: Arc<agent_store::AgentStore>,
        activity_index: aura_activity_runtime::ActivityIndex,
        runtime: RuntimeInstanceId,
        readiness: &readiness::ReadinessStore,
        release: &CoreReleaseContext,
    ) -> anyhow::Result<Self> {
        let port = Arc::new(port::CorePort {
            persistence: persistence.clone(),
            journal: journal.clone(),
            store: store.clone(),
            activity_index,
            activity_observer: aura_activity_runtime::ActivityPublicationObserver::default(),
            // R2: nothing observed yet for any managed background instance.
            background_observations: Arc::new(
                background_observations::BackgroundObservations::default(),
            ),
        });
        // Restricted implementations remain behind ExecutionBackend in their crate.
        // Default startup does not discover or verify the optional R06 installation.
        let compat =
            Arc::new(CompatExecutionBackend::new(&runtime::managed_root(release)?, runtime, port)?);
        readiness
            .register_backend(
                aura_execution_contract::BackendId(
                    aura_backend_windows_user::compat::COMPAT_BACKEND_ID.into(),
                ),
                compat.clone(),
            )
            .map_err(anyhow::Error::msg)?;
        let agent_terminals = Arc::new(agent_terminal::AgentTerminals::new(
            journal.clone(),
            store,
            compat.terminals.clone(),
            runtime,
        )?);
        Ok(Self {
            compat,
            persistence,
            journal,
            terminal_outputs: Default::default(),
            agent_terminals,
        })
    }
}
