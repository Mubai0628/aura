//! The user's child session, as Windows reports it to the session that owns it.
use windows::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTSGetChildSessionId, WTSLogoffSession,
};

/// The child session of this session, while it exists.
pub(crate) fn child_session() -> Option<u32> {
    let mut id = 0_u32;
    // SAFETY: `id` is writable storage for the duration of the call.
    let found = unsafe { WTSGetChildSessionId(&raw mut id) }.as_bool();
    // Windows reports a missing child session as 0 or as all bits set, depending on the build.
    (found && id != 0 && id != u32::MAX).then_some(id)
}

/// Signs session `id` out and waits until it has.
pub(crate) fn sign_out(id: u32) -> bool {
    // SAFETY: the local server's pseudo handle needs no closing; `id` is only a number.
    unsafe { WTSLogoffSession(Some(WTS_CURRENT_SERVER_HANDLE), id, true) }.is_ok()
}
