use super::{Control, current, tick};
use crate::guard::{Input, Pointer, is_chord, pointer};
use crate::native::{focus, input::AURA_INPUT_MARK};
use aura_desktop_domain::input::{VK_ESCAPE, VK_LWIN};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::*;

/// How often the thread looks at the kept desktop, and draws the pointer's marks while they
/// show.
const LOOK_MS: u32 = 40;
const DRAW_MS: u32 = 16;
const VK_RWIN: u16 = 0x5C;

/// The key whose press was kept from the program, so its release is kept too; 0: none.
static KEPT_KEY: AtomicU32 = AtomicU32::new(0);
struct Alive;
impl Drop for Alive {
    fn drop(&mut self) {
        super::ALIVE.store(false, Ordering::SeqCst);
        if let Some(control) = current() {
            control.take_over();
        }
    }
}

fn observe(control: &Control, input: Input) -> bool {
    let matches = focus::foreground().is_some_and(|(window, process)| {
        control.windows.contains(&window) || control.processes.contains(&process)
    });
    control.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).observe(input, matches)
}

/// Whether a modifier is down now; Windows stands for either Windows key.
fn held(code: u16) -> bool {
    // SAFETY: a plain query of the keyboard's state.
    let down = |key: u16| unsafe { GetAsyncKeyState(i32::from(key)) } < 0;
    down(code) || (code == VK_LWIN && down(VK_RWIN))
}

/// Whether a key of the user's is kept from the program: Escape while the program's window is in
/// front and the stop keys, while Aura keeps the desktop, and the release of a key kept so.
fn kept(event: &KBDLLHOOKSTRUCT, message: WPARAM) -> bool {
    let key = event.vkCode;
    if matches!(message.0 as u32, WM_KEYUP | WM_SYSKEYUP) {
        return key != 0
            && KEPT_KEY.compare_exchange(key, 0, Ordering::SeqCst, Ordering::SeqCst).is_ok();
    }
    let Some(control) = current() else { return false };
    let injected = event.flags.contains(LLKHF_INJECTED);
    let input = if key == u32::from(VK_ESCAPE) {
        Input::Escape { injected }
    } else if control.stop_keys.as_ref().is_some_and(|(chord, _)| is_chord(chord, key, held)) {
        Input::StopKeys { injected }
    } else {
        return false;
    };
    let keep = observe(&control, input);
    if keep {
        KEPT_KEY.store(key, Ordering::SeqCst);
    }
    keep
}

unsafe extern "system" fn keyboard(code: i32, message: WPARAM, event: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: Windows supplies this structure for the duration of the low-level callback.
        let value = unsafe { &*(event.0 as *const KBDLLHOOKSTRUCT) };
        if value.dwExtraInfo != AURA_INPUT_MARK && kept(value, message) {
            return LRESULT(1);
        }
    }
    // SAFETY: forwards an event this hook did not consume.
    unsafe { CallNextHookEx(None, code, message, event) }
}

/// Whether a screen point is over a window of the Aura that started the host.
fn on_aura(at: POINT) -> bool {
    focus::root_at(at.x, at.y).is_some_and(|(_, process)| crate::own::is_aura(process))
}

unsafe extern "system" fn mouse(code: i32, message: WPARAM, event: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: Windows supplies this structure for the duration of the callback.
        let value = unsafe { &*(event.0 as *const MSLLHOOKSTRUCT) };
        let injected = value.flags & LLMHF_INJECTED != 0;
        let kind = match message.0 as u32 {
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => Pointer::Pressed,
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => Pointer::Wheel,
            _ => Pointer::Other,
        };
        // Only a press or the wheel looks for the window under the pointer, never a move.
        if value.dwExtraInfo != AURA_INPUT_MARK
            && kind != Pointer::Other
            && let Some(control) = current()
        {
            observe(&control, pointer(kind, injected, on_aura(value.pt)));
        }
    }
    // SAFETY: mouse input belongs to the user and is always forwarded.
    unsafe { CallNextHookEx(None, code, message, event) }
}

pub(super) fn start() -> Result<(), String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("aura-desktop-guard".into())
        .spawn(move || {
            let _alive = Alive;
            // SAFETY: all hooks and windows are created, pumped and destroyed on this thread.
            let result = unsafe { run(&sender) };
            if let Err(error) = result {
                let _ = sender.send(Err(error.to_string()));
            }
        })
        .map_err(|error| error.to_string())?;
    receiver.recv_timeout(Duration::from_secs(5)).map_err(|error| error.to_string())?
}

unsafe fn run(
    sender: &std::sync::mpsc::SyncSender<Result<(), String>>,
) -> windows::core::Result<()> {
    // SAFETY: each hook is uninstalled before its message thread exits.
    unsafe {
        let module: HINSTANCE = GetModuleHandleW(None)?.into();
        let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), Some(module), 0)?;
        let mouse = match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), Some(module), 0) {
            Ok(hook) => hook,
            Err(error) => {
                let _ = UnhookWindowsHookEx(keyboard);
                return Err(error);
            }
        };
        let result = (|| {
            let _banner = super::banner::start(module)?;
            let mut marker = super::marker::Marker::new(module)?;
            let timer = SetTimer(None, 0, LOOK_MS, None);
            if timer == 0 {
                return Err(windows::core::Error::from_thread());
            }
            let mut drawing = false;
            super::ALIVE.store(true, Ordering::SeqCst);
            let _ = sender.send(Ok(()));
            let mut message = MSG::default();
            while GetMessageW(&raw mut message, None, 0, 0).as_bool() {
                if message.message == WM_TIMER {
                    tick();
                    super::banner::refresh();
                    // Drawn often only while the marks show, so an idle host stays quiet.
                    let shows = marker.refresh();
                    if shows != drawing {
                        drawing = shows;
                        SetTimer(None, timer, if shows { DRAW_MS } else { LOOK_MS }, None);
                    }
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            Ok(())
        })();
        let _ = UnhookWindowsHookEx(mouse);
        let _ = UnhookWindowsHookEx(keyboard);
        if let Some(control) = current() {
            control.take_over();
        }
        result
    }
}
