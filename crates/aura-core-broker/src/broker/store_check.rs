//! After a normal exit the Store opens with only its
//! structure checked, and SQLite's quick check runs on its own thread while the Store serves; a
//! failed check withholds the next normal exit.
//!
//! The quick check reads every page (184 s on a 7.3 GB Store) and slowed the start it was meant
//! to spare, so it waits until the start is over and runs with background priority, and after a
//! normal exit it runs at most once a week: a passed check is recorded beside the Store. A check
//! still running at quit withholds nothing: the next start, after this normal exit, runs it again.
//!
//! Two faults made the owner's development
//! Aura start over and over. A quit while the quick check ran left no normal exit: the check held
//! the write-ahead log, the exit's checkpoint waited for it, and Main stops a Core that has not
//! answered in three seconds. The next start then checked every page before serving (154 s on the
//! 7.1 GB Store), past the two minutes Main waits for Core. Now the check is stopped first when Core
//! exits, and the full check an unannounced end leaves owed runs here, in the background, too; the
//! normal exit is recorded only once it has passed, so a quit before then leaves it owed to the
//! next start. Damage either check finds is in Core's status.
use aura_core_base::append_core_log;
use aura_persistence_core::schema::{PendingCheck, background_check};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

const SOUND: u8 = 0;
/// The quick check is running; a quit stops it and the next start runs it again.
const CHECKING: u8 = 1;
const FAILED: u8 = 2;
/// The full check is owed; until it has passed no normal exit is recorded.
const OWED: u8 = 3;

/// Long enough for Core to have started and the window to have loaded what it shows first.
const CHECK_DELAY: Duration = Duration::from_secs(20);

/// What stops a running check when Core exits; the check asks it as it reads.
#[derive(Default)]
pub(super) struct Stop(AtomicBool);

impl Stop {
    pub(super) fn requested(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(super) struct StoreCheck {
    state: Arc<AtomicU8>,
    stop: Arc<Stop>,
}

impl StoreCheck {
    /// Whatever the Store's open left to check, on its own thread once the start is over.
    pub(super) fn start(pending: PendingCheck, database: PathBuf) -> Self {
        Self::start_after(pending, database, CHECK_DELAY)
    }

    fn start_after(pending: PendingCheck, database: PathBuf, delay: Duration) -> Self {
        Self::start_with(pending, delay, move |stop| {
            let checked = background_check(&database, pending, move || stop.requested())
                .map_err(|error| error.to_string());
            // A passed check spares the quick check of the next week's starts; a record that
            // cannot be written only means the next start checks again.
            if checked.is_ok() {
                let _ = aura_persistence_v2::record_store_checked(&database);
            }
            checked
        })
    }

    fn start_with(
        pending: PendingCheck,
        delay: Duration,
        check: impl FnOnce(Arc<Stop>) -> Result<(), String> + Send + 'static,
    ) -> Self {
        let (initial, owed) = match pending {
            PendingCheck::None => (SOUND, "none"),
            PendingCheck::Quick => (CHECKING, "quick"),
            PendingCheck::Full => (OWED, "full"),
        };
        let state = Arc::new(AtomicU8::new(initial));
        let stop = Arc::new(Stop::default());
        append_core_log("store_open_check", json!({ "check": "structure", "owed": owed }));
        if pending != PendingCheck::None {
            let shared = Arc::clone(&state);
            let stopping = Arc::clone(&stop);
            let event = if pending == PendingCheck::Full {
                "store_full_check"
            } else {
                "store_quick_check"
            };
            // A thread that cannot start leaves the check where it was: owed, or run next time.
            let _ =
                std::thread::Builder::new().name("aura-store-check".to_owned()).spawn(move || {
                    std::thread::sleep(delay);
                    if stopping.requested() {
                        return;
                    }
                    let background = aura_windows_platform::enter_background_mode();
                    let started = std::time::Instant::now();
                    let checked = check(Arc::clone(&stopping));
                    // Stopped for the exit, not damage: the check stays owed, or runs next time.
                    if checked.is_err() && stopping.requested() {
                        append_core_log(event, json!({ "stopped": true }));
                        return;
                    }
                    shared.store(if checked.is_ok() { SOUND } else { FAILED }, Ordering::Release);
                    append_core_log(
                        event,
                        json!({
                            "ok": checked.is_ok(),
                            "ms": started.elapsed().as_millis() as u64,
                            "background": background,
                            "error": checked.err(),
                        }),
                    );
                });
        }
        Self { state, stop }
    }

    /// Stops a running check, then records a normal exit for `store` unless a check found damage
    /// or the full check is still owed; says whether it did.
    pub(super) fn record_exit(&self, store: &aura_persistence_v2::V2Store) -> anyhow::Result<bool> {
        self.stop.stop();
        let recorded = matches!(self.state.load(Ordering::Acquire), SOUND | CHECKING);
        if recorded {
            store.mark_clean_exit()?;
        }
        Ok(recorded)
    }

    /// A check found the Store damaged.
    pub(super) fn damaged(&self) -> bool {
        self.state.load(Ordering::Acquire) == FAILED
    }
}

#[cfg(test)]
#[path = "store_check_tests.rs"]
mod tests;
