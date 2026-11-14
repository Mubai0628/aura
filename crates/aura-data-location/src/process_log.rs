//! Where this process's lines for Core's log go. Core and the Sandbox Host set it once they have
//! confirmed their locations; every crate that adds lines to Core's log reads it here. Until it
//! is set nothing is written, so a test that never sets it never writes into the user's profile.
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

static CORE_LOG: OnceLock<PathBuf> = OnceLock::new();

/// Sets the log once for the life of the process; a second, different path is refused and the
/// first one kept.
pub fn route_core_log(path: PathBuf) -> Result<(), PathBuf> {
    let routed = CORE_LOG.get_or_init(|| path.clone());
    if *routed == path { Ok(()) } else { Err(routed.clone()) }
}

#[must_use]
pub fn core_log_path() -> Option<&'static Path> {
    CORE_LOG.get().map(PathBuf::as_path)
}
