//! The window's own menu (`--menu`), drawn the way some programs draw theirs: a window of its
//! own that a right click opens where it was clicked, that takes the front, and that closes as
//! soon as another window takes the front. Down moves to the next of its two items, Enter
//! chooses the one moved to, Escape closes the menu. Logs `menu open` once it shows, then
//! `menu <item>` (from 1) for the item chosen or `menu closed`.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, SetForegroundWindow,
    WA_INACTIVE, WM_ACTIVATE, WM_KEYDOWN, WNDCLASSEXW, WS_BORDER, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP, WS_VISIBLE,
};
use windows::core::w;

pub(super) static ENABLED: AtomicBool = AtomicBool::new(false);
static OPEN: AtomicBool = AtomicBool::new(false);
/// The item moved to, from 1; 0 before Down is pressed.
static ITEM: AtomicU32 = AtomicU32::new(0);
const ITEMS: u32 = 2;
const WIDTH: i32 = 160;
const HEIGHT: i32 = 120;

/// Opens the menu with its top left at the screen point, owned by the window.
///
/// # Safety
/// Called on the window's own thread, from its window procedure.
pub(super) unsafe fn open(owner: HWND, at: POINT) {
    if OPEN.swap(true, Ordering::Relaxed) {
        return;
    }
    ITEM.store(0, Ordering::Relaxed);
    // SAFETY: creates a window of this thread; a class registered already fails harmlessly.
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut _),
            lpszClassName: w!("AuraDesktopFixtureMenu"),
            ..Default::default()
        };
        RegisterClassExW(&raw const class);
        let Ok(menu) = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            w!("AuraDesktopFixtureMenu"),
            w!(""),
            WS_POPUP | WS_BORDER | WS_VISIBLE,
            at.x,
            at.y,
            WIDTH,
            HEIGHT,
            Some(owner),
            None,
            Some(instance),
            None,
        ) else {
            OPEN.store(false, Ordering::Relaxed);
            return;
        };
        let _ = SetForegroundWindow(menu);
    }
    super::log("menu open");
}

unsafe extern "system" fn procedure(
    menu: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let key = |code: u16| message == WM_KEYDOWN && wparam.0 == usize::from(code);
    if (message == WM_ACTIVATE && (wparam.0 & 0xffff) as u32 == WA_INACTIVE) || key(VK_ESCAPE.0) {
        close(menu, "menu closed");
    } else if key(VK_DOWN.0) {
        ITEM.store(ITEM.load(Ordering::Relaxed) % ITEMS + 1, Ordering::Relaxed);
    } else if key(VK_RETURN.0) {
        match ITEM.load(Ordering::Relaxed) {
            0 => {}
            item => close(menu, &format!("menu {item}")),
        }
    } else {
        // SAFETY: the default handling of a message of this thread's own window.
        return unsafe { DefWindowProcW(menu, message, wparam, lparam) };
    }
    LRESULT(0)
}

/// Closes the menu once: destroying it deactivates it, which would close it again.
fn close(menu: HWND, line: &str) {
    if OPEN.swap(false, Ordering::Relaxed) {
        super::log(line);
        // SAFETY: destroys this thread's own menu window.
        unsafe {
            let _ = DestroyWindow(menu);
        }
    }
}
