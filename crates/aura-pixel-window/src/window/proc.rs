//! The window procedure. The state is borrowed only for the length of one message, and never
//! while Windows may send another (capture, destruction), so a message that arrives meanwhile
//! goes to the default handling.
use std::{cell::RefCell, rc::Rc};

use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect},
    UI::{
        Accessibility::{UiaReturnRawElementProvider, UiaRootObjectId},
        Controls::WM_MOUSELEAVE,
        Input::KeyboardAndMouse::{
            GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
            VIRTUAL_KEY, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE,
            VK_TAB, VK_UP,
        },
        WindowsAndMessaging::{
            DefWindowProcW, GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW, HTCAPTION, HTCLIENT,
            HTNOWHERE, IDC_HAND, LoadCursorW, SetCursor, SetWindowLongPtrW, WM_CAPTURECHANGED,
            WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_GETOBJECT, WM_KEYDOWN, WM_LBUTTONDOWN,
            WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOVE, WM_NCDESTROY, WM_NCHITTEST, WM_SETCURSOR,
            WM_TIMER,
        },
    },
};

use super::{State, WM_A11Y_FOCUS, WM_A11Y_INVOKE, WM_WORK_DONE};
use crate::{layout::Target, scale::fitting_scale};

pub(super) unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the pointer was set in `open` from an Rc the window keeps until WM_NCDESTROY,
    // where it is cleared and the count given back.
    unsafe {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<State>;
        if pointer.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        if message == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Rc::from_raw(pointer));
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        match handle(&*pointer, hwnd, message, wparam, lparam) {
            Some(result) => result,
            None => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

/// A point packed in a message's `lparam`, its halves signed.
fn point(lparam: LPARAM) -> (i32, i32) {
    (
        i32::from((lparam.0 & 0xffff) as u16 as i16),
        i32::from(((lparam.0 >> 16) & 0xffff) as u16 as i16),
    )
}

fn handle(
    cell: &RefCell<State>,
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    match message {
        WM_NCHITTEST => {
            let state = cell.try_borrow().ok()?;
            let (x, y) = point(lparam);
            Some(LRESULT(hit_test(&state, x - state.position.0, y - state.position.1) as isize))
        }
        WM_SETCURSOR => {
            let state = cell.try_borrow().ok()?;
            let mut cursor = POINT::default();
            // SAFETY: reads the pointer's position into a local.
            unsafe { GetCursorPos(&mut cursor).ok()? };
            let over = state.hit(cursor.x - state.position.0, cursor.y - state.position.1)?;
            if !matches!(state.laid.controls[over].target, Target::Link(_)) {
                return None;
            }
            // SAFETY: a system cursor, never destroyed.
            unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
            Some(LRESULT(1))
        }
        WM_MOUSEMOVE => {
            let mut state = cell.try_borrow_mut().ok()?;
            let (x, y) = point(lparam);
            let hover = state.hit(x, y);
            if hover != state.look.hover {
                state.look.hover = hover;
                state.paint();
            }
            if !state.tracking {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: asks for WM_MOUSELEAVE for this window.
                state.tracking = unsafe { TrackMouseEvent(&mut track) }.is_ok();
            }
            Some(LRESULT(0))
        }
        WM_MOUSELEAVE => {
            let mut state = cell.try_borrow_mut().ok()?;
            state.tracking = false;
            if state.look.hover.take().is_some() {
                state.paint();
            }
            Some(LRESULT(0))
        }
        WM_LBUTTONDOWN => {
            {
                let mut state = cell.try_borrow_mut().ok()?;
                let (x, y) = point(lparam);
                let index = state.hit(x, y).filter(|index| state.laid.controls[*index].enabled);
                state.look.pressed = index;
                state.look.cues = false;
                if let Some(index) = index.filter(|index| state.laid.controls[*index].tab_stop) {
                    state.look.focus = Some(index);
                }
                state.paint();
            }
            // SAFETY: captures the pointer for this window until the button comes up.
            unsafe { SetCapture(hwnd) };
            Some(LRESULT(0))
        }
        WM_LBUTTONUP => {
            {
                let mut state = cell.try_borrow_mut().ok()?;
                let (x, y) = point(lparam);
                let pressed = state.look.pressed.take();
                if let Some(index) = pressed.filter(|index| state.hit(x, y) == Some(*index)) {
                    state.activate(index);
                }
                state.paint();
            }
            // SAFETY: gives back the capture taken at the button's press.
            unsafe {
                let _ = ReleaseCapture();
            }
            Some(LRESULT(0))
        }
        WM_CAPTURECHANGED => {
            let mut state = cell.try_borrow_mut().ok()?;
            if state.look.pressed.take().is_some() {
                state.paint();
            }
            Some(LRESULT(0))
        }
        WM_KEYDOWN => {
            let mut state = cell.try_borrow_mut().ok()?;
            let repeat = (lparam.0 >> 30) & 1 == 1;
            match VIRTUAL_KEY(wparam.0 as u16) {
                // SAFETY: reads the Shift key's state as this thread last saw it.
                VK_TAB => state.move_focus(unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } >= 0),
                VK_LEFT | VK_UP => state.move_focus(false),
                VK_RIGHT | VK_DOWN => state.move_focus(true),
                VK_RETURN if !repeat => state.enter(),
                VK_SPACE if !repeat => {
                    if let Some(index) = state.look.focus {
                        state.activate(index);
                    }
                }
                VK_ESCAPE if !repeat => state.close(),
                _ => return None,
            }
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            cell.try_borrow_mut().ok()?.close();
            Some(LRESULT(0))
        }
        WM_TIMER => {
            let mut state = cell.try_borrow_mut().ok()?;
            state.look.phase = state.look.phase.wrapping_add(1);
            Some(LRESULT(0))
        }
        WM_MOVE => {
            let mut state = cell.try_borrow_mut().ok()?;
            state.position = point(lparam);
            state.a11y.moved(state.position);
            Some(LRESULT(0))
        }
        WM_DPICHANGED => {
            let mut state = cell.try_borrow_mut().ok()?;
            // SAFETY: Windows passes the suggested rectangle for the new DPI in `lparam`.
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            let mut info =
                MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            // SAFETY: plain queries into locals.
            unsafe {
                let _ = GetMonitorInfoW(
                    MonitorFromRect(&suggested, MONITOR_DEFAULTTONEAREST),
                    &mut info,
                );
            }
            let work = (info.rcWork.right - info.rcWork.left, info.rcWork.bottom - info.rcWork.top);
            state.dpi = (wparam.0 & 0xffff) as u32;
            state.k =
                fitting_scale(state.dpi, (state.laid.scene.width, state.laid.scene.height), work);
            state.position = (suggested.left, suggested.top);
            state.paint();
            Some(LRESULT(0))
        }
        WM_GETOBJECT if lparam.0 as i32 == UiaRootObjectId => {
            let root = cell.try_borrow().ok()?.a11y.root();
            // SAFETY: hands UI Automation the window's root provider for this request.
            Some(unsafe { UiaReturnRawElementProvider(hwnd, wparam, lparam, &root) })
        }
        WM_WORK_DONE => {
            cell.try_borrow_mut().ok()?.work_done = true;
            Some(LRESULT(0))
        }
        WM_A11Y_INVOKE | WM_A11Y_FOCUS => {
            let mut state = cell.try_borrow_mut().ok()?;
            if wparam.0 as u32 != state.a11y.generation() {
                return Some(LRESULT(0));
            }
            let index = lparam.0 as usize;
            if message == WM_A11Y_INVOKE {
                state.activate(index);
            } else {
                state.focus(index);
            }
            Some(LRESULT(0))
        }
        WM_DESTROY => {
            let shared = cell.try_borrow().ok().map(|state| std::rc::Rc::clone(&state.a11y));
            if let Some(shared) = shared {
                shared.disconnect();
            }
            None
        }
        _ => None,
    }
}

/// The title bar drags the window; the shadow takes no clicks.
fn hit_test(state: &State, x: i32, y: i32) -> u32 {
    let k = state.k;
    if x < 0 || y < 0 || x >= state.laid.scene.width * k || y >= state.laid.scene.height * k {
        return HTNOWHERE;
    }
    if state.hit(x, y).is_none() && state.laid.title_bar.contains(x / k, y / k) {
        return HTCAPTION;
    }
    HTCLIENT
}
