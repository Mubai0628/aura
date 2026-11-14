//! Windows the shell keeps in its own z-order bands, above every program's windows: Start, Search,
//! notifications, and the system's dialogs with the dimming behind them. `EnumWindows` leaves them
//! out; `FindWindowExW` walks them with the rest.
use super::window_list;
use aura_desktop_domain::Rect;
use std::collections::HashSet;
use windows::Win32::Foundation::{HWND, SetLastError, WIN32_ERROR};
use windows::Win32::UI::WindowsAndMessaging::FindWindowExW;
use windows::core::PCWSTR;

/// A walk restarts when a window it stood on closes before the next step.
const WALKS: usize = 3;

/// Where the visible, uncloaked top-level windows that `EnumWindows` leaves out are. They belong
/// to the shell or the system, never to a program the user let Aura operate, and they are in front
/// of every window it lists. When windows keep closing under every walk, the whole screen.
#[must_use]
pub fn covering() -> Vec<Rect> {
    (0..WALKS).find_map(|_| walk()).unwrap_or_else(|| vec![super::screen::virtual_screen()])
}

fn walk() -> Option<Vec<Rect>> {
    let listed: HashSet<isize> =
        window_list::listed().into_iter().map(|handle| handle.0 as isize).collect();
    let mut rects = Vec::new();
    let mut after: Option<HWND> = None;
    loop {
        // SAFETY: plain calls; `after` is a window this walk returned, and one that has closed
        // since makes the step fail with ERROR_INVALID_WINDOW_HANDLE.
        let step = unsafe {
            SetLastError(WIN32_ERROR(0));
            FindWindowExW(None, after, PCWSTR::null(), PCWSTR::null())
        };
        let handle = match step {
            Ok(handle) => handle,
            // The end of the list leaves the last error at zero; a window closed under the walk
            // leaves ERROR_INVALID_WINDOW_HANDLE, and any other failure is taken the same way.
            Err(error) if error.code().is_ok() => return Some(rects),
            Err(_) => return None,
        };
        after = Some(handle);
        if listed.contains(&(handle.0 as isize)) || !window_list::visible(handle) {
            continue;
        }
        let rect = window_list::bounds(handle);
        if rect.width > 0 && rect.height > 0 {
            rects.push(rect);
        }
    }
}
