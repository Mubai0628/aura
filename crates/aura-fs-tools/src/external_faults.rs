//! A deliberate interleaving point inside an external publication, for tests only.
//!
//! The window this exists to make observable is the one between the file layer's **last**
//! observation of the destination and the moment it publishes. Two callers that reach that
//! point at the same time both see the bytes they expect; only the one that publishes first
//! leaves them true. Whether that window is really closed cannot be shown by running two
//! writers in parallel and hoping: the calls have to be *placed* on either side of it.
//!
//! The hook is installed by a test, names the one destination it applies to, and is a no-op
//! in every other build — so no product path changes behavior because of it, and publications
//! to any other file are untouched. What it must not do is hold anything: the test owns the
//! synchronisation, and the hook only reports that it arrived.

use std::sync::{Arc, Mutex, OnceLock};

type FaultHook = Arc<dyn Fn(&str) + Send + Sync>;

/// A point a test can hold one external publication at.
#[derive(Default)]
pub struct ExternalPublishRendezvous {
    /// Reached after the caller has read the destination and before the publication makes its
    /// own final checks of it.
    ///
    /// This is the caller-visible window: what a test writes to the destination here is what
    /// the final checks are supposed to notice.
    pub before_final_checks: Option<FaultHook>,
    /// Reached after the final checks passed and before the platform is asked to publish.
    pub before_platform_publish: Option<FaultHook>,
}

/// One installed hook and the destination it applies to.
struct Installed {
    display_path: String,
    hook: Arc<ExternalPublishRendezvous>,
}

static HOOK: OnceLock<Mutex<Option<Installed>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Installed>> {
    HOOK.get_or_init(|| Mutex::new(None))
}

/// Taken by each installed hook for as long as it is installed.
static TURN: Mutex<()> = Mutex::new(());

/// Installs the hook for one destination, returning a guard that removes it when it drops.
///
/// The callbacks only fire for publications whose display path matches, so a test that holds
/// a publication open cannot also stall unrelated tests running beside it in the same binary.
/// A second test that installs a hook waits until the first one's guard drops.
pub fn install(display_path: &str, hook: ExternalPublishRendezvous) -> ExternalPublishHookGuard {
    let turn = TURN.lock().unwrap_or_else(|error| error.into_inner());
    let mut slot = slot().lock().unwrap_or_else(|error| error.into_inner());
    assert!(slot.is_none(), "an external publish hook is already installed");
    *slot = Some(Installed { display_path: display_path.to_owned(), hook: Arc::new(hook) });
    ExternalPublishHookGuard { _turn: turn }
}

/// Removes the installed hook when it goes out of scope, so a failing test cannot leave the
/// product path instrumented for the next one.
pub struct ExternalPublishHookGuard {
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Drop for ExternalPublishHookGuard {
    fn drop(&mut self) {
        *slot().lock().unwrap_or_else(|error| error.into_inner()) = None;
    }
}

fn hook_for(display_path: &str) -> Option<Arc<ExternalPublishRendezvous>> {
    let installed = slot().lock().unwrap_or_else(|error| error.into_inner());
    installed
        .as_ref()
        .filter(|installed| installed.display_path == display_path)
        .map(|installed| Arc::clone(&installed.hook))
}

pub(crate) fn reached_before_final_checks(display_path: &str) {
    if let Some(hook) = hook_for(display_path)
        && let Some(callback) = hook.before_final_checks.as_ref()
    {
        callback(display_path);
    }
}

pub(crate) fn reached_before_platform_publish(display_path: &str) {
    if let Some(hook) = hook_for(display_path)
        && let Some(callback) = hook.before_platform_publish.as_ref()
    {
        callback(display_path);
    }
}
