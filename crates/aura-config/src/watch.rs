//! Bounded, per-domain coalescing file watching.
//!
//! # The defect this module removes
//!
//! `SettingsFacade::start_watching` used to spawn **one** thread that received
//! raw `notify` events over an `mpsc` channel and called `reload` for each
//! affected module inline, in a `for` loop, after a fixed sleep. That loop is
//! serial: a domain whose `reload` is doing real, slow file I/O holds the only
//! consumer, so a change to an *unrelated* domain waits behind it. The watcher
//! therefore reproduced exactly the coupling R1 removes from ordinary saves,
//! through a different door.
//!
//! # The model
//!
//! * **One notifier thread** receives raw events and does no I/O at all. It maps
//!   each event's paths to domains and marks them dirty. A slow domain can
//!   therefore never stall event intake.
//! * **Per-domain coalescing**: a domain that is already pending is not queued
//!   again. A burst of writes to one file becomes one reload, which is what
//!   "coalesces bounded per-domain notifications" means, and the pending set is
//!   bounded by the number of domains (fifteen), not by the event rate.
//! * **A bounded worker per domain**: the dispatcher starts one worker per dirty
//!   domain, up to a fixed small cap, and each worker does exactly one domain's
//!   reload. A really slow domain occupies one worker; the rest keep running.
//!
//! No worker ever waits for another domain, so there is no cross-domain wait
//! cycle, and the number of threads is bounded by [`WATCH_WORKERS`] rather than
//! growing with the event rate.

use std::{
    collections::BTreeSet,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use notify::{EventKind, RecursiveMode, Watcher};

use crate::{ConfigModule, SettingsFacade, SettingsFacadeError};

/// The most domain reloads the watcher will have in flight at once.
///
/// Bounded deliberately: the watcher must not turn a burst of filesystem events
/// into unbounded thread growth, which would move the problem rather than solve
/// it. Fifteen domains exist, so this is enough to keep an unrelated domain
/// moving while several others are busy, and small enough to stay bounded.
pub const WATCH_WORKERS: usize = 4;

/// The domains currently waiting to be re-read, and those already being re-read.
///
/// A set rather than a queue: re-marking a domain that is already pending is a
/// no-op, so a burst of events for one file collapses into one reload.
///
/// # In-flight deduplication
///
/// `dirty` alone was not enough. `take` removed a domain from
/// the pending set and handed it to a worker, but recorded nothing — so if the
/// notifier re-marked that domain while its worker was still doing real file I/O,
/// **another** worker took it too. Two workers then contended on the same domain
/// (one losing to the claim and burning its iteration), and with enough re-marks a
/// single slow domain could occupy every worker and stall unrelated domains. That
/// is the very coupling this module exists to remove, re-entering through the
/// re-mark path; a thread cap alone does not prove isolation, because the cap can
/// be filled by one domain.
///
/// `in_flight` records the domains currently owned by a worker. `mark` puts a
/// domain back into `dirty` only when it is not already in flight, so a domain has
/// at most one worker. When that worker finishes it re-marks the domain if a
/// change arrived while it was busy — so a merged update is never lost, it is
/// simply handled by one worker in the next iteration instead of two at once.
#[derive(Default)]
struct PendingDomains {
    dirty: Mutex<PendingState>,
    signal: Condvar,
}

#[derive(Default)]
struct PendingState {
    /// Domains waiting for a worker.
    dirty: BTreeSet<ConfigModule>,
    /// Domains currently owned by a worker.
    in_flight: BTreeSet<ConfigModule>,
    /// Domains marked while they were in flight, to be retried on completion.
    redo: BTreeSet<ConfigModule>,
}

impl PendingDomains {
    fn mark(&self, modules: impl IntoIterator<Item = ConfigModule>) {
        let mut state = self.dirty.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = state.dirty.len();
        for module in modules {
            if state.in_flight.contains(&module) {
                // Already being re-read. Queue a follow-up instead of letting a
                // second worker take the same domain.
                state.redo.insert(module);
            } else {
                state.dirty.insert(module);
            }
        }
        if state.dirty.len() != before {
            self.signal.notify_all();
        }
    }

    /// Takes one pending domain, waiting at most `timeout` for one to appear.
    ///
    /// The returned domain is marked **in flight** for as long as the caller holds
    /// it; [`Self::finished`] must be called to release it.
    fn take(&self, timeout: Duration) -> Option<ConfigModule> {
        let mut state = self.dirty.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.dirty.is_empty() {
            let (next, _) = self
                .signal
                .wait_timeout(state, timeout)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
        }
        // A domain marked while in flight becomes eligible once its worker is
        // done; promote it here so a merged update is never dropped.
        let redo: Vec<ConfigModule> = state.redo.iter().copied().collect();
        for module in redo {
            if !state.in_flight.contains(&module) {
                state.redo.remove(&module);
                state.dirty.insert(module);
            }
        }
        let module = state.dirty.iter().next().copied()?;
        state.dirty.remove(&module);
        state.in_flight.insert(module);
        Some(module)
    }

    /// Releases a domain so it can be taken again.
    ///
    /// A change that arrived while the worker held the domain is promoted back to
    /// pending here, so coalescing delays a merged update rather than losing it.
    fn finished(&self, module: ConfigModule) {
        let mut state = self.dirty.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.in_flight.remove(&module);
        if state.redo.remove(&module) {
            state.dirty.insert(module);
            self.signal.notify_all();
        }
    }
}

/// A running configuration watcher.
///
/// Dropping it stops the notifier and every worker and joins them, so a test or
/// a shutdown cannot leak threads that keep re-reading configuration.
pub struct ConfigWatcherHandle {
    join: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    pending: Arc<PendingDomains>,
    workers: Vec<JoinHandle<()>>,
}

impl ConfigWatcherHandle {
    /// The stop flag, for callers that need to signal shutdown out of band.
    pub(crate) fn stop_flag(&self) -> &Arc<AtomicBool> {
        &self.stop
    }
}

impl Drop for ConfigWatcherHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake every worker so it observes the stop flag instead of waiting out
        // its poll interval.
        self.pending.mark(ConfigModule::ALL);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// Starts watching the configuration root, coalescing per domain.
pub fn start_watching(
    facade: &Arc<SettingsFacade>,
) -> Result<ConfigWatcherHandle, SettingsFacadeError> {
    let config_root = facade.paths().config_root.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = sender.send(event);
    })?;
    watcher.watch(&config_root, RecursiveMode::Recursive)?;

    let stop = Arc::new(AtomicBool::new(false));
    let pending = Arc::new(PendingDomains::default());

    let worker_stop = Arc::clone(&stop);
    let worker_pending = Arc::clone(&pending);
    let worker_facade = Arc::clone(facade);
    let mut workers = Vec::with_capacity(WATCH_WORKERS);
    for index in 0..WATCH_WORKERS {
        let stop = Arc::clone(&worker_stop);
        let pending = Arc::clone(&worker_pending);
        let facade = Arc::clone(&worker_facade);
        let worker = thread::Builder::new()
            .name(format!("aura-config-watch-{index}"))
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    let Some(module) = pending.take(Duration::from_millis(200)) else {
                        continue;
                    };
                    // One domain's reload per worker iteration. `reload` claims
                    // only this domain, so a genuinely slow domain here blocks
                    // nothing but itself.
                    let _ = facade.reload_for_watch(module);
                    // Release the domain exactly once, after the real work, so a
                    // change that arrived meanwhile is handled by this domain's
                    // next iteration rather than by a second worker at the same
                    // time.
                    pending.finished(module);
                }
            })
            .map_err(|source| SettingsFacadeError::Io {
                action: "spawn config watcher worker",
                path: config_root.clone(),
                source,
            })?;
        workers.push(worker);
    }

    let notify_stop = Arc::clone(&stop);
    let notify_pending = Arc::clone(&pending);
    let notify_facade = Arc::clone(facade);
    let join = thread::Builder::new()
        .name("aura-config-notify".to_owned())
        .spawn(move || {
            // The watcher is owned by this thread, so it lives exactly as long as
            // the notifier that reads it.
            let _watcher = watcher;
            while !notify_stop.load(Ordering::Acquire) {
                let event = match receiver.recv_timeout(Duration::from_millis(250)) {
                    Ok(Ok(event)) => event,
                    Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                };
                if matches!(event.kind, EventKind::Access(_)) {
                    continue;
                }
                let modules = event
                    .paths
                    .iter()
                    .filter_map(|path| notify_facade.module_for_path(path))
                    .collect::<Vec<_>>();
                notify_pending.mark(modules);
            }
        })
        .map_err(|source| SettingsFacadeError::Io {
            action: "spawn config watcher",
            path: config_root,
            source,
        })?;

    Ok(ConfigWatcherHandle { join: Some(join), stop, pending, workers })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeout() -> Duration {
        // Zero so a test never blocks: it asserts the pending/in-flight bookkeeping,
        // not the wait behaviour.
        Duration::from_millis(0)
    }

    /// **The counterexample.** A re-marked in-flight domain must not be handed to a
    /// second worker.
    ///
    /// Before the in-flight set existed, `take` removed the domain from `dirty` and
    /// recorded nothing, so a re-mark put it straight back and another worker took
    /// it. Repeating that fills every worker with one slow domain — which is
    /// exactly the "one domain occupies all workers" failure the module exists to
    /// prevent, and a thread cap alone does not rule it out.
    #[test]
    fn a_remark_while_in_flight_does_not_yield_a_second_worker() {
        let pending = PendingDomains::default();
        pending.mark([ConfigModule::Ui]);

        // Worker one takes the domain and is now really re-reading it.
        assert_eq!(pending.take(timeout()), Some(ConfigModule::Ui));

        // The notifier sees more writes to the same file while that is happening.
        for _ in 0..(WATCH_WORKERS * 4) {
            pending.mark([ConfigModule::Ui]);
        }

        // No other worker can obtain the same domain: nothing is pending, because
        // the only change is queued as a follow-up for the worker that owns it.
        assert_eq!(
            pending.take(timeout()),
            None,
            "a domain already in flight must not be handed to a second worker"
        );

        // When the owner finishes, the merged change is handled — once, by the
        // next take — so coalescing delays an update rather than losing it.
        pending.finished(ConfigModule::Ui);
        assert_eq!(
            pending.take(timeout()),
            Some(ConfigModule::Ui),
            "a change that arrived while the domain was in flight must not be lost"
        );
    }

    /// A slow domain must not be able to occupy every worker.
    ///
    /// This is the property that makes the bounding real: with one domain stuck in
    /// flight, the *other* domains are still handed out.
    #[test]
    fn one_stuck_domain_does_not_starve_the_others() {
        let pending = PendingDomains::default();
        let stuck = ConfigModule::Ui;
        pending.mark([stuck]);
        assert_eq!(pending.take(timeout()), Some(stuck));

        // A storm of events for the stuck domain, plus one event for another.
        for _ in 0..(WATCH_WORKERS * 8) {
            pending.mark([stuck]);
        }
        pending.mark([ConfigModule::Diagnostics]);

        assert_eq!(
            pending.take(timeout()),
            Some(ConfigModule::Diagnostics),
            "an unrelated domain must still be handed out while one is stuck"
        );
    }

    /// Coalescing is per domain and bounded by the domain count.
    #[test]
    fn marking_the_same_domain_many_times_coalesces_to_one() {
        let pending = PendingDomains::default();
        for _ in 0..64 {
            pending.mark([ConfigModule::Ui]);
        }
        assert_eq!(pending.take(timeout()), Some(ConfigModule::Ui));
        assert_eq!(
            pending.take(timeout()),
            None,
            "a burst of events for one file collapses into one reload"
        );
    }

    /// Finishing a domain with no intervening change must not re-queue it.
    #[test]
    fn finishing_without_a_change_does_not_requeue() {
        let pending = PendingDomains::default();
        pending.mark([ConfigModule::Ui]);
        assert_eq!(pending.take(timeout()), Some(ConfigModule::Ui));
        pending.finished(ConfigModule::Ui);
        assert_eq!(
            pending.take(timeout()),
            None,
            "a completed reload with no further change must not run again"
        );
    }
}
