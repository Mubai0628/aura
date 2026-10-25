//! Work nobody is waiting for, such as reading every
//! page of the Store to check it, runs with background CPU and I/O priority, so it never slows the
//! start or what the user does next.
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN, THREAD_PRIORITY_BELOW_NORMAL,
};

/// Lowers the calling thread to background CPU and I/O priority for the rest of its life; false if
/// Windows refused (the work then runs at normal priority).
#[must_use]
pub fn enter_background_mode() -> bool {
    // SAFETY: `GetCurrentThread` returns the calling thread's pseudo-handle, valid for this call.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) != 0 }
}

/// Lowers the calling thread's CPU priority below normal for the rest of its life, its I/O
/// staying at normal priority: for short work the user may soon wait for, such as reading the
/// Skill folders once at start; false if Windows refused.
#[must_use]
pub fn lower_cpu_priority() -> bool {
    // SAFETY: `GetCurrentThread` returns the calling thread's pseudo-handle, valid for this call.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) != 0 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_thread_can_enter_background_mode() {
        assert!(std::thread::spawn(super::enter_background_mode).join().expect("thread"));
    }

    #[test]
    fn a_thread_can_lower_its_cpu_priority() {
        assert!(std::thread::spawn(super::lower_cpu_priority).join().expect("thread"));
    }
}
