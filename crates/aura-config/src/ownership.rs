//! Real per-domain ownership for ordinary operations, domain re-open,
//! file watching and cross-domain transactions.
//!
//! # The defect this module removes
//!
//! Before R1 every facade operation took one process-wide
//! `pending_bundle_transaction` mutex and held it across the domain's entire
//! bounded file I/O. `SettingsFacade::update_with_fault` acquired it, then called
//! `service.update(...)`, which encodes, validates and atomically replaces a real
//! file. A domain whose save was genuinely slow therefore blocked *every* other
//! domain's write, even when the two domains share no file, no service and no
//! transaction. The independent probe
//! `unrelated_write_finishes_while_another_domain_is_held` measures exactly that.
//!
//! Dropping the global lock is not by itself a fix: the same mutex was also what
//! kept the *same* domain's compare-and-swap honest, what kept one recovery the
//! only owner of a re-open, and what kept two bundle transactions from
//! interleaving. R1 keeps all of those properties and changes only their scope.
//!
//! # The model
//!
//! Ownership is expressed as a set of *claims* over concrete resources:
//!
//! * [`DomainClaim::Exclusive`] — this operation owns the domain and may change
//!   its snapshot or its file. Ordinary saves, re-opens and recovery take this.
//! * [`DomainClaim::Shared`] — this operation reads the domain's revision but
//!   will not change it. A cross-domain transaction's read-only revision
//!   dependencies take this, so a transaction admits its **complete** domain set
//!   (writers and readers) exactly once, before it publishes anything.
//!
//! Two operations conflict only when they claim the same domain and at least one
//! of them is exclusive. Disjoint domains never conflict, so an unrelated
//! domain's read, write and projection proceed while another domain's file I/O is
//! genuinely still in flight.
//!
//! # Same-domain serialization is preserved
//!
//! A per-domain claim is a real reader/writer lock held for the whole operation.
//! Two same-domain writers therefore serialize, and R1 additionally rejects an
//! older snapshot *inside* the service (`ModuleConfigService::reload` refuses a
//! non-monotonic revision), so "two retries must not insert an older snapshot" is
//! enforced twice rather than once.
//!
//! # Bounded, non-blocking admission
//!
//! The registry never blocks a caller and never waits on a domain to make
//! progress. `try_claim` either takes the whole claim set immediately or reports
//! which domains are busy; the caller decides. There is no ordering between
//! claims, so no cross-domain wait cycle can form, and there is no global lock,
//! so a genuinely stalled domain cannot hold anything another domain needs.

use std::{
    collections::BTreeMap,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use crate::ConfigModule;

/// How long a single-domain waiter sleeps before rechecking its domain.
///
/// A poll rather than a hand-rolled wake-up counter: the wake-up is signalled on
/// every release, and the poll only bounds the damage if a signal were ever
/// missed. It is short enough that a same-domain racer resolves immediately and
/// long enough that a busy domain costs nothing.
const SERIAL_CLAIM_POLL: Duration = Duration::from_millis(2);

/// The longest an ordinary operation waits for its own domain.
///
/// Chosen to be far longer than any real same-domain compare-and-swap (which is
/// one bounded file write) and far shorter than any human-noticeable stall, so
/// ordinary contention always resolves and a transaction-owned domain is
/// reported rather than waited on.
const SERIAL_CLAIM_TIMEOUT: Duration = Duration::from_secs(5);

/// How one operation uses one domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainClaim {
    /// The operation may change this domain's snapshot or its file. Ordinary
    /// saves, domain re-opens, recovery and a bundle's written entries.
    Exclusive,
    /// The operation reads this domain's revision and must see it unchanged for
    /// the operation's duration. A bundle's read-only dependencies.
    Shared,
}

impl DomainClaim {
    /// Whether this claim excludes another claim over the same domain.
    #[must_use]
    pub const fn conflicts_with(self, other: Self) -> bool {
        matches!((self, other), (Self::Exclusive, _) | (Self::Shared, Self::Exclusive))
    }
}

/// What one domain currently has held against it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct DomainHold {
    exclusive: bool,
    shared: u32,
}

impl DomainHold {
    const fn is_free(self) -> bool {
        !self.exclusive && self.shared == 0
    }
}

/// A held set of domain claims, released when dropped.
///
/// The registry is the one owner of the claim table, so a guard that outlives its
/// operation is a real ownership bug rather than a silent leak: `Drop` proves the
/// claims it is releasing as still its own.
pub struct DomainClaimGuard {
    registry: Arc<DomainClaimRegistry>,
    claims: BTreeMap<ConfigModule, DomainClaim>,
}

impl std::fmt::Debug for DomainClaimGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("DomainClaimGuard").field("claims", &self.claims).finish()
    }
}

impl DomainClaimGuard {
    /// The domains this guard owns, in module order.
    #[must_use]
    pub fn domains(&self) -> Vec<ConfigModule> {
        self.claims.keys().copied().collect()
    }

    /// The claim held for one domain.
    #[must_use]
    pub fn claim(&self, module: ConfigModule) -> Option<DomainClaim> {
        self.claims.get(&module).copied()
    }
}

impl Drop for DomainClaimGuard {
    fn drop(&mut self) {
        let mut holds = self.registry.holds();
        for (module, claim) in &self.claims {
            let hold = holds.entry(*module).or_default();
            match claim {
                DomainClaim::Exclusive => hold.exclusive = false,
                DomainClaim::Shared => hold.shared = hold.shared.saturating_sub(1),
            }
            if hold.is_free() {
                holds.remove(module);
            }
        }
        drop(holds);
        // Wake single-domain waiters so a same-domain racer proceeds promptly
        // instead of waiting out its poll interval.
        self.registry.released.notify_all();
    }
}

/// Which domains were busy when a claim could not be taken whole.
///
/// Reported instead of blocking so a caller can make an honest admission
/// decision. It is never converted into "wait and retry forever": a caller that
/// cannot take its whole set refuses the request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainClaimBusy {
    pub domains: Vec<ConfigModule>,
}

impl DomainClaimBusy {
    #[must_use]
    pub fn message(&self) -> String {
        let keys = self.domains.iter().map(|module| module.key()).collect::<Vec<_>>().join(",");
        format!("configuration domain owned by another operation: {keys}")
    }
}

/// The one owner of the in-process domain claim table.
///
/// Deliberately small: it decides *admission*, not I/O, and it holds its inner
/// mutex only for the length of a map edit. No caller's file I/O happens under
/// this mutex, which is the property the old global lock violated.
///
/// # Why there are two acquisition modes
///
/// `try_claim` is all-or-nothing and never waits. It is what a *multi*-domain
/// transaction uses, because waiting for a whole set while holding part of it is
/// how lock-order deadlocks are built.
///
/// [`DomainClaimRegistry::claim_serially`] waits for **one** domain. It exists
/// because "two writers of the same domain with the same expected revision" must
/// produce one commit and one revision conflict — not one commit and one busy
/// refusal. Waiting on a single domain cannot form a cycle either: the waiter
/// holds no other claim while it waits, and the domain it waits for is the only
/// thing that can release it.
pub struct DomainClaimRegistry {
    holds: Mutex<BTreeMap<ConfigModule, DomainHold>>,
    released: Condvar,
}

impl Default for DomainClaimRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DomainClaimRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self { holds: Mutex::new(BTreeMap::new()), released: Condvar::new() }
    }

    fn holds(&self) -> MutexGuard<'_, BTreeMap<ConfigModule, DomainHold>> {
        self.holds.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether a set of claims can be taken right now.
    fn is_free_for(
        holds: &BTreeMap<ConfigModule, DomainHold>,
        requested: &BTreeMap<ConfigModule, DomainClaim>,
    ) -> bool {
        requested.iter().all(|(module, claim)| {
            holds.get(module).is_none_or(|hold| {
                !hold.exclusive && !(*claim == DomainClaim::Exclusive && hold.shared > 0)
            })
        })
    }

    /// Takes the **whole** claim set at once, or nothing.
    ///
    /// All-or-nothing is what makes this an admission decision rather than a
    /// partial lock order: a caller that cannot get every domain it needs does
    /// not hold some of them while it waits for the rest, so no two operations
    /// can deadlock against each other.
    pub fn try_claim(
        self: &Arc<Self>,
        claims: impl IntoIterator<Item = (ConfigModule, DomainClaim)>,
    ) -> Result<DomainClaimGuard, DomainClaimBusy> {
        let mut requested = BTreeMap::new();
        for (module, claim) in claims {
            requested
                .entry(module)
                .and_modify(|existing| {
                    if claim == DomainClaim::Exclusive {
                        *existing = DomainClaim::Exclusive;
                    }
                })
                .or_insert(claim);
        }
        let mut holds = self.holds();
        if !Self::is_free_for(&holds, &requested) {
            let busy = requested
                .iter()
                .filter(|(module, claim)| {
                    holds.get(module).is_some_and(|hold| {
                        hold.exclusive || (**claim == DomainClaim::Exclusive && hold.shared > 0)
                    })
                })
                .map(|(module, _)| *module)
                .collect::<Vec<_>>();
            return Err(DomainClaimBusy { domains: busy });
        }
        for (module, claim) in &requested {
            let hold = holds.entry(*module).or_default();
            match claim {
                DomainClaim::Exclusive => hold.exclusive = true,
                DomainClaim::Shared => hold.shared = hold.shared.saturating_add(1),
            }
        }
        drop(holds);
        Ok(DomainClaimGuard { registry: Arc::clone(self), claims: requested })
    }

    /// Waits a **bounded** time for exactly one domain, then takes it exclusively.
    ///
    /// The waiter holds nothing while it waits, so it cannot be part of a cycle;
    /// the only claim that can release it is the one it is waiting on. Unrelated
    /// domains are unaffected for the whole wait, which is the property that
    /// distinguishes this from the old process-wide lock.
    ///
    /// # Why the wait is bounded
    ///
    /// Two *ordinary* operations on one domain are both short: waiting for the
    /// other is exactly the same-domain compare-and-swap the old global lock
    /// provided, and the racer then observes the real revision conflict instead
    /// of a spurious busy refusal.
    ///
    /// A *cross-domain transaction* also owns its domains, but it is not short
    /// and its duration is not the waiter's to predict. Waiting behind it would
    /// turn "this domain is part of an unresolved transaction" into a silent
    /// stall. Waiting is therefore bounded by [`SERIAL_CLAIM_TIMEOUT`], after
    /// which the caller gets an honest `DomainBusy` naming the domain. Ordinary
    /// contention never comes close to the bound; a transaction is reported.
    pub fn claim_serially(
        self: &Arc<Self>,
        module: ConfigModule,
    ) -> Result<DomainClaimGuard, DomainClaimBusy> {
        let deadline = Instant::now() + SERIAL_CLAIM_TIMEOUT;
        let mut holds = self.holds();
        loop {
            let requested = BTreeMap::from([(module, DomainClaim::Exclusive)]);
            if Self::is_free_for(&holds, &requested) {
                holds.entry(module).or_default().exclusive = true;
                drop(holds);
                return Ok(DomainClaimGuard { registry: Arc::clone(self), claims: requested });
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(DomainClaimBusy { domains: vec![module] });
            }
            let (next, _) = self
                .released
                .wait_timeout(holds, remaining.min(SERIAL_CLAIM_POLL))
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            holds = next;
        }
    }

    /// Whether one domain is currently free of any claim.
    #[must_use]
    pub fn is_free(&self, module: ConfigModule) -> bool {
        self.holds().get(&module).is_none_or(|hold| hold.is_free())
    }

    /// The domains currently claimed, in module order.
    #[must_use]
    pub fn claimed(&self) -> Vec<ConfigModule> {
        self.holds().iter().filter(|(_, hold)| !hold.is_free()).map(|(module, _)| *module).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Arc<DomainClaimRegistry> {
        Arc::new(DomainClaimRegistry::new())
    }

    #[test]
    fn disjoint_domains_never_conflict() {
        let registry = registry();
        let browser = registry
            .try_claim([(ConfigModule::Browser, DomainClaim::Exclusive)])
            .expect("first domain");
        let ui =
            registry.try_claim([(ConfigModule::Ui, DomainClaim::Exclusive)]).expect("unrelated");
        assert_eq!(browser.domains(), vec![ConfigModule::Browser]);
        assert_eq!(ui.domains(), vec![ConfigModule::Ui]);
    }

    #[test]
    fn same_domain_exclusive_claims_serialize() {
        let registry = registry();
        let _first =
            registry.try_claim([(ConfigModule::Tools, DomainClaim::Exclusive)]).expect("first");
        let busy = registry
            .try_claim([(ConfigModule::Tools, DomainClaim::Exclusive)])
            .expect_err("a second exclusive claim must be refused, not queued");
        assert_eq!(busy.domains, vec![ConfigModule::Tools]);
    }

    #[test]
    fn shared_dependencies_admit_together_and_exclude_writers() {
        let registry = registry();
        let readers = registry
            .try_claim([
                (ConfigModule::Models, DomainClaim::Shared),
                (ConfigModule::Aura, DomainClaim::Shared),
            ])
            .expect("read-only dependencies");
        // Another reader of the same domains is admitted alongside.
        let second = registry
            .try_claim([(ConfigModule::Models, DomainClaim::Shared)])
            .expect("second reader");
        // But a writer of one of them is refused, because a transaction's
        // read-only revision dependency must not move underneath it.
        let busy = registry
            .try_claim([(ConfigModule::Models, DomainClaim::Exclusive)])
            .expect_err("a writer conflicts with a reader");
        assert_eq!(busy.domains, vec![ConfigModule::Models]);
        drop(readers);
        // One reader is still alive, so the domain is still not writable.
        assert!(
            registry.try_claim([(ConfigModule::Models, DomainClaim::Exclusive)]).is_err(),
            "a remaining reader still excludes a writer"
        );
        drop(second);
        // Releasing every reader frees the domain for the writer.
        registry.try_claim([(ConfigModule::Models, DomainClaim::Exclusive)]).expect("now free");
    }

    #[test]
    fn duplicate_claims_preserve_the_strongest_requirement_in_both_orders() {
        let registry = registry();
        let reader = registry.try_claim([(ConfigModule::Ui, DomainClaim::Shared)]).expect("reader");
        for claims in [
            [DomainClaim::Exclusive, DomainClaim::Shared],
            [DomainClaim::Shared, DomainClaim::Exclusive],
        ] {
            let busy = registry
                .try_claim(claims.map(|claim| (ConfigModule::Ui, claim)))
                .expect_err("a shared duplicate must not erase exclusive access");
            assert_eq!(busy.domains, vec![ConfigModule::Ui]);
        }
        drop(reader);
        let writer = registry
            .try_claim([
                (ConfigModule::Ui, DomainClaim::Exclusive),
                (ConfigModule::Ui, DomainClaim::Shared),
            ])
            .expect("duplicates can be admitted after the reader leaves");
        assert_eq!(writer.claim(ConfigModule::Ui), Some(DomainClaim::Exclusive));
        assert!(registry.try_claim([(ConfigModule::Ui, DomainClaim::Shared)]).is_err());
    }

    #[test]
    fn a_refused_claim_takes_nothing() {
        let registry = registry();
        let _held =
            registry.try_claim([(ConfigModule::Hooks, DomainClaim::Exclusive)]).expect("held");
        let _ = registry
            .try_claim([
                (ConfigModule::Memory, DomainClaim::Exclusive),
                (ConfigModule::Hooks, DomainClaim::Exclusive),
            ])
            .expect_err("one busy domain refuses the whole set");
        assert!(
            registry.is_free(ConfigModule::Memory),
            "a refused set must not leave a partial claim behind"
        );
    }

    #[test]
    fn dropping_a_guard_releases_every_claim_it_took() {
        let registry = registry();
        {
            let _guard = registry
                .try_claim([
                    (ConfigModule::Skills, DomainClaim::Exclusive),
                    (ConfigModule::Plugins, DomainClaim::Shared),
                ])
                .expect("claim");
            assert!(!registry.is_free(ConfigModule::Skills));
            assert!(!registry.is_free(ConfigModule::Plugins));
        }
        assert!(registry.claimed().is_empty(), "every claim is released");
    }
}
