//! The test seam at a publication's two disk-committed stages, `applying` and `published`.
//!
//! The publication calls [`reached`] at each. A test that holds or interrupts the process there
//! installs its function first; with none installed nothing happens, so a publication in a test
//! that installs nothing runs exactly as it does in the product.
use std::sync::RwLock;

static HOOK: RwLock<Option<fn(&'static str)>> = RwLock::new(None);

/// Installs the function the publication calls at each stage.
pub fn install(hook: fn(&'static str)) {
    if let Ok(mut slot) = HOOK.write() {
        *slot = Some(hook);
    }
}

/// Called by the publication at `stage`. The function runs after the lock is released, so a
/// test function that panics to simulate termination cannot poison it.
pub(super) fn reached(stage: &'static str) {
    let hook = HOOK.read().ok().and_then(|slot| *slot);
    if let Some(hook) = hook {
        hook(stage);
    }
}
