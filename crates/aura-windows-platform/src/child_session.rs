//! The user's Windows child session: a second session of the same user on this computer, which a
//! Remote Desktop client opens with `ConnectToChildSession`. Windows allows one at a time.
use std::io;

use windows_sys::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTSGetChildSessionId, WTSIsChildSessionsEnabled, WTSLogoffSession,
};

/// The child session of the session this process runs in, while it exists.
#[must_use]
pub fn child_session_id() -> Option<u32> {
    let mut id = 0_u32;
    // SAFETY: `id` is writable storage for the duration of the call.
    let found = unsafe { WTSGetChildSessionId(&raw mut id) } != 0;
    // Windows reports a missing child session as 0 or as all bits set, depending on the build.
    (found && id != 0 && id != u32::MAX).then_some(id)
}

/// Whether Windows lets this user open a child session.
#[must_use]
pub fn child_sessions_enabled() -> bool {
    let mut enabled = 0;
    // SAFETY: `enabled` is writable storage for the duration of the call.
    let answered = unsafe { WTSIsChildSessionsEnabled(&raw mut enabled) } != 0;
    answered && enabled != 0
}

/// Signs session `id` out and waits until it has: every program in it ends.
pub fn log_off_session(id: u32) -> io::Result<()> {
    // SAFETY: the local server's pseudo handle needs no closing; `id` is only a number.
    if unsafe { WTSLogoffSession(WTS_CURRENT_SERVER_HANDLE, id, 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
