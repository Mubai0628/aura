//! The user's clicks into the picture while a turn keeps the child session's desktop: a
//! low-level mouse hook of the window's thread, set only while it watches. A click of the user's
//! own (not one a program sent) on the picture, where no other window covers it, is kept from the
//! child session and tells the window instead: it is the user taking the desktop over.
use super::window::CLICKED;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GA_ROOT, GetAncestor, GetWindowRect, HC_ACTION, HHOOK, LLMHF_INJECTED,
    MSLLHOOKSTRUCT, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL,
    WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_XBUTTONDOWN, WindowFromPoint,
};

static HOOK: AtomicIsize = AtomicIsize::new(0);
static MAIN: AtomicIsize = AtomicIsize::new(0);
static PICTURE: AtomicIsize = AtomicIsize::new(0);

fn window(slot: &AtomicIsize) -> HWND {
    HWND(slot.load(Ordering::SeqCst) as *mut _)
}

/// Starts watching the clicks on `picture`, a child of `main`.
pub(crate) fn watch(main: HWND, picture: HWND) -> bool {
    MAIN.store(main.0 as isize, Ordering::SeqCst);
    PICTURE.store(picture.0 as isize, Ordering::SeqCst);
    if HOOK.load(Ordering::SeqCst) != 0 {
        return true;
    }
    // SAFETY: a low-level hook runs on this thread, which pumps messages; it is removed by
    // `unwatch` before the window goes.
    if let Ok(hook) = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(procedure), None, 0) } {
        HOOK.store(hook.0 as isize, Ordering::SeqCst);
        return true;
    }
    false
}

pub(crate) fn unwatch() {
    let hook = HOOK.swap(0, Ordering::SeqCst);
    if hook != 0 {
        // SAFETY: the hook was set by `watch` and is removed once.
        let _ = unsafe { UnhookWindowsHookEx(HHOOK(hook as *mut _)) };
    }
}

fn on_picture(point: POINT) -> bool {
    let (main, picture) = (window(&MAIN), window(&PICTURE));
    let mut area = RECT::default();
    // SAFETY: reads where our own windows are, and which window is under the point.
    unsafe {
        if GetWindowRect(picture, &raw mut area).is_err() {
            return false;
        }
        let inside = point.x >= area.left
            && point.x < area.right
            && point.y >= area.top
            && point.y < area.bottom;
        inside && GetAncestor(WindowFromPoint(point), GA_ROOT) == main
    }
}

unsafe extern "system" fn procedure(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && matches!(
            wparam.0 as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        )
    {
        // SAFETY: for a low-level mouse hook, `lparam` points to the event's MSLLHOOKSTRUCT.
        let event = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        if event.flags & LLMHF_INJECTED == 0 && on_picture(event.pt) {
            // SAFETY: a plain message to our own window.
            let _ = unsafe { PostMessageW(Some(window(&MAIN)), CLICKED, WPARAM(0), LPARAM(0)) };
            return LRESULT(1);
        }
    }
    // SAFETY: passes the event on as Windows asks of every hook.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
