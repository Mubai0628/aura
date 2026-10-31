//! What a configuration fault costs, and what it does **not** cost.
//!
//! # The defect this module removes
//!
//! Core's startup used to treat a configuration fault as a Core fault:
//!
//! * `ConfigTransactionJournal::recover_with(...)?` failed the whole runtime;
//! * `SettingsFacade::open_with_validators(...)?` failed the whole runtime;
//! * P03 Tools, P04 Security/Aura and P05 Models each `?`-ed their startup
//!   configuration snapshot, so one bad domain propagated to all of Core;
//! * and the Tools read fell back to
//!   `.unwrap_or_else(|_| ToolSettingsSnapshotV1::default())`, which **fabricates
//!   a legitimate authority out of a read failure**.
//!
//! The result was that one damaged configuration domain took away sessions,
//! status, stop and repair — the exact capabilities the user's confirmed rule
//! requires to survive.
//!
//! # The model
//!
//! This module does not invent a second configuration source of truth and it does
//! not loosen any permission. It records, per domain, **which capability lost its
//! configuration** and **the stable reason**. Consumers then:
//!
//! * serve unrelated capabilities from their own, still-valid domains;
//! * refuse the genuinely dependent capability honestly, with a stable code;
//! * never substitute a default for a failed read.
//!
//! # Why a default is not an acceptable fallback here
//!
//! `ToolSettingsSnapshotV1::default()` is a *legitimate* value when a domain has
//! no file yet — that is the "first use" case the user confirmed may create a
//! validated default. It is not legitimate when the read *failed*, because then
//! the runtime does not know whether the stored policy was more restrictive. A
//! default that happens to be more permissive would silently widen authority, and
//! one that is more restrictive would silently narrow it. Both are fabrications,
//! so the read failure is reported as a fault instead.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use aura_config::{ConfigModule, DomainFaultKind, SettingsFacadeError};

/// One capability of Core that a configuration domain feeds.
///
/// A closed enum rather than a string: a caller that asks "is P04 Security
/// available" must get a typed answer, and adding a capability must force a
/// decision at every match site instead of silently defaulting to "available".
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ConfigDependentCapability {
    /// The Agent execution layer's tool policy (`tools.json`).
    ToolPolicy,
    /// P04 Security's permission defaults and Aura's default mode.
    SecurityAuthority,
    /// P05's provider and model configuration (`models.json`).
    ModelAuthority,
    /// The cross-domain bundle journal's convergence at startup.
    ConfigTransactionRecovery,
}

impl ConfigDependentCapability {
    /// Every capability, in a stable order.
    pub const ALL: [Self; 4] = [
        Self::ToolPolicy,
        Self::SecurityAuthority,
        Self::ModelAuthority,
        Self::ConfigTransactionRecovery,
    ];

    /// The configuration domains this capability cannot work without.
    ///
    /// This is the *dependency* declaration, kept in one place so the dependency
    /// is a stated fact rather than an emergent property of which `?` happens to
    /// be where.
    #[must_use]
    pub const fn domains(self) -> &'static [ConfigModule] {
        match self {
            // A tool policy read is the domain itself; the execution layer also
            // needs Aura's global policy switch.
            Self::ToolPolicy => &[ConfigModule::Tools, ConfigModule::Aura],
            // Reads both security.toml and aura.toml.
            Self::SecurityAuthority => &[ConfigModule::Security, ConfigModule::Aura],
            Self::ModelAuthority => &[ConfigModule::Models],
            // Convergence can name any domain, so this capability depends on the
            // whole set.
            Self::ConfigTransactionRecovery => &ConfigModule::ALL,
        }
    }

    /// A bounded, renderer-safe code for the capability being unavailable.
    #[must_use]
    pub const fn unavailable_code(self) -> &'static str {
        match self {
            Self::ToolPolicy => "core.tool_policy.config_unavailable",
            Self::SecurityAuthority => "core.security_authority.config_unavailable",
            Self::ModelAuthority => "core.model_authority.config_unavailable",
            Self::ConfigTransactionRecovery => "core.config_recovery.unresolved",
        }
    }

    /// The capability's name for a log line or a status message.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::ToolPolicy => "tool_policy",
            Self::SecurityAuthority => "security_authority",
            Self::ModelAuthority => "model_authority",
            Self::ConfigTransactionRecovery => "config_transaction_recovery",
        }
    }
}

/// A capability that lost its configuration, and the stable reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DegradedCapability {
    pub capability: ConfigDependentCapability,
    /// The domain whose fault caused it, when a single domain can be named.
    pub domain: Option<ConfigModule>,
    /// The stable classification of the underlying configuration fault.
    pub fault_code: &'static str,
    /// Whether the capability's own dependency set is fully known.
    ///
    /// `false` is the recovery-scope-unknown case: the runtime cannot say which
    /// domains are in doubt, so it does not assume none of them are.
    pub scope_known: bool,
}

/// The process's configuration-degradation state.
///
/// One small mutex, held only for a map edit. It owns no configuration, no file
/// handle and no permission, so it cannot become a second source of truth.
///
/// Each capability now keeps **every** fault that named it,
/// not only the most recent one.
///
/// Before, `degrade` did a plain `insert`, so a second fault on the same
/// capability silently **overwrote** the first. Because a capability can be
/// disabled by more than one domain (`ToolPolicy` depends on `Tools` *and* `Aura`),
/// breaking two domains in sequence lost the evidence of the first: the notice
/// reported one domain and the user could repair it without the runtime ever
/// stating that a second fault was still there. Recovery then appeared to fail for
/// no reason, because the reason had been erased.
///
/// `is_available` is unchanged — any recorded fault makes the capability
/// unavailable. Only the *evidence* is now cumulative.
#[derive(Default)]
pub struct ConfigDegradationRegistry {
    degraded: Mutex<BTreeMap<ConfigDependentCapability, Vec<DegradedCapability>>>,
}

impl ConfigDegradationRegistry {
    /// Records that a capability cannot work, with the real reason.
    ///
    /// A second fault on the same capability is **added**, not substituted. An
    /// exact repeat (same domain, same code, same scope answer) is not duplicated,
    /// so a retry loop cannot grow the record without bound.
    pub fn degrade(
        &self,
        capability: ConfigDependentCapability,
        domain: Option<ConfigModule>,
        fault_code: &'static str,
        scope_known: bool,
    ) {
        if let Ok(mut degraded) = self.degraded.lock() {
            // The `AURA_S05R2_OVERWRITE_FAULT_COUNTERFACTUAL` switch
            // that used to live here has been **removed from product code**. It restored
            // the pre-repair per-capability overwrite, which could silently discard a
            // recorded fault — a product-path mutation of the degradation record, not a
            // test seam. The counterfactual is reproducible by temporarily editing this
            // function.
            let entry = degraded.entry(capability).or_default();
            let candidate = DegradedCapability { capability, domain, fault_code, scope_known };
            let already_recorded = entry.iter().any(|existing| {
                existing.domain == candidate.domain
                    && existing.fault_code == candidate.fault_code
                    && existing.scope_known == candidate.scope_known
            });
            if !already_recorded {
                entry.push(candidate);
            }
        }
    }

    /// Clears every fault that named one domain, across all capabilities.
    ///
    /// A recovered **domain** may have recorded faults against several
    /// capabilities (`Tools` feeds `ToolPolicy`; `Security` feeds `SecurityAuthority`), and
    /// a caller usually has the domain rather than the capability list. This clears exactly
    /// the faults that named that domain and leaves every other domain's evidence intact,
    /// so both scopings are preserved.
    ///
    /// This is a **record** operation. It says nothing about whether a service exists; the
    /// caller must still establish that before reporting a capability as restored.
    pub fn restore_domain_for_module(&self, domain: ConfigModule) {
        for capability in ConfigDependentCapability::ALL {
            self.restore_domain(capability, domain);
        }
    }

    /// Clears only the faults that named one domain, leaving any others.
    ///
    /// This is the honest half of a partial repair. Recovering
    /// `Tools` must not clear a fault that `Aura` recorded for the same capability,
    /// or the capability would be reported available while a real fault remains.
    ///
    /// A whole-capability `restore` was removed in favour of this: it cleared every
    /// fault for a capability from a repair that had only fixed one domain, so it
    /// was the very operation this replacement exists to prevent.
    pub fn restore_domain(&self, capability: ConfigDependentCapability, domain: ConfigModule) {
        if let Ok(mut degraded) = self.degraded.lock() {
            let remove_capability = match degraded.get_mut(&capability) {
                Some(entry) => {
                    entry.retain(|existing| existing.domain != Some(domain));
                    entry.is_empty()
                }
                None => false,
            };
            if remove_capability {
                degraded.remove(&capability);
            }
        }
    }

    /// Called only after the settings owner has settled its complete held set.
    /// Retains unrelated transaction responsibility and all per-domain failures.
    pub fn restore_bundle_judgement(&self) {
        if let Ok(mut degraded) = self.degraded.lock() {
            let capability = ConfigDependentCapability::ConfigTransactionRecovery;
            if let Some(entry) = degraded.get_mut(&capability) {
                entry.retain(|fault| {
                    fault.domain.is_some() || fault.fault_code != BUNDLE_SCOPE_UNRESOLVED
                });
                if entry.is_empty() {
                    degraded.remove(&capability);
                }
            }
        }
    }

    /// Whether one capability is currently usable.
    #[must_use]
    pub fn is_available(&self, capability: ConfigDependentCapability) -> bool {
        self.degraded
            .lock()
            .map(|degraded| degraded.get(&capability).is_none_or(|entry| entry.is_empty()))
            .unwrap_or(false)
    }

    /// The refused capability's reason, if it is degraded.
    ///
    /// When several faults named the same capability the **first** is returned, in
    /// recording order: it is the one that originally made the capability
    /// unavailable, and later faults are additional evidence rather than a
    /// replacement.
    #[must_use]
    pub fn reason(&self, capability: ConfigDependentCapability) -> Option<DegradedCapability> {
        self.degraded
            .lock()
            .ok()
            .and_then(|degraded| degraded.get(&capability).and_then(|entry| entry.first().cloned()))
    }

    /// Every degraded capability, in enum order.
    ///
    /// A capability with several recorded faults appears **once per fault**, so a
    /// caller listing what is affected sees the whole set rather than one sample.
    #[must_use]
    pub fn all(&self) -> Vec<DegradedCapability> {
        self.degraded
            .lock()
            .map(|degraded| degraded.values().flatten().cloned().collect())
            .unwrap_or_default()
    }

    /// Whether the recovery scope itself is unknown.
    ///
    /// While it is, the runtime keeps status, stop and repair reachable and does
    /// not claim the uncertain configuration is Ready. This is the user's
    /// confirmed protection, exposed as one query rather than re-derived by every
    /// consumer.
    #[must_use]
    pub fn recovery_scope_unknown(&self) -> bool {
        self.degraded.lock().is_ok_and(|degraded| {
            degraded
                .get(&ConfigDependentCapability::ConfigTransactionRecovery)
                .is_some_and(|entry| entry.iter().any(|fault| !fault.scope_known))
        })
    }

    /// A bounded summary for a log line: capability keys and stable codes only.
    ///
    /// Contains no path, no document body and nothing a caller supplied.
    #[must_use]
    pub fn summary(&self) -> String {
        self.all()
            .into_iter()
            .map(|entry| format!("{}={}", entry.capability.key(), entry.fault_code))
            .collect::<Vec<_>>()
            .join(";")
    }
}

/// Turns a configuration error into the pair (domain, stable fault code).
///
/// The **raw input does not enter ordinary error output**: the stable code is
/// what a projection or a log carries, and the original message stays inside the
/// configuration layer where it already is.
#[must_use]
pub fn config_fault_code(error: &SettingsFacadeError) -> &'static str {
    DomainFaultKind::of(error).code()
}

/// The shared degradation registry, owned by the runtime.
pub type SharedConfigDegradation = Arc<ConfigDegradationRegistry>;

/// This fault is owned by the module-bundle judgement, not the separate legacy
/// transaction journal (which may also describe credential/package cleanup).
pub const BUNDLE_SCOPE_UNRESOLVED: &str = "core.config_recovery.bundle_unresolved";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capability_is_available_until_it_is_degraded_and_again_after_repair() {
        let registry = ConfigDegradationRegistry::default();
        let capability = ConfigDependentCapability::ToolPolicy;
        assert!(registry.is_available(capability));

        registry.degrade(capability, Some(ConfigModule::Tools), "config.domain.codec", true);
        assert!(!registry.is_available(capability));
        let reason = registry.reason(capability).expect("a reason is recorded");
        assert_eq!(reason.domain, Some(ConfigModule::Tools));
        assert_eq!(reason.fault_code, "config.domain.codec");
        assert!(reason.scope_known);

        registry.restore_domain(capability, ConfigModule::Tools);
        assert!(registry.is_available(capability));
        assert!(registry.reason(capability).is_none());
    }

    /// **Cumulative-fault counterexample.** A second fault must not erase the first.
    ///
    /// `degrade` used to be a plain `insert` into a map keyed by capability, so a
    /// second fault on the same capability **replaced** the first. Because
    /// `ToolPolicy` depends on both `Tools` and `Aura`, breaking `Aura` after
    /// `Tools` lost the `Tools` evidence entirely: the notice named one domain, and
    /// a repair of that one domain appeared to succeed while the other was still
    /// broken — with no recorded reason why.
    #[test]
    fn a_second_fault_does_not_erase_the_first_for_the_same_capability() {
        let registry = ConfigDegradationRegistry::default();
        let capability = ConfigDependentCapability::ToolPolicy;

        registry.degrade(capability, Some(ConfigModule::Tools), "config.domain.codec", true);
        registry.degrade(capability, Some(ConfigModule::Aura), "config.domain.io", true);

        // Both faults are still on the record.
        let faults = registry.all();
        let domains = faults.iter().filter_map(|fault| fault.domain).collect::<Vec<_>>();
        assert!(
            domains.contains(&ConfigModule::Tools) && domains.contains(&ConfigModule::Aura),
            "both faults must be retained; got {domains:?}"
        );
        assert_eq!(registry.all().len(), 2, "one record per fault, not per capability");

        // Repairing one domain clears only that domain's fault…
        registry.restore_domain(capability, ConfigModule::Tools);
        assert!(
            !registry.is_available(capability),
            "the capability is still unavailable while Aura's fault remains"
        );
        let remaining = registry.all();
        assert_eq!(remaining.len(), 1, "only the repaired domain's fault is cleared");
        assert_eq!(remaining[0].domain, Some(ConfigModule::Aura));

        // …and the capability is available only once no fault remains.
        registry.restore_domain(capability, ConfigModule::Aura);
        assert!(registry.is_available(capability));
    }

    /// An exact repeat is not duplicated, so a retry loop cannot grow the record.
    #[test]
    fn an_identical_repeat_is_not_recorded_twice() {
        let registry = ConfigDegradationRegistry::default();
        let capability = ConfigDependentCapability::ToolPolicy;
        for _ in 0..5 {
            registry.degrade(capability, Some(ConfigModule::Tools), "config.domain.codec", true);
        }
        assert_eq!(registry.all().len(), 1, "an identical fault is recorded once");
    }

    #[test]
    fn an_unknown_recovery_scope_is_reported_as_unknown_not_as_absent() {
        let registry = ConfigDegradationRegistry::default();
        assert!(!registry.recovery_scope_unknown());
        registry.degrade(
            ConfigDependentCapability::ConfigTransactionRecovery,
            None,
            "config.domain.bundle_recovery",
            false,
        );
        assert!(
            registry.recovery_scope_unknown(),
            "an unknown scope must be visible as unknown, not silently treated as no scope"
        );
    }

    #[test]
    fn the_summary_is_bounded_and_carries_no_path_or_body() {
        let registry = ConfigDegradationRegistry::default();
        registry.degrade(
            ConfigDependentCapability::ModelAuthority,
            Some(ConfigModule::Models),
            "config.domain.validation",
            true,
        );
        let summary = registry.summary();
        assert_eq!(summary, "model_authority=config.domain.validation");
        assert!(!summary.contains(':'));
        assert!(!summary.contains('/'));
        assert!(!summary.contains('\\'));
    }
}
