use crate::*;

impl RuntimeDependentRebuilder {
    /// Installs the process's sidecar set if it never got one, and returns it.
    ///
    /// The sidecar set is **deferred** construction: at startup it
    /// starts no process, and each sidecar is launched on first use. A process whose
    /// `tools.json` was damaged never got a set at all, which is why repairing that domain could
    /// not bring the command backend back. Installing the one set here is not a second sidecar
    /// host: it is the set the process was missing, on the process's existing options.
    ///
    /// `Ok(None)` means the set genuinely cannot exist in this process (no options were
    /// retained), and the caller names that as the missing dependency.
    pub(crate) fn ensure_sidecars(
        &self,
    ) -> std::result::Result<
        Option<Arc<sidecars::SecuritySidecars>>,
        service_rebuild::ConstructionRefusal,
    > {
        let Ok(mut slot) = self.slots.sidecars.lock() else {
            return Err(service_rebuild::ConstructionRefusal::Failed(
                "the sidecar set could not be read".to_owned(),
            ));
        };
        if let Some(present) = slot.as_ref() {
            return Ok(Some(Arc::clone(present)));
        }
        let Some(facts) = self.dependencies.sidecar_facts.as_ref() else {
            return Ok(None);
        };
        // `SecuritySidecars::deferred` starts the sidecar supervisor tasks with `tokio::spawn`,
        // so it must run with a Tokio reactor **in context**. A recovery runs on the bounded
        // **synchronous** boundary — a blocking thread, which has no reactor — so the
        // construction is entered through the process's own runtime handle.
        //
        // `Handle::enter` is the correct primitive and not `block_on`: the constructor only
        // *spawns* tasks and returns, it does not await anything. `block_on` would additionally
        // fail when the caller already sits on a runtime thread, which the direct call path
        // does, and blocking a worker there is exactly the stall the bounded boundary exists to
        // avoid. The handle is the process's own runtime, which is the one that owns and shuts
        // down every other long-lived Core task.
        let Some(handle) = self.dependencies.runtime_handle.as_ref() else {
            return Err(service_rebuild::ConstructionRefusal::Failed(
                "this process has no runtime handle, so the sidecar set cannot be started"
                    .to_owned(),
            ));
        };
        let _guard = handle.enter();
        let set = Arc::new(sidecars::SecuritySidecars::deferred(&facts.options, &facts.release));
        *slot = Some(Arc::clone(&set));
        Ok(Some(set))
    }

    /// Supplies the security authority, reusing this process's existing owners.
    ///
    /// It is built on the workspace runtime: P04's filesystem capability is the workspace's own
    /// `FsToolService` and `WorkspaceRuntime`, so a missing workspace runtime is named as the
    /// real dependency rather than reported as a generic construction failure.
    pub(crate) fn supply_security_authority(
        &self,
    ) -> std::result::Result<(), service_rebuild::ConstructionRefusal> {
        use service_rebuild::ConstructionRefusal as Refusal;
        let Some(p03) = self.slots.p03.get() else {
            return Err(Refusal::DependencyMissing(
                service_rebuild::ServiceDependency::WorkspaceFiles,
            ));
        };
        let outcome = self.slots.claims.security_authority.with_responsibility(
            || self.slots.p04.is_present(),
            || {
                P04Services::open(
                    Arc::clone(&self.dependencies.persistence),
                    Arc::clone(&p03.fs),
                    Arc::clone(&p03.workspace),
                    &self.dependencies.settings,
                    ElevatedBrokerOptions {
                        helper_executable: self.release.elevated_helper_executable.clone(),
                        helper_content_hash: self.release.elevated_helper_hash.clone(),
                        component_manifest_path: self.release.manifest_path.clone(),
                        release_descriptor: self.release.release_descriptor.clone(),
                        runtime_instance_id: self.dependencies.runtime_instance_id,
                    },
                    self.dependencies.runtime_instance_id,
                    p03.publication_pool(),
                    self.dependencies.activity_index.clone(),
                    self.dependencies.activity_observer.clone(),
                )
            },
            |result| {
                let services = result.map_err(|error| Refusal::Failed(error.to_string()))?;
                // The turn journal is bound exactly as startup binds it, so the supplied
                // authority reads the same journal as the execution layer. The owner set may not
                // exist yet — this supply is itself a dependency of building it — so the binding
                // is made when it is available and, in the other order, by
                // `ensure_agent_owners` once it builds the execution layer.
                if let Ok(owners) = self.agent_owners.lock()
                    && let Some(owners) = owners.as_ref()
                {
                    services.bind_turn_journal(Arc::clone(&owners.execution.journal));
                }
                if self.slots.p04.publish_if_vacant(Arc::new(services)) {
                    Ok(())
                } else {
                    Err(Refusal::SuppliedConcurrently)
                }
            },
        );
        claim_refusal(outcome)
    }
}
