use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

/// Makes every coordinate this process reads or sends a physical pixel, on every monitor. Call
/// it before the first window call; a process that already chose keeps its choice.
pub fn per_monitor_aware() {
    // SAFETY: sets a process-wide flag; failing (already set) changes nothing.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}
