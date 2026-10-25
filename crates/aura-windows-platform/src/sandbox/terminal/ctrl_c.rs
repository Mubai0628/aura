//! ConPTY turns an input 0x03 into a console Ctrl+C event for the programs it hosts. A child
//! ignores that event when it inherits the "ignore Ctrl+C" flag, which Windows passes on from a
//! parent started in a new process group or from one that set the flag itself. Core clears the
//! flag before each terminal launch and keeps its own handler, so Core never exits on Ctrl+C.
use std::{io, sync::OnceLock};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};

unsafe extern "system" fn keep_core_running(event: u32) -> i32 {
    // Close, logoff and shutdown keep their default handling.
    i32::from(event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT)
}

pub(super) fn let_terminal_programs_receive_ctrl_c() -> io::Result<()> {
    static HANDLER: OnceLock<Option<i32>> = OnceLock::new();
    // SAFETY: a static function registered once for the life of the process.
    let failed = *HANDLER.get_or_init(|| {
        (unsafe { SetConsoleCtrlHandler(Some(keep_core_running), 1) } == 0)
            .then(|| io::Error::last_os_error().raw_os_error().unwrap_or(0))
    });
    if let Some(code) = failed {
        return Err(io::Error::from_raw_os_error(code));
    }
    // On every launch: the flag is process-wide and can be set again after an earlier launch.
    // SAFETY: a NULL handler only changes this process's inheritable ignore flag.
    if unsafe { SetConsoleCtrlHandler(None, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Test support: the state a Core inherits when its parent ignores Ctrl+C.
#[cfg(any(test, feature = "compat-adoption"))]
pub(super) fn set_ignored(ignore: bool) -> io::Result<()> {
    // SAFETY: a NULL handler only changes this process's inheritable ignore flag.
    if unsafe { SetConsoleCtrlHandler(None, i32::from(ignore)) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
