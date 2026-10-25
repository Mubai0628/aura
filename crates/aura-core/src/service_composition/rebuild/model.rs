use crate::*;

impl RuntimeDependentRebuilder {
    /// Supplies the model authority, reusing this process's existing owners.
    ///
    /// It calls the **live** constructor, which opens no journal reconciliation: the startup
    /// half already settled the rows a previous process left, and repeating it against a live
    /// runtime would be a second reconciliation of the same rows.
    pub(crate) fn supply_model_authority(
        &self,
    ) -> std::result::Result<(), service_rebuild::ConstructionRefusal> {
        use service_rebuild::ConstructionRefusal as Refusal;
        let outcome = self.slots.claims.model_authority.with_responsibility(
            || self.slots.p05.is_present(),
            || {
                P05Services::open_live(
                    Arc::clone(&self.dependencies.persistence),
                    Arc::clone(&self.dependencies.settings),
                    self.release.release_descriptor.bundle_namespace.clone(),
                    self.dependencies.sync.clone(),
                )
            },
            |result| {
                let services = result.map_err(|error| Refusal::Failed(error.to_string()))?;
                // Complete first-recovery responsibilities before publishing a usable slot.
                self.discharge_model_one_time_steps(&services)?;
                if self.slots.p05.publish_if_vacant(Arc::new(services.clone())) {
                    // The model authority's **own maintenance**
                    // and the steps its absence prevented. They run only after the service is
                    // really published, and only if the startup path never reached them.
                    Ok(())
                } else {
                    Err(Refusal::SuppliedConcurrently)
                }
            },
        );
        claim_refusal(outcome)
    }
}
