use crate::*;

/// Attaches the live runtime's dependent-service rebuilder to the recovery entry.
///
/// It reuses the process's existing owners — persistence,
/// broker, activity index, Agent Store and the Agent application's own supervisor — and holds
/// the runtime's own slots, so it is not a second owner of anything and cannot resurrect a
/// service after shutdown (the slots are `Arc`s the rebuilder shares, not new ones it owns).
pub(crate) fn attach_dependent_rebuilder(
    services: &mut RuntimeServices,
    startup_ledger: Arc<service_rebuild::StartupLedger>,
) {
    // The Agent-side owners are retained when the process really built them, and are installed
    // lazily when it did not: a damaged `tools.json` means the startup path refused the
    // execution layer, and a live supply must be able to create the one the process never got.
    let agent_owners =
        Arc::new(std::sync::Mutex::new(services.agent_execution.get().map(|execution| {
            Arc::new(AgentLiveOwners {
                execution: Arc::clone(&execution),
                store: Arc::clone(&services.agent_store),
                // The **existing** supervisor, cloned from the application rather than minted.
                // `RunSupervisor` is a handle over one shared record, so the supplied
                // application and the existing one supervise exactly the same runs.
                supervisor: services
                    .agent
                    .get()
                    .map_or_else(agent_supervision::RunSupervisor::new, |agent| {
                        agent.supervisor().clone()
                    }),
            })
        })));
    let rebuilder = Arc::new(RuntimeDependentRebuilder {
        slots: service_rebuild::LiveServices {
            commands: Arc::clone(&services.commands),
            agent_execution: Arc::clone(&services.agent_execution),
            agent: Arc::clone(&services.agent),
            p04: Arc::clone(&services.p04_slot),
            p05: Arc::clone(&services.p05_slot),
            p03: Arc::clone(&services.p03_slot),
            sidecars: Arc::clone(&services.security_sidecars),
            claims: Arc::new(service_rebuild::ConstructionClaims::new()),
        },
        dependencies: Arc::clone(&services.p03_dependencies),
        agent_owners,
        agent_store: Arc::clone(&services.agent_store),
        activities: Arc::clone(&services.activities),
        broker_http: Arc::clone(&services.broker_http),
        broker: Arc::clone(&services.broker),
        release: Arc::clone(&services.release),
        startup_ledger,
        sync: services.sync.clone(),
        settings: Arc::clone(&services._settings),
        extra_tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
    });
    services.config_recovery = services.config_recovery.clone_entry().with_rebuilder(rebuilder);
}
