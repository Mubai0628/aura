//! Every call into Windows that needs `unsafe`.
pub(crate) mod hook;
pub(crate) mod pipe;
pub(crate) mod rdp;
pub(crate) mod session;
pub(crate) mod window;

/// Readies this thread for hosting a control: OLE in a single-threaded apartment.
pub(crate) fn ole() -> windows::core::Result<()> {
    // SAFETY: called once, on the window's thread, before any control is made.
    unsafe { windows::Win32::System::Ole::OleInitialize(None) }
}
