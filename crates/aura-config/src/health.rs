//! Per-domain health for the fifteen configuration domains.
//!
//! One domain's load, parse, recovery or write failure must not disable the
//! domains that do not depend on it, and must not take away sessions or control.
//! Before this module `SettingsFacade::open_with_validators` used `?` inside its
//! `for module in ConfigModule::ALL` loop, so the first failing domain aborted the
//! whole facade — all fifteen domains and every settings consumer with them.
//!
//! The model here is deliberately small and honest:
//!
//! * a domain is either [`DomainHealthState::Ready`] (it has a valid in-memory
//!   snapshot) or [`DomainHealthState::Degraded`] (it does not);
//! * a degraded domain records the *original* failure — the reason its own load
//!   failed — and a separate recovery log of later attempts. A recovery attempt
//!   that succeeds clears the domain; one that fails appends to the log and never
//!   overwrites the original failure;
//! * health is per domain. Nothing here holds a facade-wide lock, so a domain
//!   whose I/O is slow cannot block a different domain's read.
//!
//! This module does **not** invent a fallback for "there is no legitimate LKG".
//! An existing invalid file without a legitimate last-known-good is preserved
//! and its domain reported degraded. Fresh defaults must pass domain validation.

use std::sync::{Arc, Mutex};

use aura_domain_common::UtcTimestamp;

use crate::{ConfigModule, ConfigSource, SettingsFacadeError};

const MAX_RECOVERY_ATTEMPTS: usize = 64;

/// The bounded, stable classification of why a domain is not usable.
///
/// A closed enum rather than a string so callers and tests can reject a wrong
/// implementation instead of matching on message text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainFaultKind {
    /// The file could not be opened, read or stat'ed.
    Io,
    /// The bytes were present but could not be decoded as the module's format.
    Codec,
    /// The decoded document did not satisfy base or module validation.
    Validation,
    /// The document exceeded the per-module safety limit.
    FileTooLarge,
    /// A bundle transaction journal belongs to this domain and could not be
    /// converged.
    BundleRecovery,
    /// An internal lock was poisoned by an earlier panic.
    LockPoisoned,
}

impl DomainFaultKind {
    /// Classifies a facade error into the closed set above.
    ///
    /// `SettingsFacadeError` carries the real cause and its own module; the
    /// mapping is total so a new variant cannot be silently reported as "ready".
    #[must_use]
    pub const fn of(error: &SettingsFacadeError) -> Self {
        match error {
            SettingsFacadeError::Paths(_) | SettingsFacadeError::Io { .. } => Self::Io,
            SettingsFacadeError::Codec { .. } => Self::Codec,
            SettingsFacadeError::Validation { .. } => Self::Validation,
            SettingsFacadeError::FileTooLarge { .. }
            | SettingsFacadeError::FileTooLargeWithLkg { .. } => Self::FileTooLarge,
            SettingsFacadeError::BundleRecoveryConflict { .. }
            | SettingsFacadeError::BundleJournal { .. }
            | SettingsFacadeError::BundleUnresolved { .. } => Self::BundleRecovery,
            SettingsFacadeError::LockPoisoned(_) => Self::LockPoisoned,
            // A domain that is unavailable because it was never opened is
            // classified by the failure that stopped it, which is what
            // `original_fault` already holds; a second-hand report of the same
            // condition is still an I/O-class unavailability.
            SettingsFacadeError::DomainUnavailable { .. } => Self::Io,
            // The remaining variants describe a *request* being refused (revision
            // conflict, empty bundle, and so on), not a domain that failed to
            // load. They are still classified rather than ignored so a future
            // caller cannot read them as health.
            SettingsFacadeError::RevisionConflict { .. }
            | SettingsFacadeError::NonMonotonicRevision { .. }
            | SettingsFacadeError::RevisionExhausted { .. }
            | SettingsFacadeError::AtomicReplace(_)
            | SettingsFacadeError::FaultInjected(_)
            | SettingsFacadeError::Watch(_)
            | SettingsFacadeError::EmptyBundle
            | SettingsFacadeError::DuplicateBundleModule(_)
            | SettingsFacadeError::BundleFaultInjected(_)
            | SettingsFacadeError::BundlePending { .. }
            | SettingsFacadeError::BundleNotPending
            | SettingsFacadeError::BlankBundleTransactionId
            | SettingsFacadeError::DomainBusy { .. } => Self::Io,
        }
    }

    /// A bounded, renderer-safe code for the fault. No path, no bytes, no secret.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Io => "config.domain.io",
            Self::Codec => "config.domain.codec",
            Self::Validation => "config.domain.validation",
            Self::FileTooLarge => "config.domain.file_too_large",
            Self::BundleRecovery => "config.domain.bundle_recovery",
            Self::LockPoisoned => "config.domain.lock_poisoned",
        }
    }
}

/// One retry that the facade made for a domain, and whether it worked.
///
/// Recorded separately from the original failure so a later successful recovery
/// cannot erase the fact that the domain was unusable and why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainRecoveryAttempt {
    pub attempt: u32,
    pub succeeded: bool,
    pub fault: Option<DomainFaultKind>,
    pub message: String,
    pub occurred_at: UtcTimestamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainHealthState {
    Ready,
    Degraded,
}

/// The health of exactly one configuration domain.
#[derive(Clone, Debug)]
pub struct DomainHealth {
    pub module: ConfigModule,
    pub state: DomainHealthState,
    /// The failure that made this domain unusable, kept verbatim.
    pub original_fault: Option<DomainFaultKind>,
    pub original_message: Option<String>,
    /// What the domain is currently serving, if it is serving anything.
    pub source: Option<ConfigSource>,
    pub revision: Option<u64>,
    pub degraded_since: Option<UtcTimestamp>,
    pub recovery_attempts: Vec<DomainRecoveryAttempt>,
}

impl DomainHealth {
    #[must_use]
    pub fn ready(module: ConfigModule, source: ConfigSource, revision: u64) -> Self {
        Self {
            module,
            state: DomainHealthState::Ready,
            original_fault: None,
            original_message: None,
            source: Some(source),
            revision: Some(revision),
            degraded_since: None,
            recovery_attempts: Vec::new(),
        }
    }

    #[must_use]
    pub fn degraded(module: ConfigModule, error: &SettingsFacadeError) -> Self {
        Self {
            module,
            state: DomainHealthState::Degraded,
            original_fault: Some(DomainFaultKind::of(error)),
            original_message: Some(error.to_string()),
            source: None,
            revision: None,
            degraded_since: Some(UtcTimestamp::now()),
            recovery_attempts: Vec::new(),
        }
    }

    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self.state, DomainHealthState::Ready)
    }

    #[must_use]
    pub fn original_fault_code(&self) -> Option<&'static str> {
        self.original_fault.map(DomainFaultKind::code)
    }

    /// Records a later attempt without disturbing the original failure.
    fn note_attempt(&mut self, succeeded: bool, error: Option<&SettingsFacadeError>) {
        let attempt =
            self.recovery_attempts.last().map_or(1, |last| last.attempt.saturating_add(1));
        if self.recovery_attempts.len() == MAX_RECOVERY_ATTEMPTS {
            self.recovery_attempts.remove(0);
        }
        self.recovery_attempts.push(DomainRecoveryAttempt {
            attempt,
            succeeded,
            fault: error.map(DomainFaultKind::of),
            message: error.map_or_else(|| "recovered".to_owned(), ToString::to_string),
            occurred_at: UtcTimestamp::now(),
        });
    }

    /// Moves the domain to ready after a successful retry.
    ///
    /// The original failure is preserved: the domain really was unusable, and a
    /// reader that only sees "ready" would lose that fact.
    fn mark_recovered(&mut self, source: ConfigSource, revision: u64) {
        self.note_attempt(true, None);
        self.state = DomainHealthState::Ready;
        self.source = Some(source);
        self.revision = Some(revision);
        self.degraded_since = None;
    }
}

/// Facade-wide health, one entry per domain.
///
/// Every method takes the mutex for the duration of one small update and the
/// per-domain entries are independent, so a slow domain never holds anything a
/// different domain needs.
pub struct ConfigHealthRegistry {
    domains: Mutex<Vec<DomainHealth>>,
}

impl ConfigHealthRegistry {
    #[must_use]
    pub fn new(domains: Vec<DomainHealth>) -> Self {
        Self { domains: Mutex::new(domains) }
    }

    fn with_domain<T>(
        &self,
        module: ConfigModule,
        update: impl FnOnce(&mut DomainHealth) -> T,
    ) -> T {
        let mut domains = self.domains.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let index = domains
            .iter()
            .position(|health| health.module == module)
            .expect("every ConfigModule value has one health entry");
        update(&mut domains[index])
    }

    #[must_use]
    pub fn health(&self, module: ConfigModule) -> DomainHealth {
        self.with_domain(module, |health| health.clone())
    }

    /// Every domain's health in `ConfigModule::ALL` order.
    #[must_use]
    pub fn all(&self) -> Vec<DomainHealth> {
        self.domains.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    /// The domains that are currently degraded, in module order.
    #[must_use]
    pub fn degraded(&self) -> Vec<ConfigModule> {
        self.all()
            .into_iter()
            .filter(|health| !health.is_ready())
            .map(|health| health.module)
            .collect()
    }

    /// Clears a bundle hold once **all** live responsibility over a domain is gone.
    ///
    /// The held set is recomputed from the complete set
    /// of surviving records, and this is called only for a domain that no live
    /// record names *and* whose real service is present again. The original fault
    /// is deliberately preserved — the domain really was unusable, and a reader
    /// that loses that fact cannot tell a repaired domain from one that was never
    /// broken.
    pub fn clear_bundle_hold(&self, module: ConfigModule) {
        self.with_domain(module, |health| {
            if health.state == DomainHealthState::Degraded {
                health.mark_recovered(
                    health.source.unwrap_or(ConfigSource::File),
                    health.revision.unwrap_or(0),
                );
            }
        });
    }

    /// Records a failed retry. The original failure is not touched.
    pub fn record_failure(&self, module: ConfigModule, error: &SettingsFacadeError) {
        self.with_domain(module, |health| health.note_attempt(false, Some(error)));
    }

    /// Records a successful retry and marks the domain ready.
    pub fn record_recovery(&self, module: ConfigModule, source: ConfigSource, revision: u64) {
        self.with_domain(module, |health| health.mark_recovered(source, revision));
    }

    /// Tracks an accepted snapshot without inventing a recovery attempt.
    pub(crate) fn record_snapshot(
        &self,
        module: ConfigModule,
        source: ConfigSource,
        revision: u64,
    ) {
        self.with_domain(module, |health| {
            health.source = Some(source);
            health.revision = Some(revision);
        });
    }

    /// Records that a write against a ready domain failed.
    ///
    /// A failed save does not by itself make the domain unusable — the previous
    /// valid snapshot is still what the process is serving — so the domain keeps
    /// its state and the attempt is recorded for inspection.
    pub fn record_write_failure(&self, module: ConfigModule, error: &SettingsFacadeError) {
        self.with_domain(module, |health| health.note_attempt(false, Some(error)));
    }

    /// Marks a domain degraded because its own load failed, or because a bundle
    /// recovery that includes it could not converge.
    pub fn mark_degraded(&self, module: ConfigModule, error: &SettingsFacadeError) {
        self.with_domain(module, |health| {
            if health.original_fault.is_none() {
                health.original_fault = Some(DomainFaultKind::of(error));
                health.original_message = Some(error.to_string());
            }
            if health.is_ready() {
                health.degraded_since = Some(UtcTimestamp::now());
            }
            health.state = DomainHealthState::Degraded;
            health.source = None;
            health.revision = None;
            health.note_attempt(false, Some(error));
        });
    }
}

/// A short, bounded summary line for the degraded set, for logs and the report.
///
/// Contains module keys and stable codes only — never a path, a document body or
/// anything a caller supplied.
#[must_use]
pub fn degraded_summary(health: &[DomainHealth]) -> String {
    use std::fmt::Write as _;

    let mut summary = String::new();
    for domain in health.iter().filter(|domain| !domain.is_ready()) {
        if !summary.is_empty() {
            summary.push(';');
        }
        let _ = write!(
            summary,
            "{}={}",
            domain.module.key(),
            domain.original_fault.map_or("config.domain.unknown", |fault| fault.code())
        );
    }
    summary
}

/// Convenience for callers that already hold `Arc`s of the registry.
pub type SharedConfigHealth = Arc<ConfigHealthRegistry>;
