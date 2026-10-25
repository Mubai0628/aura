use crate::*;

impl RuntimeDependentRebuilder {
    /// Supplies the command backend, reusing this process's existing owners.
    pub(crate) fn supply_commands(
        &self,
    ) -> std::result::Result<(), service_rebuild::ConstructionRefusal> {
        use service_rebuild::ConstructionRefusal as Refusal;
        // The command backend is built on the workspace runtime, so a missing workspace runtime
        // is the real dependency to name rather than a generic construction failure.
        if !self.slots.p03.is_present() {
            return Err(Refusal::DependencyMissing(
                service_rebuild::ServiceDependency::WorkspaceFiles,
            ));
        }
        // The sidecar set supplies the readiness store the command backend publishes into.
        // Installing it here (rather than only at startup) is what lets a repaired `tools.json`
        // bring the backend back; the refusal names the real dependency when it cannot.
        let Some(sidecars) = self.ensure_sidecars()? else {
            return Err(Refusal::DependencyMissing(
                service_rebuild::ServiceDependency::SecuritySidecars,
            ));
        };
        let readiness = sidecars.readiness.clone();
        let owners = self.ensure_agent_owners()?;
        let store = Arc::clone(&owners.store);
        let outcome = self.slots.claims.commands.with_responsibility(
            || self.slots.commands.is_present(),
            || {
                command_execution::CommandExecution::open(
                    Arc::clone(&self.dependencies.persistence),
                    Arc::clone(&self.broker),
                    store,
                    self.dependencies.activity_index.clone(),
                    self.dependencies.runtime_instance_id,
                    &readiness,
                    &self.release,
                )
            },
            |result| {
                let commands = result.map_err(|error| Refusal::Failed(error.to_string()))?;
                if self.slots.commands.publish_if_vacant(Arc::new(commands)) {
                    Ok(())
                } else {
                    Err(Refusal::SuppliedConcurrently)
                }
            },
        );
        claim_refusal(outcome)
    }
}
