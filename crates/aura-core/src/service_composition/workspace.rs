//! Live workspace construction reuses the process authority store.
use crate::*;

impl RuntimeDependentRebuilder {
    /// Supplies the workspace runtime, reusing this process's existing owners.
    ///
    /// This is the **root** of the dependent chain. `tools.json`
    /// sizes the workspace runtime, so a damaged `tools.json` refuses P03 — and P03 is what P04
    /// and the command backend are built on. Without supplying P03, repairing `tools.json` could
    /// bring back none of them, which is exactly the "a restart is required" answer the brief
    /// forbids.
    ///
    /// It runs no startup reconciliation: `P03Services::open` reads the repaired configuration
    /// and opens the workspace runtime on the process's existing persistence. Its blocking pool
    /// is its own by construction at startup too, so this is the same shape the startup path
    /// produces rather than a second owner of anything process-wide.
    pub(crate) fn supply_workspace(
        &self,
    ) -> std::result::Result<(), service_rebuild::ConstructionRefusal> {
        use service_rebuild::ConstructionRefusal as Refusal;
        let outcome = self.slots.claims.workspace_files.with_responsibility(
            || self.slots.p03.is_present(),
            || {
                P03Services::open(
                    Arc::clone(&self.dependencies.persistence),
                    Arc::clone(&self.broker),
                    &self.dependencies.settings,
                    self.dependencies.activity_index.clone(),
                    self.dependencies.activity_observer.clone(),
                    self.dependencies.runtime_instance_id,
                )
            },
            |result| {
                let services = result.map_err(|error| Refusal::Failed(error.to_string()))?;
                if self.slots.p03.publish_if_vacant(Arc::new(services)) {
                    Ok(())
                } else {
                    Err(Refusal::SuppliedConcurrently)
                }
            },
        );
        claim_refusal(outcome)
    }
}
