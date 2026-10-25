//! The reachable user entry for recovering a damaged configuration.
//!
//! # Why this exists at the Core level
//!
//! `aura-config` owns the recovery *mechanics* (`plan_domain_recovery` /
//! `apply_domain_recovery`), and its own tests prove them. A mechanic is not an
//! **entry**: R2 gave Core a `ConfigDegradationRegistry` recording which
//! capability was lost, and until something consumes it the registry is a record
//! nobody reads. This module is that consumer, and it is what makes the flow
//! reachable from the product rather than only from a test.
//!
//! # What it produces
//!
//! * a **fault notice** naming the affected configuration, the new calls that are
//!   suspended, and the actionable steps — the "故障提示" the requirement asks for;
//! * the **preview** for a chosen scope, before the user triggers anything;
//! * the **apply**, and the registry update that follows a real recovery.
//!
//! # What it never does
//!
//! * delete credentials, sessions or workspaces;
//! * clear the unresolved records R2 protects;
//! * reset configuration the user did not scope in;
//! * report a recovery that did not happen.

mod admission;

pub use aura_config::recovery_contract::{
    ConfigRecoveryApplyRequest, ConfigRecoveryPlanRequest, RecoveryNotice, RecoveryReport,
    UnavailableCapability,
};

use aura_config::{
    ConfigModule, SettingsFacade, SettingsFacadeError,
    recovery::{BundleRecoveryPlan, ConfigRecoveryAction, ConfigRecoveryError, DomainRecoveryPlan},
};

use crate::config_degradation::{ConfigDegradationRegistry, ConfigDependentCapability};

/// Builds path-free notices from actual configuration and service availability.
struct RecoveryNoticeBuilder;

impl RecoveryNoticeBuilder {
    /// The notice for a runtime whose degradation registry is in this state.
    ///
    /// `scope_known == false` yields an explicitly unknown scope: the requirement is
    /// that an unknown scope must not be presented as "no configuration is
    /// affected", and must not silently reset all configuration either.
    #[must_use]
    pub fn from_registry(registry: &ConfigDegradationRegistry) -> RecoveryNotice {
        let degraded = registry.all();
        let scope_known = !registry.recovery_scope_unknown();
        let mut affected_domains = degraded
            .iter()
            .filter_map(|entry| entry.domain)
            .map(|module| module.key().to_owned())
            .collect::<Vec<_>>();
        affected_domains.sort();
        affected_domains.dedup();
        let mut unavailable_capabilities =
            degraded.iter().map(|entry| entry.capability.key().to_owned()).collect::<Vec<_>>();
        unavailable_capabilities.sort();
        unavailable_capabilities.dedup();
        let mut fault_codes =
            degraded.iter().map(|entry| entry.fault_code.to_owned()).collect::<Vec<_>>();
        fault_codes.sort();
        fault_codes.dedup();

        // What is suspended, in bounded terms. A degraded capability suspends the
        // calls that depend on it; everything else, plus status, stop and repair,
        // stays usable.
        let mut suspended_calls = Vec::new();
        for entry in &degraded {
            match entry.capability {
                ConfigDependentCapability::ToolPolicy => {
                    suspended_calls.push("new tool execution".to_owned());
                }
                ConfigDependentCapability::SecurityAuthority => {
                    suspended_calls.push("new permission-gated operations".to_owned());
                }
                ConfigDependentCapability::ModelAuthority => {
                    suspended_calls.push("new model calls".to_owned());
                }
                ConfigDependentCapability::ConfigTransactionRecovery => {
                    suspended_calls
                        .push("new calls that depend on the suspect configuration".to_owned());
                    if !entry.scope_known {
                        suspended_calls.push(
                            "every configuration-dependent new call, because the affected set \
                             could not be established"
                                .to_owned(),
                        );
                    }
                }
            }
        }
        suspended_calls.sort();
        suspended_calls.dedup();

        let mut available_actions = vec![
            ConfigRecoveryAction::Retry.key().to_owned(),
            ConfigRecoveryAction::ResetToDefault.key().to_owned(),
        ];
        if scope_known {
            available_actions.push(ConfigRecoveryAction::RestorePreviousValid.key().to_owned());
        }
        if degraded.is_empty() {
            available_actions.clear();
        }
        available_actions.sort();

        let next_step = if degraded.is_empty() {
            "No configuration capability is unavailable.".to_owned()
        } else if scope_known {
            "Open configuration recovery to preview the affected scope, then choose retry, \
             restore the previous valid configuration, or reset the affected domains to their \
             validated defaults."
                .to_owned()
        } else {
            "The affected configuration could not be narrowed down. Review the complete \
             configuration reset scope and loss before explicit confirmation; the runtime \
             will never reset configuration without that confirmation."
                .to_owned()
        };

        RecoveryNotice {
            affected_domains,
            unavailable_capabilities,
            fault_codes,
            suspended_calls,
            available_actions,
            scope_known,
            recovery_in_progress: false,
            next_step,
        }
    }
}

/// The Core-side recovery entry.
///
/// It holds the two owners it needs — the settings facade that performs the
/// recovery, and the degradation registry that records what it means — and it
/// **updates the registry after a real recovery**, which is what closes the loop:
/// a repaired capability must stop being reported as unavailable, or the notice
/// would keep telling the user to fix something that is already fixed.
pub struct ConfigRecoveryEntry {
    settings: std::sync::Arc<SettingsFacade>,
    degradation: crate::config_degradation::SharedConfigDegradation,
    /// The live runtime's way to fill missing dependent services, when one was supplied.
    ///
    /// `None` in a process that has no dependent services to rebuild (and in tests that
    /// only exercise the configuration half). It is `Option` rather than a required field
    /// so the entry stays constructible without a runtime, but a **real** recovery path
    /// supplies it — otherwise a capability could still be reported restored from a marker
    /// alone, which is the defect this exists to remove.
    rebuilder: Option<std::sync::Arc<dyn DependentServiceRebuilder>>,
    admission: std::sync::Arc<admission::ApplyAdmission>,
}

impl Clone for ConfigRecoveryEntry {
    fn clone(&self) -> Self {
        Self {
            settings: std::sync::Arc::clone(&self.settings),
            degradation: std::sync::Arc::clone(&self.degradation),
            // The rebuilder is shared, not duplicated: it refers to the live runtime's
            // own slots, and a second rebuilder would be a second owner of them.
            rebuilder: self.rebuilder.as_ref().map(std::sync::Arc::clone),
            admission: std::sync::Arc::clone(&self.admission),
        }
    }
}

impl ConfigRecoveryEntry {
    #[must_use]
    pub fn new(
        settings: std::sync::Arc<SettingsFacade>,
        degradation: crate::config_degradation::SharedConfigDegradation,
    ) -> Self {
        let admission = admission::ApplyAdmission::for_owner(&settings);
        Self { settings, degradation, rebuilder: None, admission }
    }

    /// Await overlapping recovery/supply before taking a general blocking permit.
    /// The guard moves into the admitted write so a departed caller cannot release it early.
    pub(crate) async fn admit_apply(
        &self,
        modules: &[ConfigModule],
    ) -> Result<admission::ApplyGuard, &'static str> {
        let mut keys: std::collections::BTreeSet<String> =
            modules.iter().map(|m| format!("domain/{}", m.key())).collect();
        if let Some(rebuilder) = &self.rebuilder {
            for capability in self.capabilities_for_domains(modules) {
                if !rebuilder.is_available(capability)
                    && capability != ConfigDependentCapability::ConfigTransactionRecovery
                {
                    keys.insert(format!("capability/{}", capability.key()));
                    // These missing chains share the one Agent consumer; its construction cannot
                    // become four blocking callers waiting on the same responsibility.
                    keys.insert("consumer/agent".to_owned());
                }
            }
        }
        self.admission.claim(keys).await
    }

    /// Supplies the live runtime's dependent-service rebuilder.
    ///
    /// Without this a recovery can clear a degradation
    /// marker and report a capability restored while its service slot is still absent.
    /// The runtime that owns the slots attaches itself here, so the recovery reports
    /// capability restoration from the **slot** rather than from the record.
    #[must_use]
    pub fn with_rebuilder(
        mut self,
        rebuilder: std::sync::Arc<dyn DependentServiceRebuilder>,
    ) -> Self {
        self.rebuilder = Some(rebuilder);
        self
    }

    /// A copy of this entry, for a caller that owns its own `Arc`s.
    #[must_use]
    pub fn clone_entry(&self) -> Self {
        self.clone()
    }

    /// The fault notice for this runtime's current configuration state.
    #[must_use]
    pub fn notice(&self) -> RecoveryNotice {
        let mut notice = RecoveryNoticeBuilder::from_registry(&self.degradation);
        // A healthy Models file cannot supply P05 when the actual dependent chain
        // was never built. Bootstrap needs the
        // real capability set, while the affected domains remain the actual cause.
        for capability in ConfigDependentCapability::ALL {
            if !self.capability_is_usable(capability)
                && !notice.unavailable_capabilities.iter().any(|key| key == capability.key())
            {
                notice.unavailable_capabilities.push(capability.key().to_owned());
                notice.fault_codes.push(capability.unavailable_code().to_owned());
                notice.suspended_calls.push(format!(
                    "new calls that depend on the unavailable {} service",
                    capability.key()
                ));
            }
        }
        notice.unavailable_capabilities.sort();
        notice.fault_codes.sort();
        notice.fault_codes.dedup();
        notice
    }

    /// The preview for recovering a **scoped set** of domains as one version.
    pub fn plan_bundle(
        &self,
        modules: &[ConfigModule],
    ) -> Result<BundleRecoveryPlan, ConfigRecoveryError> {
        self.settings.plan_bundle_recovery(modules)
    }

    /// Plans a single domain and checks it still matches the preview the user saw.
    ///
    /// The wire apply used to plan from the
    /// request and ignore the preview, so what the user confirmed was never what was
    /// written. This is the check that closes that gap: the caller passes the
    /// identity from the preview it showed, and a mismatch is refused as
    /// `config.recovery.preview_stale` **before** any write.
    ///
    /// `preview_id` is now **required**. It used to be an
    /// `Option` whose `None` meant "apply the current state", which was a documented bypass of
    /// the whole confirmation: a caller that simply omitted the field wrote configuration that
    /// no user had seen. The confirmed rule is that the actual entry binds the preview, so a
    /// missing identity is refused exactly like a stale one.
    pub fn plan_domain_confirmed(
        &self,
        module: ConfigModule,
        preview_id: &str,
    ) -> Result<DomainRecoveryPlan, ConfigRecoveryError> {
        let plan = self.settings.plan_domain_recovery(module)?;
        if preview_id != plan.preview_id {
            return Err(ConfigRecoveryError::Unavailable {
                module,
                reason_code: aura_config::recovery::codes::PREVIEW_STALE,
            });
        }
        Ok(plan)
    }

    /// Plans a cross-domain recovery and checks it matches the preview the user saw.
    ///
    /// The bundle path used to rebuild the plan and apply it without ever consuming the
    /// request's `preview_id`, so a cross-domain confirmation was not binding either. The same
    /// check applies here, for the same reason.
    ///
    /// # What the bundle's identity is
    ///
    /// A `BundleRecoveryPlan` has no `preview_id` of its own; its binding facts are the
    /// per-domain `preview_id` of every member plan plus the `consistent_set` it would restore.
    /// The bundle's identity is therefore the **join of its members' identities**: a
    /// confirmation is valid only when every domain in the bundle is at exactly the state the
    /// user was shown. That is strictly stronger than comparing a single scalar, because a
    /// bundle whose members moved independently cannot be described by one revision number.
    pub fn plan_bundle_confirmed(
        &self,
        modules: &[ConfigModule],
        preview_id: &str,
    ) -> Result<BundleRecoveryPlan, ConfigRecoveryError> {
        let plan = self.settings.plan_bundle_recovery(modules)?;
        // The settings owner also binds any superseded save-journal identities
        // into this final token. Re-deriving only the member join rejects every
        // genuine all-domain confirmation (and discards part of the binding).
        if preview_id != plan.preview_id {
            return Err(ConfigRecoveryError::Unavailable {
                module: modules.first().copied().unwrap_or(ConfigModule::Aura),
                reason_code: aura_config::recovery::codes::PREVIEW_STALE,
            });
        }
        Ok(plan)
    }

    /// Whether a bundle plan still offers `action` among its available options.
    ///
    /// A preview lists the actions that were really available **when it was taken**. An action
    /// that has since become unavailable must be refused rather than applied, and an action the
    /// preview never listed is not something the user confirmed.
    #[must_use]
    pub fn bundle_permits(plan: &BundleRecoveryPlan, action: ConfigRecoveryAction) -> bool {
        plan.options.iter().any(|option| option.action == action && option.available)
    }

    /// Whether a single-domain plan still offers `action` among its available options.
    #[must_use]
    pub fn domain_permits(plan: &DomainRecoveryPlan, action: ConfigRecoveryAction) -> bool {
        plan.options.iter().any(|option| option.action == action && option.available)
    }

    /// Applies a previewed single-domain recovery and updates the registry.
    ///
    /// The rebuild's own result is **consumed**, not discarded.
    /// Before, `let _ = rebuilder.rebuild_missing(...)` threw away both a construction failure
    /// and the reason it failed, so the user could not be told why a repaired domain's feature
    /// still did not work.
    pub fn apply_domain(
        &self,
        plan: &DomainRecoveryPlan,
        action: ConfigRecoveryAction,
    ) -> Result<RecoveryReport, ConfigRecoveryError> {
        let outcome = self.settings.apply_domain_recovery(plan, action)?;
        // The registry follows a real recovery. `outcome.applied` is the fact that
        // decides it: a failed attempt must not mark anything available again.
        let (restored, unavailable) = if outcome.applied {
            self.degradation.restore_domain_for_module(plan.module);
            self.refresh_bundle_judgement();
            // **supply the missing services the repaired
            // configuration now permits**, before claiming anything is back.
            //
            // The record was already cleared above, which is only half the statement. A
            // capability is reported restored solely when its service is really present
            // afterwards. A supply that fails leaves the configuration recovery true and the
            // capability still missing, and the two are reported as the separate facts they are.
            //
            // The supply runs **once**, and both halves of the report are derived from its own
            // result. Asking them separately would let the two disagree — one reading the
            // services after a supply and the other reading them before it.
            self.supply_and_report(std::slice::from_ref(&plan.module))
        } else {
            (Vec::new(), Vec::new())
        };
        Ok(RecoveryReport {
            action,
            outcomes: vec![outcome],
            restored_capabilities: restored,
            unavailable_capabilities: unavailable,
            reason_code: None,
        })
    }

    /// Applies a previewed cross-domain recovery and updates the registry.
    ///
    /// The affected capabilities are computed from the **whole**
    /// recovered set once, and the supply runs once for that set. The previous per-outcome loop
    /// rebuilt the same capability repeatedly and could report a capability restored while
    /// another domain's fault for the same capability was still recorded (`ToolPolicy` depends
    /// on both `Tools` and `Aura`).
    pub fn apply_bundle(
        &self,
        plan: &BundleRecoveryPlan,
        action: ConfigRecoveryAction,
    ) -> Result<RecoveryReport, ConfigRecoveryError> {
        let outcomes = self.settings.apply_bundle_recovery(plan, action)?;
        // Only the domains that really applied can have removed a fault, and only the faults
        // that named **this** domain are cleared. A whole-capability clear would drop a fault
        // another domain recorded for the same capability.
        let mut repaired_domains = Vec::new();
        for outcome in &outcomes {
            if !outcome.applied {
                continue;
            }
            self.degradation.restore_domain_for_module(outcome.module);
            repaired_domains.push(outcome.module);
        }
        if !repaired_domains.is_empty() {
            self.refresh_bundle_judgement();
        }
        // The affected capabilities are computed from the **whole**
        // recovered set and supplied once. The previous per-outcome loop rebuilt the same
        // capability repeatedly and could report it restored while another domain's fault for
        // the same capability was still recorded (`ToolPolicy` depends on `Tools` and `Aura`).
        let (restored, unavailable) = if repaired_domains.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            self.supply_and_report(&repaired_domains)
        };
        Ok(RecoveryReport {
            action,
            outcomes,
            restored_capabilities: restored,
            unavailable_capabilities: unavailable,
            reason_code: None,
        })
    }

    /// Supplies once, then reports current availability and this attempt's refusals.
    ///
    /// Current service presence and domain health decide availability. Construction reasons
    /// come from this call's result, never a process-wide cache of another call's refusals.
    fn supply_and_report(
        &self,
        modules: &[ConfigModule],
    ) -> (Vec<String>, Vec<UnavailableCapability>) {
        let affected = self.capabilities_for_domains(modules);
        let refusals = self.rebuilder.as_ref().map_or_else(Vec::new, |rebuilder| {
            match rebuilder.rebuild_missing(modules) {
                Ok(outcome) => outcome.still_missing,
                // Keep this attempt's reasons. A process-wide "last" cache can already
                // describe a different concurrent recovery by the time we read it.
                Err(error) => {
                    affected.iter().map(|capability| (*capability, error.to_string())).collect()
                }
            }
        });
        let mut restored = Vec::new();
        let mut unavailable = Vec::new();
        for capability in &affected {
            // Both facts are required: the service must be really present, and the record must
            // no longer name a fault for that capability.
            let healthy = self.degradation.is_available(*capability);
            // Transaction recovery is a judgement, not a service slot. Its registry
            // records the unresolved responsibility; a healthy judgement has no missing
            // service to construct. Every other capability still needs its real service.
            let present = *capability == ConfigDependentCapability::ConfigTransactionRecovery
                || self.rebuilder.as_ref().is_some_and(|r| r.is_available(*capability));
            if present && healthy {
                restored.push(capability.key().to_owned());
            } else {
                let reason = if !healthy {
                    self.degradation.reason(*capability).map_or_else(
                        || capability.unavailable_code().to_owned(),
                        |fault| match fault.domain {
                            Some(domain) => format!("{} ({})", fault.fault_code, domain.key()),
                            None => fault.fault_code.to_owned(),
                        },
                    )
                } else {
                    refusals
                        .iter()
                        .find(|(candidate, _)| candidate == capability)
                        .map(|(_, reason)| reason.clone())
                        .unwrap_or_else(|| {
                            "its service is still absent, so this capability is not available"
                                .to_owned()
                        })
                };
                unavailable.push(UnavailableCapability {
                    capability: capability.key().to_owned(),
                    reason,
                });
            }
        }
        restored.sort();
        restored.dedup();
        unavailable.sort_by(|left, right| left.capability.cmp(&right.capability));
        (restored, unavailable)
    }

    fn refresh_bundle_judgement(&self) {
        // A domain reset alone is not evidence that the save set settled. The
        // same owner must confirm both the scope and the absence of held domains.
        if self.settings.recovery_scope_known() && self.settings.recovery_held_domains().is_empty()
        {
            self.degradation.restore_bundle_judgement();
        }
    }

    /// Whether a capability is currently usable, read from the registry.
    ///
    /// This is the consumer that makes the registry a live fact rather than a
    /// write-only record: a caller asks here instead of re-deriving availability
    /// from the configuration itself.
    #[must_use]
    pub fn capability_available(&self, capability: ConfigDependentCapability) -> bool {
        self.capability_is_usable(capability)
    }

    /// Whether Agent work can run at all, per the registry.
    ///
    /// This is what `RuntimeServices::agent_capability_available` consults, so the
    /// two answer the same question from the same fact instead of each deciding
    /// for itself.
    ///
    /// This used to consult **`ToolPolicy` alone**, and a measurement
    /// showed the consequence — with only `ConfigTransactionRecovery` unknown, the
    /// real `agent_work_available()` still returned `true`. New Agent work was
    /// therefore admitted while a cross-domain save's scope was unresolved, even
    /// though an unknown scope may cover every domain.
    ///
    /// Agent work depends on more than the tool policy:
    ///
    /// * `ToolPolicy` — which tools may run at all;
    /// * `SecurityAuthority` — the permission defaults and default mode the Agent
    ///   execution layer is bound by, so a Security fault must gate new work;
    /// * `ConfigTransactionRecovery` — an unresolved cross-domain bundle may have
    ///   left any domain between its before and after image, so new work that reads
    ///   configuration cannot be trusted to start.
    ///
    /// This answers the "may new work start" question only. Viewing, stopping,
    /// settling and diagnosing deliberately do **not** consult it, which is what
    /// keeps the user's stop and inspection paths alive.
    #[must_use]
    pub fn agent_work_available(&self) -> bool {
        self.agent_work_blocker().is_none()
    }

    /// The capability blocking new Agent work, when one is.
    ///
    /// Callers surface this so a refusal names the real reason instead of a generic
    /// "unavailable". The order matches [`Self::agent_work_available`], so the
    /// reported reason is the first capability that would have blocked.
    ///
    /// # The two facts, and why both are required
    ///
    /// Configuration health, service presence and the
    /// cross-domain transaction judgement are **different facts**, and new work depends on all of
    /// them. The previous form consulted the degradation record alone, which has two consequences
    /// the brief names:
    ///
    /// * `ModelAuthority` was not in the set at all, so a damaged `models.json` — the very fault
    ///   that removes the Agent chain's model authority — did not gate new Agent work;
    /// * a **construction failure** was invisible: the record is cleared as soon as the domain is
    ///   repaired, so a capability whose service could not be built was reported available and new
    ///   work was admitted against a service that does not exist.
    ///
    /// A capability therefore blocks when its **record** names a fault *or* when its **service**
    /// (or its real consumer's chain) is not present. The second half is what makes the answer agree
    /// with the health projection and with the refusal an ordinary new call would receive.
    ///
    /// This answers the "may new work start" question only. Viewing, stopping, settling and
    /// diagnosing deliberately do **not** consult it, which is what keeps the user's stop and
    /// inspection paths alive.
    #[must_use]
    pub fn agent_work_blocker(&self) -> Option<(ConfigDependentCapability, String)> {
        [
            ConfigDependentCapability::ToolPolicy,
            ConfigDependentCapability::SecurityAuthority,
            ConfigDependentCapability::ModelAuthority,
            ConfigDependentCapability::ConfigTransactionRecovery,
        ]
        .into_iter()
        .find(|capability| !self.capability_is_usable(*capability))
        .map(|capability| {
            // The reason names the fact that really blocks: a recorded fault when there is one, and
            // the capability's own stable code otherwise. Reporting a fault code for a capability
            // whose domain is healthy would send the user to inspect configuration that is fine,
            // which is why the two cases are distinguished.
            let reason = if self.degradation.is_available(capability) {
                capability.unavailable_code().to_owned()
            } else {
                self.degradation.reason(capability).map_or_else(
                    || capability.unavailable_code().to_owned(),
                    |entry| entry.fault_code.to_owned(),
                )
            };
            (capability, reason)
        })
    }

    /// Whether a capability is usable: its record is healthy **and** its service is really there.
    ///
    /// `ConfigTransactionRecovery` is a judgement rather than a
    /// service, so its record is the whole answer; every other capability additionally needs the
    /// service a new call would actually use. A process with no runtime rebuilder has no dependent
    /// services, so the record is likewise the whole answer there.
    fn capability_is_usable(&self, capability: ConfigDependentCapability) -> bool {
        if !self.degradation.is_available(capability) {
            return false;
        }
        if capability == ConfigDependentCapability::ConfigTransactionRecovery {
            return true;
        }
        self.rebuilder.as_ref().is_none_or(|rebuilder| rebuilder.is_available(capability))
    }

    /// Why a capability is unavailable, when it is.
    #[must_use]
    pub fn capability_reason(&self, capability: ConfigDependentCapability) -> Option<String> {
        self.degradation.reason(capability).map(|entry| entry.fault_code.to_owned())
    }

    /// Which capabilities a repaired **set** of domains feeds.
    ///
    /// Computed from the whole set, so a capability that two of
    /// the repaired domains share (`ToolPolicy` depends on both `Tools` and `Aura`) is supplied
    /// and reported **once**.
    fn capabilities_for_domains(&self, modules: &[ConfigModule]) -> Vec<ConfigDependentCapability> {
        ConfigDependentCapability::ALL
            .into_iter()
            .filter(|capability| modules.iter().any(|module| capability.domains().contains(module)))
            .collect()
    }

    /// The domains a scoped reset would touch, so the caller can state the scope.
    ///
    /// This is the "preview the scope before triggering it" half for a caller that
    /// only needs the scope: it never resets everything, and it never guesses.
    #[must_use]
    pub fn scoped_reset_domains(&self, modules: &[ConfigModule]) -> Vec<ConfigModule> {
        let mut scoped = modules.to_vec();
        scoped.sort_unstable();
        scoped.dedup();
        scoped
    }
}

/// Supplies the live runtime's missing dependent services after a configuration repair.
///
/// The recovery entry used to clear degradation **markers**
/// and report `restored_capabilities` from the registry alone. That proves a record was
/// deleted; it does **not** prove a service exists. The consequence, exactly:
/// `a_repair_brings_the_capability_back_without_a_restart` clears markers by hand and
/// checks only the health projection, so it passes while the real slot is still `None`
/// and a real request for that capability would still fail.
///
/// This is the seam that closes the gap: the runtime supplies a way to fill the missing
/// slots, and the recovery entry reports a capability as restored **only when the slot is
/// actually present afterwards**.
///
/// # Contract for an implementation
///
/// * **Reuse the live owners.** It must use the running process's `PersistenceCoordinator`,
///   `BrokerJournal`, `SettingsFacade`, `SyncBoundary`, `AgentStore` and activity owners.
///   It must not open a second database, create a second admission capacity, or construct
///   a second owner of anything.
/// * **By real dependency.** It may recompute the whole dependency graph and publish only
///   the affected part. A consumer whose dependency is still missing must **not** be
///   reported ready: supplying `Agent` requires `Commands` and the model authority to be
///   present, because the execution layer writes through the former and reads the latter.
/// * **Do not repeat startup recovery.** Creating an `AgentApplication` runs the
///   Agent/inbox startup scan and creates a `RunSupervisor`; opening P05 finishes pending
///   model-configuration transactions. Those are **one-time process startup work** and must
///   not be re-run against a live runtime, and no second supervisor may exist for the same
///   runs.
/// * **Do not replace a working service.** Only a missing slot is filled. An object that is
///   already serving keeps serving, so an in-flight task's owner, run identity, cancel and
///   settlement path are unchanged.
/// * **Prepare, then publish in a short section.** Disk work goes through the existing
///   bounded execution boundary; no runtime-wide lock may be held across I/O or an await.
/// * **Return what is really present**, so the caller can report configuration recovery and
///   capability restoration as the two separate facts they are.
pub trait DependentServiceRebuilder: Send + Sync {
    /// Fills the missing dependent services that the repaired configuration now permits.
    ///
    /// `repaired_domains` are the domains whose configuration was really recovered, so a
    /// rebuilder can narrow its work to the capabilities those domains feed.
    ///
    /// Returns the capabilities whose slot is **really present** afterwards, plus a
    /// per-capability reason for any that could not be supplied.
    fn rebuild_missing(
        &self,
        repaired_domains: &[ConfigModule],
    ) -> Result<CapabilityRebuildOutcome, CapabilityRebuildError>;

    /// Whether one capability's service is present and usable **right now**.
    ///
    /// This is the fact a recovery report must rest on. It is a read: asking must not
    /// construct, repair or publish anything.
    fn is_available(&self, capability: ConfigDependentCapability) -> bool;
}

/// What a rebuild really achieved, kept separate from what it attempted.
///
/// The split matters for the user-facing statement: "the configuration was recovered" and
/// "the capability works again" are two facts, and a rebuild that failed must not erase the
/// first while claiming the second.
#[derive(Clone, Debug, Default)]
pub struct CapabilityRebuildOutcome {
    /// Capabilities whose service slot is present and usable now.
    pub available: Vec<ConfigDependentCapability>,
    /// Capabilities still missing, each with a bounded reason.
    pub still_missing: Vec<(ConfigDependentCapability, String)>,
}

/// Why a rebuild could not run at all. A single refusal for the whole attempt.
#[derive(Clone, Debug)]
pub struct CapabilityRebuildError {
    /// A bounded, path-free reason.
    pub reason: String,
}

impl std::fmt::Display for CapabilityRebuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for CapabilityRebuildError {}

/// Whether a `SettingsFacadeError` names a configuration fault the recovery entry
/// can act on.
///
/// A recovery entry may only offer itself for a *configuration* fault; offering it
/// for an unrelated failure would send the user to a repair that cannot help. This
/// delegates to [`code_is_recoverable`] so there is **one** judgement rather than
/// two that could drift.
#[must_use]
pub fn is_recoverable_configuration_fault(error: &SettingsFacadeError) -> bool {
    match error.stable_code() {
        Some(code) => code_is_recoverable(code),
        // An error with no stable code is not a classified configuration fault, so
        // the recovery entry does not claim it can repair it.
        None => false,
    }
}

/// Whether a fault **code** is one the recovery entry can act on.
///
/// The same judgement as [`is_recoverable_configuration_fault`], for a caller that
/// holds the code rather than the error — which is what a notice carries.
#[must_use]
pub fn code_is_recoverable(code: &str) -> bool {
    // The recovery entry acts on configuration faults. A runtime-state code (the
    // recovery-required lifecycle) is named in the notice but is not something a
    // configuration restore can fix.
    code.starts_with("config.")
        || code == crate::config_degradation::BUNDLE_SCOPE_UNRESOLVED
        || ConfigDependentCapability::ALL
            .into_iter()
            .any(|capability| capability.unavailable_code() == code)
}

#[cfg(test)]
mod report_tests;

#[cfg(test)]
mod reset_judgement_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_registry_produces_a_notice_that_offers_no_repair() {
        let registry = ConfigDegradationRegistry::default();
        let notice = RecoveryNoticeBuilder::from_registry(&registry);
        assert!(notice.affected_domains.is_empty());
        assert!(notice.unavailable_capabilities.is_empty());
        assert!(notice.available_actions.is_empty());
        assert!(notice.scope_known);
        assert!(notice.next_step.contains("No configuration capability"));
    }

    #[test]
    fn every_declared_capability_fault_is_recognized_by_the_notice_check() {
        for capability in ConfigDependentCapability::ALL {
            assert!(code_is_recoverable(capability.unavailable_code()));
        }
        assert!(!code_is_recoverable("core.unrelated_failure"));
    }

    #[test]
    fn an_unknown_scope_is_reported_as_unknown_and_never_as_an_empty_scope() {
        let registry = ConfigDegradationRegistry::default();
        registry.degrade(
            ConfigDependentCapability::ConfigTransactionRecovery,
            None,
            "core.config_recovery.unresolved",
            false,
        );
        let notice = RecoveryNoticeBuilder::from_registry(&registry);
        assert!(
            !notice.scope_known,
            "an unknown scope must be reported as unknown, not as no scope"
        );
        // The restore option is not offered for a scope the runtime cannot prove.
        assert!(
            !notice
                .available_actions
                .contains(&ConfigRecoveryAction::RestorePreviousValid.key().to_owned()),
            "a cross-domain restore must not be offered while the scope is unknown: {:?}",
            notice.available_actions
        );
        assert!(
            notice
                .available_actions
                .contains(&ConfigRecoveryAction::ResetToDefault.key().to_owned()),
            "the scoped default reset is offered instead: {:?}",
            notice.available_actions
        );
        assert!(
            notice
                .suspended_calls
                .iter()
                .any(|call| call.contains("every configuration-dependent")),
            "the user is told the suspension is broad because the scope is unknown: {:?}",
            notice.suspended_calls
        );
        assert!(
            notice.next_step.contains("will never reset configuration without that confirmation"),
            "the notice states that an unknown scope is not silently reset: {}",
            notice.next_step
        );
    }

    #[test]
    fn the_notice_names_the_domain_the_capability_and_the_suspension() {
        let registry = ConfigDegradationRegistry::default();
        registry.degrade(
            ConfigDependentCapability::ToolPolicy,
            Some(ConfigModule::Tools),
            "config.domain.codec",
            true,
        );
        let notice = RecoveryNoticeBuilder::from_registry(&registry);
        assert_eq!(notice.affected_domains, vec!["tools".to_owned()]);
        assert_eq!(notice.unavailable_capabilities, vec!["tool_policy".to_owned()]);
        assert_eq!(notice.fault_codes, vec!["config.domain.codec".to_owned()]);
        assert!(notice.suspended_calls.iter().any(|call| call.contains("tool execution")));
        assert!(notice.scope_known);
    }

    #[test]
    fn a_notice_carries_no_path_and_no_value() {
        let registry = ConfigDegradationRegistry::default();
        registry.degrade(
            ConfigDependentCapability::SecurityAuthority,
            Some(ConfigModule::Security),
            "config.domain.io",
            true,
        );
        let described = serde_json::to_value(RecoveryNoticeBuilder::from_registry(&registry))
            .expect("serialize notice");
        let fields = described.as_object().expect("notice object");
        for value in fields.values() {
            let strings =
                value.as_array().map_or_else(|| vec![value], |items| items.iter().collect());
            for text in strings.into_iter().filter_map(serde_json::Value::as_str) {
                assert!(!text.contains(':'), "notice value contains a drive separator: {text}");
                assert!(!text.contains('\\'), "notice value contains a path separator: {text}");
                assert!(!text.contains('/'), "notice value contains a path separator: {text}");
            }
        }
    }
}
