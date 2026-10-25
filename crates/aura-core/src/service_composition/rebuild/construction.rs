use crate::*;

impl service_rebuild::ServiceConstruction for RuntimeDependentRebuilder {
    fn is_available(&self, capability: ConfigDependentCapability) -> bool {
        match capability {
            // The tool policy is served by the workspace runtime, the command backend **and**
            // the Agent application; all three must be present, because the execution layer
            // writes through the second and the registry path reads the third, and both are
            // built on the first.
            ConfigDependentCapability::ToolPolicy => {
                self.slots.p03.is_present()
                    && self.slots.commands.is_present()
                    && self.slots.agent.is_present()
            }
            ConfigDependentCapability::SecurityAuthority => self.slots.p04.is_present(),
            // The model authority is available when its own
            // service is present **and** its real consumer has the chain it needs. Reporting it
            // available on the strength of P05 alone would claim a capability the product cannot
            // exercise — exactly the "cleared record read as working service" confusion this round
            // exists to remove. The Agent application is that consumer, so when the process has the
            // rest of the Agent chain the Agent must be present too.
            ConfigDependentCapability::ModelAuthority => {
                self.slots.p05.is_present() && self.slots.agent.is_present()
            }
            // Convergence is served by the startup judgement, not by a service slot.
            ConfigDependentCapability::ConfigTransactionRecovery => false,
        }
    }

    fn supply(
        &self,
        capability: ConfigDependentCapability,
    ) -> std::result::Result<(), service_rebuild::ConstructionRefusal> {
        match capability {
            ConfigDependentCapability::ToolPolicy => {
                // The chain is supplied **in dependency order** and every step is checked, so a
                // partially supplied chain is never reported as a working tool policy. Order:
                // the workspace runtime, the security authority built on it, the command backend,
                // then the Agent application, which needs the backend and the model authority.
                //
                // The security authority is supplied here as a **dependency** even though it is
                // its own capability: the startup path refused P03 for a damaged `tools.json`,
                // and P04 is built on P03 — so a tool-policy repair that left P04 missing would
                // restore the tool policy while every permission-gated operation still failed.
                self.supply_workspace()?;
                self.supply_security_authority()?;
                self.supply_commands()?;
                self.supply_agent()
            }
            ConfigDependentCapability::SecurityAuthority => {
                self.supply_workspace()?;
                self.supply_security_authority()
            }
            // The model authority is a **dependency** of the
            // Agent chain, not a leaf.
            //
            // A damaged `models.json` at startup means no P05, and with no model authority the Agent
            // application was never created either — `open_dependent_services` builds the Agent only
            // from `(commands, p05)`. Supplying P05 alone therefore left the process with an
            // authority and no consumer, so the ordinary Agent call that reads it still failed and
            // the capability was reported available while nothing could use it.
            //
            // The chain is supplied in dependency order and every step is checked, so a partially
            // supplied chain is never reported as working. The workspace runtime and the command
            // backend come first because the Agent application is built on both.
            ConfigDependentCapability::ModelAuthority => {
                self.supply_model_authority()?;
                // The Agent application is the model authority's real consumer. It is supplied only
                // when the process has the rest of its chain; otherwise the genuine missing
                // dependency is reported rather than a half-built Agent.
                if self.slots.p03.is_present() && self.slots.commands.is_present() {
                    self.supply_agent()?;
                }
                Ok(())
            }
            ConfigDependentCapability::ConfigTransactionRecovery => Ok(()),
        }
    }
}

impl config_recovery::DependentServiceRebuilder for RuntimeDependentRebuilder {
    fn is_available(&self, capability: ConfigDependentCapability) -> bool {
        <Self as service_rebuild::ServiceConstruction>::is_available(self, capability)
    }

    /// Supplies every missing capability the repaired domains feed.
    ///
    /// The capabilities are computed from the **repaired domain
    /// set** once, and each is supplied in dependency order, so the caller gets one coherent
    /// answer rather than a per-domain loop that could publish a half-ready service.
    ///
    /// # Why this returns `Ok` even when nothing could be supplied
    ///
    /// A supply failure is a **capability** fact, not a failed recovery attempt: the
    /// configuration really was repaired. Returning `Err` for it made the caller lose the
    /// per-capability reasons and report a whole-attempt failure, which is a different and less
    /// true statement. The reasons therefore travel in this call's `still_missing`, and
    /// `Err` is reserved for the case where the rebuild could not even be attempted.
    fn rebuild_missing(
        &self,
        repaired_domains: &[ConfigModule],
    ) -> std::result::Result<
        config_recovery::CapabilityRebuildOutcome,
        config_recovery::CapabilityRebuildError,
    > {
        use config_recovery::CapabilityRebuildOutcome;
        use service_rebuild::ServiceConstruction;

        let affected = affected_capabilities(repaired_domains);
        let mut outcome = CapabilityRebuildOutcome::default();
        let mut failures = Vec::new();
        for capability in &affected {
            if !is_slot_backed(*capability) {
                // Convergence is served by the startup judgement, which is not a service a
                // live supply can create. It is reported as its own fact rather than being
                // silently dropped or claimed as available.
                continue;
            }
            if let Err(refusal) = self.supply(*capability) {
                failures.push((*capability, refusal.reason()));
            }
        }
        for capability in &affected {
            if ServiceConstruction::is_available(self, *capability) {
                outcome.available.push(*capability);
            } else if let Some((_, reason)) =
                failures.iter().find(|(candidate, _)| candidate == capability)
            {
                outcome.still_missing.push((*capability, reason.clone()));
            } else if is_slot_backed(*capability) {
                outcome.still_missing.push((
                    *capability,
                    "its service is still absent, so this capability is not available".to_owned(),
                ));
            }
        }
        Ok(outcome)
    }
}

/// The capabilities a repaired set of domains really feeds.
///
/// Computed once from the whole repaired set. A per-domain loop
/// would rebuild the same capability repeatedly and could publish a service while another
/// domain's fault for the same capability was still recorded.
pub(crate) fn affected_capabilities(
    repaired_domains: &[ConfigModule],
) -> Vec<ConfigDependentCapability> {
    ConfigDependentCapability::ALL
        .into_iter()
        .filter(|capability| {
            repaired_domains.iter().any(|module| capability.domains().contains(module))
        })
        .collect()
}
