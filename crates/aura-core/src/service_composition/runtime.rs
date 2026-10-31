use crate::*;

impl RuntimeServices {
    pub(crate) fn p03(&self) -> Result<Arc<P03Services>> {
        self.p03_slot.get().ok_or_else(|| MissingCapability::P03WorkspaceFiles.refusal())
    }

    /// The security authority, read from the **one** slot the recovery publishes into.
    ///
    /// This used to read a plain `Option` while the rebuilder
    /// inspected a different slot, so a live supply was invisible to every ordinary request.
    pub(crate) fn p04(&self) -> Result<Arc<P04Services>> {
        self.p04_slot.get().ok_or_else(|| MissingCapability::P04SecurityAuthority.refusal())
    }

    /// The model authority, likewise read from the slot a supply publishes into.
    pub(crate) fn p05(&self) -> Result<Arc<P05Services>> {
        self.p05_slot.get().ok_or_else(|| MissingCapability::P05ModelAuthority.refusal())
    }

    pub(crate) fn agent(&self) -> Result<Arc<agent_application::AgentApplication>> {
        self.agent.get().ok_or_else(|| MissingCapability::AgentExecution.refusal())
    }

    pub(crate) fn commands(&self) -> Result<Arc<command_execution::CommandExecution>> {
        self.commands.get().ok_or_else(|| MissingCapability::CommandExecution.refusal())
    }

    pub(crate) fn security_sidecars(&self) -> Result<Arc<sidecars::SecuritySidecars>> {
        self.security_sidecars
            .lock()
            .ok()
            .and_then(|set| set.clone())
            .ok_or_else(|| MissingCapability::SecuritySidecars.refusal())
    }

    /// Whether the process can execute Agent work at all.
    ///
    /// A caller that must distinguish "refused because configuration is damaged"
    /// from "refused for its own reason" asks this rather than inspecting fields.
    /// It reads the **same** degradation registry the recovery entry reads, so the
    /// two cannot disagree about whether Agent work is available.
    ///
    /// This now consults **every** capability Agent work really
    /// depends on — tool policy, security authority and cross-domain transaction
    /// recovery — rather than the tool policy alone. A measurement showed that the narrower
    /// answer admitted new work while an unresolved bundle's scope was unknown.
    #[must_use]
    pub(crate) fn agent_capability_available(&self) -> bool {
        self.config_recovery.agent_work_available()
    }

    /// The capability blocking new Agent work, and its reason.
    ///
    /// The routes use this to refuse **new work** with the real
    /// reason while leaving view/stop/settle/diagnose reachable. It reads the same
    /// registry as [`Self::agent_capability_available`], so the two cannot disagree.
    #[must_use]
    pub(crate) fn agent_work_blocker(
        &self,
    ) -> Option<(aura_core_base::config_degradation::ConfigDependentCapability, String)> {
        self.config_recovery.agent_work_blocker()
    }

    /// The user's configuration recovery entry, built on this process's own
    /// settings facade and degradation registry.
    #[must_use]
    pub(crate) fn config_recovery(&self) -> config_recovery::ConfigRecoveryEntry {
        self.config_recovery.clone_entry()
    }

    /// The one-time startup work this process has **not** really finished, by stable key.
    ///
    /// This is the honest answer to "did the startup work
    /// happen?", which the live constructors previously assumed. A caller — a route, a diagnostic,
    /// or a test — can therefore observe the difference between "already done, do not repeat" and
    /// "never reached, must be discharged" rather than inferring it from which services exist.
    #[must_use]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn outstanding_startup_steps(&self) -> Vec<&'static str> {
        self.startup_ledger.outstanding().into_iter().map(|step| step.key()).collect()
    }
}

/// Test-only access to the dependent chain, which a test **requires**.
///
/// These fields are `Option` so a damaged configuration domain degrades a
/// capability instead of the whole runtime. Existing integration tests exercise
/// the full chain and therefore genuinely need it: a test that reaches into
/// `services.p04` cannot run against a runtime whose P04 was refused.
///
/// These accessors say that explicitly and fail loudly with a diagnostic naming
/// the missing capability, rather than each of ~30 test modules individually
/// re-deriving it. They are `#[cfg(test)]`-gated, so production code cannot use
/// them and cannot accidentally convert a degraded capability into a panic on a
/// real control path.
#[cfg(test)]
impl RuntimeServices {
    pub(crate) fn p03_required(&self) -> Arc<P03Services> {
        self.p03_slot.get().expect("this test requires the P03 workspace-files capability")
    }

    pub(crate) fn p04_required(&self) -> Arc<P04Services> {
        self.p04_slot.get().expect("this test requires the P04 security authority")
    }

    pub(crate) fn p05_required(&self) -> Arc<P05Services> {
        self.p05_slot.get().expect("this test requires the P05 model authority")
    }

    pub(crate) fn agent_required(&self) -> Arc<agent_application::AgentApplication> {
        self.agent.get().expect("this test requires the Agent application")
    }

    pub(crate) fn commands_required(&self) -> Arc<command_execution::CommandExecution> {
        self.commands.get().expect("this test requires the command execution layer")
    }

    pub(crate) fn security_sidecars_required(&self) -> Arc<sidecars::SecuritySidecars> {
        self.security_sidecars
            .lock()
            .expect("the sidecar set lock is usable")
            .clone()
            .expect("this test requires the security sidecars")
    }

    #[cfg(test)]
    pub(crate) fn agent_execution_required(&self) -> Arc<agent_execution::AgentExecution> {
        self.agent_execution.get().expect("this test requires the Agent execution layer")
    }
}

impl Drop for RuntimeServices {
    fn drop(&mut self) {
        // Each half is shut down only if it was really built. An absent
        // dependent chain has nothing to shut down, and shutting down an
        // unrelated owner here would be a second, silent failure.
        if let Some(agent) = self.agent.get() {
            agent.shutdown();
        }
        if let Some(commands) = self.commands.get() {
            commands.shutdown();
        }
        // A running package Conformance is cancelled; its Job ends and its terminal is
        // written by its own worker. Startup recovery settles anything that cannot be.
        if let Some(packages) = &self.packages {
            packages.shutdown();
        }
        // Every MCP server's process tree ends with the runtime.
        self.mcp.shutdown();
    }
}

impl Drop for RuntimeBackgroundTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
