//! Per-session start ordering and authority ownership.
//!
//! The authority gate has two consumers with two different waiting costs, and both
//! must observe the same one barrier:
//!
//! * a **synchronous** reader ([`AuthorityGate::read`]) and writer
//!   ([`AuthorityGate::write`]) — the paths that already run whole inside a bounded
//!   blocking owner, so parking their thread is the owner's normal and bounded
//!   behaviour;
//! * an **asynchronous** mode change ([`AuthorityGate::write_async`]) — the public
//!   `PersistenceCoordinator::change_session_permission_mode`, which is awaited
//!   directly from the request reactor and must therefore never park a runtime worker
//!   while it waits. The mode change used to take the write
//!   side synchronously from an async stack, so a live same-session authority section
//!   stopped the executor that also owes the timer, the heartbeat and every other
//!   session its progress.
//!
//! The async wait is **this** gate's condition variable — the same mutex, the same
//! predicate, the same poisoning rule — not a copy and not a second barrier, so a
//! mode change still cannot commit inside its session's live authority section and the
//! reader is still held for the whole section rather than released early to look
//! non-blocking.
use crate::PersistenceError;
use aura_domain_common::SessionId;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::task::{Context, Poll};

/// The wake-up list an async waiter registers on.
///
/// It holds `std::task::Waker`s rather than one shared `Notify`: a `Notified` future
/// created and dropped inside a single `poll` is *deregistered* when that poll returns
/// `Pending`, so a waiter built that way is never woken again. Registering the waker
/// itself is what makes the release reach the waiter, and it also lets a released waiter
/// deregister so a long-lived gate does not accumulate wakers for dropped futures.
#[derive(Default)]
struct Bumps {
    waiters: Mutex<Vec<std::task::Waker>>,
}

impl Bumps {
    fn register(&self, waker: &std::task::Waker) {
        let Ok(mut waiters) = self.waiters.lock() else {
            return;
        };
        waiters.retain(|waiter| !waiter.will_wake(waker));
        waiters.push(waker.clone());
    }

    fn deregister(&self, waker: &std::task::Waker) {
        let Ok(mut waiters) = self.waiters.lock() else {
            return;
        };
        waiters.retain(|waiter| !waiter.will_wake(waker));
    }

    fn bump(&self) {
        let Ok(mut waiters) = self.waiters.lock() else {
            return;
        };
        for waker in std::mem::take(&mut *waiters) {
            waker.wake();
        }
    }
}

#[derive(Default)]
pub(super) struct AuthorityGate {
    state: Mutex<AuthorityState>,
    available: Condvar,
    writer_poisoned: std::sync::atomic::AtomicBool,
    bumps: Arc<Bumps>,
}

#[derive(Default)]
struct AuthorityState {
    readers: usize,
    writer: bool,
}

pub(crate) struct AuthorityRead(Arc<AuthorityGate>);
pub(crate) struct AuthorityWrite(Arc<AuthorityGate>);

impl Drop for AuthorityRead {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.readers = state.readers.saturating_sub(1);
        }
        self.0.available.notify_all();
        self.0.bumps.bump();
    }
}

impl Drop for AuthorityWrite {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.writer = false;
            // Preserve the former RwLock's writer-unwind failure semantics. Only this
            // session becomes unavailable; a later lookup must not replace its gate.
            if std::thread::panicking() {
                self.0.writer_poisoned.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        self.0.available.notify_all();
        self.0.bumps.bump();
    }
}

impl AuthorityGate {
    fn is_poisoned(&self) -> bool {
        self.state.is_poisoned() || self.writer_poisoned.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(super) fn read(self: &Arc<Self>) -> Result<AuthorityRead, PersistenceError> {
        let mut state = self.state.lock().map_err(|_| PersistenceError::ReadLockPoisoned)?;
        while state.writer {
            state = self.available.wait(state).map_err(|_| PersistenceError::ReadLockPoisoned)?;
        }
        if self.is_poisoned() {
            return Err(PersistenceError::ReadLockPoisoned);
        }
        state.readers = state.readers.saturating_add(1);
        Ok(AuthorityRead(Arc::clone(self)))
    }

    pub(super) fn write(self: &Arc<Self>) -> Result<AuthorityWrite, PersistenceError> {
        let mut state = self.state.lock().map_err(|_| PersistenceError::ReadLockPoisoned)?;
        while state.writer || state.readers > 0 {
            state = self.available.wait(state).map_err(|_| PersistenceError::ReadLockPoisoned)?;
        }
        if self.is_poisoned() {
            return Err(PersistenceError::ReadLockPoisoned);
        }
        state.writer = true;
        Ok(AuthorityWrite(Arc::clone(self)))
    }

    /// The write side, waited for **without parking a runtime worker**.
    ///
    /// Same gate, same predicate, same poisoning rule as [`Self::write`]; the only
    /// difference is that a waiter that cannot take it yet registers on this gate's
    /// own wake-up list and returns [`Poll::Pending`] instead of sitting in
    /// `Condvar::wait`. An async mode change is therefore genuinely excluded from a
    /// live same-session authority section (the wait is real and unbounded until the
    /// section ends) while the thread it runs on stays free to serve the timer, the
    /// heartbeat and another session's request.
    ///
    /// Dropping the returned future before it resolves acquires nothing: the future
    /// only ever sets `writer` in the branch that hands the guard back, so a cancelled
    /// caller cannot leave the gate held.
    pub(super) fn write_async(self: &Arc<Self>) -> AuthorityWriteFuture {
        AuthorityWriteFuture { gate: Arc::clone(self) }
    }
}

/// Allocation-free polling over the gate's own state, for an async waiter.
///
/// The waker is registered **while the predicate is still false**, and the predicate is
/// checked under the gate's own mutex, so a release that lands anywhere after the check
/// also runs `bump` after it - and `bump` is called after the state is already updated.
/// The waiter cannot miss a release: either the check sees it, or the waker it has
/// already installed is woken by it.
pub(super) struct AuthorityWriteFuture {
    gate: Arc<AuthorityGate>,
}

impl Future for AuthorityWriteFuture {
    type Output = Result<AuthorityWrite, PersistenceError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let gate = Arc::clone(&self.gate);
        let mut state = match gate.state.try_lock() {
            Ok(state) => state,
            // The mutex itself is momentarily held; this is not the predicate being
            // false, so try again rather than reporting a fault.
            Err(std::sync::TryLockError::WouldBlock) => {
                gate.bumps.register(context.waker());
                return Poll::Pending;
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Poll::Ready(Err(PersistenceError::ReadLockPoisoned));
            }
        };
        if state.writer || state.readers > 0 {
            drop(state);
            gate.bumps.register(context.waker());
            return Poll::Pending;
        }
        if gate.is_poisoned() {
            return Poll::Ready(Err(PersistenceError::ReadLockPoisoned));
        }
        state.writer = true;
        drop(state);
        gate.bumps.deregister(context.waker());
        Poll::Ready(Ok(AuthorityWrite(Arc::clone(&gate))))
    }
}

/// Owned guards let a live holder or waiter retain this session's exact gate.
#[derive(Clone, Default)]
pub(super) struct SessionGate {
    pub(super) authority: Arc<AuthorityGate>,
    pub(super) start: Arc<tokio::sync::RwLock<()>>,
}

#[derive(Default)]
pub(super) struct PermissionGates {
    sessions: Mutex<BTreeMap<SessionId, SessionGate>>,
    shared: SessionGate,
    pub(super) share: std::sync::atomic::AtomicBool,
}

/// Healthy idle gates can be evicted; holders, waiters and known faults retain their gates.
const PERMISSION_GATE_SESSIONS: usize = 256;

impl PermissionGates {
    fn registry(&self) -> MutexGuard<'_, BTreeMap<SessionId, SessionGate>> {
        // This mutex protects lookup/eviction, not permission state. BTreeMap remains
        // valid after unwind. Recover its existing bindings: minting a new gate here
        // would let a same-session writer bypass a live reader or start lease.
        self.sessions.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn of(&self, session: SessionId) -> SessionGate {
        if self.share.load(std::sync::atomic::Ordering::Acquire) {
            return self.shared.clone();
        }
        let mut sessions = self.registry();
        if sessions.len() >= PERMISSION_GATE_SESSIONS {
            sessions.retain(|_, gate| {
                Arc::strong_count(&gate.authority) > 1
                    || Arc::strong_count(&gate.start) > 1
                    || gate.authority.is_poisoned()
            });
        }
        sessions.entry(session).or_default().clone()
    }

    pub(super) fn live(&self) -> Vec<(SessionId, bool, bool)> {
        self.registry()
            .iter()
            .map(|(session, gate)| {
                (
                    *session,
                    Arc::strong_count(&gate.authority) > 1,
                    Arc::strong_count(&gate.start) > 1,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
