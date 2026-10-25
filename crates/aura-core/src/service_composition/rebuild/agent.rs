use crate::*;

impl RuntimeDependentRebuilder {
    /// The process's Agent-side owners, installing them if the process never built them.
    ///
    /// The execution layer is what the command backend, the Agent
    /// application and every tool call are built on, and a damaged `tools.json` means the
    /// startup path refused it — so there was nothing for a supplied backend to attach to and
    /// the capability could not come back without a restart.
    ///
    /// This creates the **one** execution layer the process never got, over the process's
    /// existing Agent Store, broker, persistence, sidecar readiness and activity owners. An
    /// already-present set is returned unchanged: an occupant is never replaced, so an
    /// in-flight run keeps the exact task, owner and cancel path it has.
    pub(crate) fn ensure_agent_owners(
        &self,
    ) -> std::result::Result<Arc<AgentLiveOwners>, service_rebuild::ConstructionRefusal> {
        use service_rebuild::ConstructionRefusal as Refusal;
        let Ok(mut slot) = self.agent_owners.lock() else {
            return Err(Refusal::Failed("the Agent owner set could not be read".to_owned()));
        };
        if let Some(present) = slot.as_ref() {
            return Ok(Arc::clone(present));
        }
        // The execution layer binds the workspace runtime's fs tool service, so without that
        // runtime there is nothing to build it on and the real dependency is named.
        let Some(p03) = self.slots.p03.get() else {
            return Err(Refusal::DependencyMissing(
                service_rebuild::ServiceDependency::WorkspaceFiles,
            ));
        };
        // It hands the process's sidecar readiness store to the sandbox host it will use, so
        // the store must exist first.
        if self.ensure_sidecars()?.is_none() {
            return Err(Refusal::DependencyMissing(
                service_rebuild::ServiceDependency::SecuritySidecars,
            ));
        }
        let execution = agent_execution::AgentExecution::new_with_global_concurrency(
            &p03,
            Arc::clone(&self.dependencies.persistence),
            Arc::clone(&self.broker),
            Arc::clone(&self.agent_store),
            Arc::new(controlled_web::ControlledWeb::new(
                Arc::clone(&self.dependencies.persistence),
                Arc::clone(&self.broker),
                Arc::clone(&self.broker_http),
                self.activities.as_ref().clone(),
                Arc::clone(&self.agent_store),
            )),
            self.activities.as_ref().clone(),
            self.dependencies.activity_index.clone(),
            self.dependencies.runtime_instance_id,
            self.dependencies.max_concurrent_tool_calls,
        )
        .map_err(|error| Refusal::Failed(error.to_string()))?;
        let owners = Arc::new(AgentLiveOwners {
            execution: Arc::new(execution),
            store: Arc::clone(&self.agent_store),
            // A process that never built an Agent application has no supervisor, so the one it
            // gets is created here — exactly once, because the caller holds this capability's
            // construction responsibility and this slot is filled before it is released.
            supervisor: agent_supervision::RunSupervisor::new(),
        });
        owners.execution.control_resources.register_runtime(self.settings.paths(), &self.release);
        // A security authority supplied **before** this owner set existed could not bind the
        // turn journal. It is bound here, which is the same binding startup makes and the only
        // other order the supply chain can take.
        if let Some(p04) = self.slots.p04.get() {
            p04.bind_turn_journal(Arc::clone(&owners.execution.journal));
        }
        self.dependencies
            .persistence
            .bind_tool_credential_scope_owner(owners.store.clone())
            .map_err(|error| Refusal::Failed(error.to_string()))?;
        // The runtime's own slot is filled with the same layer every other consumer reads, so
        // the supplied execution layer is the one requests use rather than a private copy.
        if !self.slots.agent_execution.publish_if_vacant(Arc::clone(&owners.execution)) {
            // The owner slot is still locked by this call, so acquiring it again would
            // deadlock. A conflicting execution slot is not evidence that these newly
            // constructed owners belong to its occupant; leave the occupant intact and
            // report the conflict instead of publishing an unrelated supervisor.
            return Err(Refusal::SuppliedConcurrently);
        }
        *slot = Some(Arc::clone(&owners));
        Ok(owners)
    }

    /// Supplies the Agent application **without** repeating startup and **without** a second
    /// supervisor.
    pub(crate) fn supply_agent(
        &self,
    ) -> std::result::Result<(), service_rebuild::ConstructionRefusal> {
        use service_rebuild::ConstructionRefusal as Refusal;
        let owners = self.ensure_agent_owners()?;
        // The Agent application needs a real model authority and a command backend: the
        // execution layer writes through the latter and reads the former. They are supplied
        // **as dependencies** rather than assumed present, because a capability the repair did
        // not name may still be missing — and a chain that is only partly rebuilt must not be
        // reported as a working tool policy.
        self.supply_model_authority()?;
        let Some(p05) = self.slots.p05.get() else {
            return Err(Refusal::DependencyMissing(
                service_rebuild::ServiceDependency::ModelAuthority,
            ));
        };
        let Some(commands) = self.slots.commands.get() else {
            return Err(Refusal::DependencyMissing(
                service_rebuild::ServiceDependency::CommandBackend,
            ));
        };
        let outcome = self.slots.claims.agent.with_responsibility(
            || self.slots.agent.is_present(),
            || {
                agent_application::AgentApplication::open_from_existing_owners(
                    Arc::clone(&owners.execution),
                    commands,
                    (*p05).clone(),
                    Arc::clone(&self.dependencies.settings),
                    Arc::clone(&owners.store),
                    owners.supervisor.clone(),
                )
            },
            |result| {
                let agent = result.map_err(|error| Refusal::Failed(error.to_string()))?;
                let agent = Arc::new(agent);
                self.discharge_agent_one_time_steps(&agent)?;
                if self.slots.agent.publish_if_vacant(Arc::clone(&agent)) {
                    // The Agent's one-time responsibilities.
                    // `open_from_existing_owners` opens no run and scans nothing — which is right
                    // when startup already did it, and wrong when startup never had an Agent
                    // application to build. The ledger decides which case this is.
                    Ok(())
                } else {
                    Err(Refusal::SuppliedConcurrently)
                }
            },
        );
        claim_refusal(outcome)
    }
}
