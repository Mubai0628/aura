//! A rename through a held handle that another program briefly blocks.
//!
//! Windows refuses to rename a folder while another program holds a file inside it open (a virus
//! scanner or the search indexer reading what just arrived there, typically) and reports access
//! denied or a sharing violation until that program lets go, usually within moments. A move, or
//! the undo of one, that gave up at once would leave the workspace needing reconciliation for a
//! lock that was about to go, so the rename is tried again for a short while first. The object
//! stays held without sharing throughout and every try is the same handle-bound rename, so a retry
//! cannot move another object.
use std::{
    ffi::OsStr,
    time::{Duration, Instant},
};

use aura_windows_platform::{HandlePublishError, VerifiedFile, handle_bound_move_existing};

/// How long a blocked rename keeps being tried.
const RETRY_FOR: Duration = Duration::from_secs(2);
/// The pause between tries.
const RETRY_EVERY: Duration = Duration::from_millis(50);
const STATUS_ACCESS_DENIED: u32 = 0xC000_0022;
const STATUS_SHARING_VIOLATION: u32 = 0xC000_0043;

/// [`handle_bound_move_existing`], tried again while another program briefly blocks the rename.
pub(crate) fn move_held(
    held: &VerifiedFile,
    parent: &VerifiedFile,
    name: &OsStr,
) -> Result<(), HandlePublishError> {
    let until = Instant::now() + RETRY_FOR;
    loop {
        match handle_bound_move_existing(held, parent, name) {
            Err(HandlePublishError::NativeRename { ntstatus, .. })
                if matches!(ntstatus, STATUS_ACCESS_DENIED | STATUS_SHARING_VIOLATION)
                    && Instant::now() < until =>
            {
                std::thread::sleep(RETRY_EVERY);
            }
            result => return result,
        }
    }
}

#[cfg(test)]
#[path = "held_rename_tests.rs"]
mod tests;
