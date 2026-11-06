//! A window's size and title-bar buttons, and asking it to change size or to close. Each request
//! is posted to the window's own thread: nothing here waits for the program.
use crate::arrange::{Buttons, Shown};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, IsIconic, IsZoomed, PostMessageW, SC_CLOSE, SHOW_WINDOW_CMD,
    SW_MAXIMIZE, SW_MINIMIZE, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, ShowWindowAsync,
    WM_SYSCOMMAND, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
};

fn handle(window: isize) -> HWND {
    HWND(window as *mut _)
}

#[must_use]
pub fn shown(window: isize) -> Shown {
    // SAFETY: queries of a window handle; a window that is gone answers false.
    unsafe {
        Shown {
            minimized: IsIconic(handle(window)).as_bool(),
            maximized: IsZoomed(handle(window)).as_bool(),
        }
    }
}

#[must_use]
pub fn buttons(window: isize) -> Buttons {
    // SAFETY: a query of a window handle's style.
    let style = unsafe { GetWindowLongPtrW(handle(window), GWL_STYLE) } as u32;
    Buttons { minimize: style & WS_MINIMIZEBOX.0 != 0, maximize: style & WS_MAXIMIZEBOX.0 != 0 }
}

fn post(window: isize, how: SHOW_WINDOW_CMD) -> bool {
    // SAFETY: posts a show request to the window's thread; a window that is gone answers false.
    unsafe { ShowWindowAsync(handle(window), how).as_bool() }
}

/// As its minimize button does, or, `behind`, leaving the window in front where it is.
pub fn minimize(window: isize, behind: bool) -> bool {
    post(window, if behind { SW_SHOWMINNOACTIVE } else { SW_MINIMIZE })
}

pub fn maximize(window: isize) -> bool {
    post(window, SW_MAXIMIZE)
}

/// Its normal size, without bringing it to the front.
pub fn normal(window: isize) -> bool {
    post(window, SW_SHOWNOACTIVATE)
}

/// Asks it to close as its close button does; true when the request was posted.
pub fn close(window: isize) -> bool {
    // SAFETY: posts the close command to the window's thread; nothing waits for the program.
    unsafe {
        PostMessageW(Some(handle(window)), WM_SYSCOMMAND, WPARAM(SC_CLOSE as usize), LPARAM(0))
            .is_ok()
    }
}
