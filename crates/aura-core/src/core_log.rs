//! Core's own diagnostics, `logs/core.jsonl` under `LOCALAPPDATA`.
use super::*;

/// The Core process has started: startup stages count from here, and the records the backend
/// keeps about the user's own programs and about its installation are written to this log from now on.
pub(crate) fn start() {
    let _ = STARTUP_STARTED.set(Instant::now());
    aura_backend_windows_user::checked_program::route_log(|event, fields| {
        append_core_log(event, fields);
    });
    route_installation_log();
}

/// The backend writes its installation records wherever Core's own diagnostics go: under
/// `LOCALAPPDATA` in production, and in a test build only into a test's own route.
fn route_installation_log() {
    aura_backend_windows_user::installation::route_log(core_log_path);
}

/// Start mark for the startup stage timings in `logs/core.jsonl`.
static STARTUP_STARTED: OnceLock<Instant> = OnceLock::new();

/// Records one startup boundary with its cost since the Core process started.
pub(crate) fn append_startup_stage(stage: &str) {
    let elapsed_ms =
        STARTUP_STARTED.get().map_or(0, |started| started.elapsed().as_millis() as u64);
    append_core_log("startup_stage", json!({ "stage": stage, "elapsed_ms": elapsed_ms }));
}

#[cfg(test)]
use std::sync::{Condvar, PoisonError};

/// The route in force, if a test holds one (see [`CoreLogRoute`]).
#[cfg(test)]
static CORE_LOG_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
/// Signalled when a route ends, for a test waiting to take its own.
#[cfg(test)]
static CORE_LOG_ROUTE_ENDED: Condvar = Condvar::new();

/// One test's route for Core diagnostics and the backend's installation records, in force until
/// this is dropped. Production never routes them and keeps writing under `LOCALAPPDATA`.
///
/// The test binary has one route slot, so a route is exclusive: a test that routes the log while
/// another test's route is in force waits here until that route ends, instead of re-routing or
/// clearing it under that test. Tests that never route the log are not held back. No lock is
/// held while the route is in force, so an async test can keep it across `.await`. A test takes
/// one route at a time; a second one in the same test would wait for itself.
#[cfg(test)]
#[must_use = "the route ends as soon as this is dropped"]
pub(crate) struct CoreLogRoute(());

#[cfg(test)]
impl CoreLogRoute {
    pub(crate) fn to(path: PathBuf) -> Self {
        // Tests never call `start()`, so taking a route is what points installation records at
        // Core's log.
        route_installation_log();
        let slot = CORE_LOG_OVERRIDE.lock().unwrap_or_else(PoisonError::into_inner);
        let mut slot = CORE_LOG_ROUTE_ENDED
            .wait_while(slot, |route| route.is_some())
            .unwrap_or_else(PoisonError::into_inner);
        *slot = Some(path);
        Self(())
    }
}

#[cfg(test)]
impl Drop for CoreLogRoute {
    fn drop(&mut self) {
        *CORE_LOG_OVERRIDE.lock().unwrap_or_else(PoisonError::into_inner) = None;
        CORE_LOG_ROUTE_ENDED.notify_one();
    }
}

pub(crate) fn core_log_path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        // A test with no explicit route must never write to the user's real profile.
        CORE_LOG_OVERRIDE.lock().ok().and_then(|slot| slot.clone())
    }
    #[cfg(not(test))]
    {
        let local = env::var_os("LOCALAPPDATA")?;
        Some(Path::new(&local).join("Aura").join("logs").join("core.jsonl"))
    }
}

pub(crate) fn append_core_log(event: &str, fields: Value) {
    let Some(path) = core_log_path() else {
        return;
    };
    let _ = write_core_log_line(&path, event, fields);
}

pub(crate) fn write_core_log_line(path: &Path, event: &str, fields: Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let entry = json!({
        "timestamp": UtcTimestamp::now(),
        "component": "core",
        "event": event,
        "fields": fields,
    });
    let mut bytes = serde_json::to_vec(&entry)?;
    bytes.push(b'\n');
    use std::io::Write;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests;
