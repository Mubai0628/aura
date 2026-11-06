//! Which window is in front or under a point, bringing one to the front, and whether a process
//! runs with more rights than Aura's.
use aura_desktop_domain::Rect;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, POINT};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL,
    TOKEN_QUERY, TokenIntegrityLevel,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GA_ROOT, GetAncestor, GetForegroundWindow, GetWindowThreadProcessId,
    IsHungAppWindow, IsIconic, IsWindow, SW_RESTORE, SetForegroundWindow, ShowWindow,
    WindowFromPoint,
};

fn handle(window: isize) -> HWND {
    HWND(window as *mut _)
}

fn process_of(window: HWND) -> u32 {
    let mut process = 0_u32;
    // SAFETY: a query of a window handle; a window that is gone answers 0.
    unsafe { GetWindowThreadProcessId(window, Some(&raw mut process)) };
    process
}

/// The top-level window the user's input goes to now, and its process.
#[must_use]
pub fn foreground() -> Option<(isize, u32)> {
    // SAFETY: plain queries.
    let window = unsafe { GetForegroundWindow() };
    (!window.is_invalid()).then(|| (window.0 as isize, process_of(window)))
}

/// The top-level window under a screen point, and its process.
#[must_use]
pub fn root_at(x: i32, y: i32) -> Option<(isize, u32)> {
    // SAFETY: plain queries of the window under a point and its root.
    unsafe {
        let window = WindowFromPoint(POINT { x, y });
        if window.is_invalid() {
            return None;
        }
        let root = GetAncestor(window, GA_ROOT);
        let root = if root.is_invalid() { window } else { root };
        Some((root.0 as isize, process_of(root)))
    }
}

#[must_use]
pub fn exists(window: isize) -> bool {
    // SAFETY: a query of a window handle.
    unsafe { IsWindow(Some(handle(window))).as_bool() }
}

/// Whether Windows takes the window for not responding: its program has not taken a message
/// for about five seconds.
#[must_use]
pub fn hung(window: isize) -> bool {
    // SAFETY: a query of a window handle.
    unsafe { IsHungAppWindow(handle(window)).as_bool() }
}

#[must_use]
pub fn bounds(window: isize) -> Rect {
    super::window_list::bounds(handle(window))
}

/// Restores the window when it is minimized and brings it to the front. Windows lets only the
/// program the user is using take the front; joining its input queue for the call is the
/// accepted way for an assistant to do it on the user's behalf. True when it is in front.
pub fn bring_to_front(window: isize) -> bool {
    let target = handle(window);
    // SAFETY: calls on a window handle and on thread ids Windows gave; the input queues are
    // joined only for the duration of the calls and detached again.
    unsafe {
        if IsIconic(target).as_bool() {
            let _ = ShowWindow(target, SW_RESTORE);
        }
        if GetForegroundWindow() == target {
            return true;
        }
        let ours = GetCurrentThreadId();
        let front = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let joined = front != 0 && front != ours && AttachThreadInput(ours, front, true).as_bool();
        let _ = BringWindowToTop(target);
        let _ = SetForegroundWindow(target);
        if joined {
            let _ = AttachThreadInput(ours, front, false);
        }
        GetForegroundWindow() == target
    }
}

fn integrity(process: HANDLE) -> Option<u32> {
    // SAFETY: the token is opened for a query and closed here; the label is read inside the
    // buffer GetTokenInformation filled.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &raw mut token).ok()?;
        let mut needed = 0_u32;
        let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &raw mut needed);
        // Eight-byte units keep the label's pointer aligned.
        let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
        let read = GetTokenInformation(
            token,
            TokenIntegrityLevel,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &raw mut needed,
        );
        let _ = CloseHandle(token);
        read.ok()?;
        let label = &*(buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>());
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        (count > 0).then(|| *GetSidSubAuthority(label.Label.Sid, u32::from(count) - 1))
    }
}

/// Whether the process runs at a higher integrity level than this one (for example as
/// administrator). Windows drops simulated input for such a program without saying so. A
/// process whose level cannot be read counts as higher.
#[must_use]
pub fn above_us(process: u32) -> bool {
    // SAFETY: the current-process pseudo handle needs no closing; the other handle is opened for
    // a limited query and closed here.
    let (ours, theirs) = unsafe {
        let theirs =
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process).ok().and_then(|other| {
                let level = integrity(other);
                let _ = CloseHandle(other);
                level
            });
        (integrity(GetCurrentProcess()), theirs)
    };
    !matches!((ours, theirs), (Some(ours), Some(theirs)) if theirs <= ours)
}
