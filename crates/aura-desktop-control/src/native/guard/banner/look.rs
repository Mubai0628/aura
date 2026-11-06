//! What the banner looks at on the desktop: the screen of the program Aura operates, whether
//! another program's window covers the banner, and the banner's own window as tests see it.
use super::CLASS;
use crate::native::{focus, window_list};
use aura_desktop_domain::Rect;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GW_HWNDNEXT, GWL_EXSTYLE, GetForegroundWindow, GetTopWindow, GetWindow,
    GetWindowDisplayAffinity, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
    IsWindowVisible, WDA_EXCLUDEFROMCAPTURE, WS_EX_TRANSPARENT,
};

/// The most windows looked at above the banner.
const ABOVE_MAX: usize = 1024;

pub(super) fn handle(window: isize) -> HWND {
    HWND(window as *mut _)
}

fn rect_of(area: RECT) -> Rect {
    Rect {
        x: area.left,
        y: area.top,
        width: area.right - area.left,
        height: area.bottom - area.top,
    }
}

fn meets(one: Rect, other: Rect) -> bool {
    one.x < other.x + other.width
        && other.x < one.x + one.width
        && one.y < other.y + other.height
        && other.y < one.y + one.height
}

/// The work area and scale of the screen of `front`, the program's window, else of the window in
/// front, else of the main screen.
pub(super) fn screen_of(front: Option<isize>) -> (Rect, f32) {
    // SAFETY: plain queries of windows and monitors into locals.
    unsafe {
        let window = front.filter(|window| focus::exists(*window)).map(handle).or_else(|| {
            let window = GetForegroundWindow();
            (!window.is_invalid()).then_some(window)
        });
        let monitor = match window {
            Some(window) => MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
            None => MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
        };
        let mut info =
            MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let area = if GetMonitorInfoW(monitor, &raw mut info).as_bool() {
            rect_of(info.rcWork)
        } else {
            crate::native::screen::virtual_screen()
        };
        let (mut dpi, mut unused) = (96_u32, 96_u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut dpi, &raw mut unused);
        (area, dpi as f32 / 96.0)
    }
}

/// Whether a window of another program covers part of `rect` above the banner: a click-through
/// overlay does not.
fn covers(window: HWND, rect: Rect) -> bool {
    // SAFETY: queries of a window handle; a window that is gone answers 0 or fails.
    unsafe {
        let style = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
        let mut process = 0_u32;
        GetWindowThreadProcessId(window, Some(&raw mut process));
        style & WS_EX_TRANSPARENT.0 == 0
            && process != std::process::id()
            && window_list::visible(window)
            && meets(window_list::bounds(window), rect)
    }
}

/// Whether no other program's window covers the banner.
pub(super) fn on_top(window: HWND) -> bool {
    // SAFETY: plain queries of the windows' order, front to back.
    unsafe {
        let mut place = RECT::default();
        if GetWindowRect(window, &raw mut place).is_err() {
            return false;
        }
        let rect = rect_of(place);
        let mut next = GetTopWindow(None);
        for _ in 0..ABOVE_MAX {
            let Ok(above) = next else { return false };
            if above == window {
                return true;
            }
            if covers(above, rect) {
                return false;
            }
            next = GetWindow(above, GW_HWNDNEXT);
        }
        false
    }
}

/// Whether the banner's window shows, and whether captures leave it out.
#[must_use]
pub fn banner_status() -> Option<(bool, bool)> {
    // SAFETY: queries only the guard's known window class; no other window content is read.
    unsafe {
        let window = FindWindowW(CLASS, None).ok()?;
        let mut affinity = 0;
        GetWindowDisplayAffinity(window, &raw mut affinity).ok()?;
        Some((IsWindowVisible(window).as_bool(), affinity == WDA_EXCLUDEFROMCAPTURE.0))
    }
}

/// Where the banner shows, its shadow included; `None` while it does not.
#[must_use]
pub fn banner_place() -> Option<Rect> {
    // SAFETY: queries only the guard's known window class.
    unsafe {
        let window = FindWindowW(CLASS, None).ok()?;
        if !IsWindowVisible(window).as_bool() {
            return None;
        }
        let mut place = RECT::default();
        GetWindowRect(window, &raw mut place).ok()?;
        Some(rect_of(place))
    }
}

/// Whether the banner shows and no other program's window covers it.
#[must_use]
pub fn banner_on_top() -> bool {
    // SAFETY: queries only the guard's known window class and the windows above it.
    unsafe {
        FindWindowW(CLASS, None)
            .ok()
            .is_some_and(|window| IsWindowVisible(window).as_bool() && on_top(window))
    }
}
