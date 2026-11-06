//! Simulated mouse and keyboard input. Every event carries [`AURA_INPUT_MARK`], so that what
//! listens to input (the watch on the user's while Aura keeps the desktop) can tell it from the
//! user's.
use aura_desktop_domain::{Key, MouseButton, Rect};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, POINT};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSE_EVENT_FLAGS,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// `dwExtraInfo` of every event Aura sends.
pub const AURA_INPUT_MARK: usize = 0x4155_5241;
static LAST_SENT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[must_use]
pub fn last_sent_tick() -> u32 {
    LAST_SENT.load(std::sync::atomic::Ordering::SeqCst)
}
const WHEEL_DELTA: i32 = 120;

fn mouse(dx: i32, dy: i32, data: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: AURA_INPUT_MARK,
            },
        },
    }
}

fn keyboard(code: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(code),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: AURA_INPUT_MARK,
            },
        },
    }
}

/// Sends the events in order; true only when Windows took every one. Windows takes them and
/// still drops them silently for a program running with more rights, which the caller checks
/// before.
fn send(events: &[INPUT]) -> bool {
    if events.is_empty() {
        return true;
    }
    // SAFETY: a slice of fully initialised INPUT values and their size.
    let sent = unsafe { SendInput(events, size_of::<INPUT>() as i32) };
    if sent > 0 {
        let mut last = windows::Win32::UI::Input::KeyboardAndMouse::LASTINPUTINFO {
            cbSize: size_of::<windows::Win32::UI::Input::KeyboardAndMouse::LASTINPUTINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: reads the system's input timestamp after this batch, without reading content.
        if unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetLastInputInfo(&raw mut last) }
            .as_bool()
        {
            LAST_SENT.store(last.dwTime, std::sync::atomic::Ordering::SeqCst);
        }
    }
    sent as usize == events.len()
}

/// The absolute position Windows expects for a screen point: 0–65535 across the virtual screen.
fn absolute(screen: Rect, x: i32, y: i32) -> (i32, i32) {
    let scale = |offset: i32, size: i32| -> i32 {
        let span = i64::from(size.max(2) - 1);
        ((i64::from(offset) * 65_535 + span / 2) / span) as i32
    };
    (scale(x - screen.x, screen.width), scale(y - screen.y, screen.height))
}

/// Where the pointer is, in screen pixels.
#[must_use]
pub fn cursor() -> Option<(i32, i32)> {
    let mut point = POINT::default();
    // SAFETY: a query that fills one point.
    unsafe { GetCursorPos(&raw mut point) }.ok().map(|()| (point.x, point.y))
}

fn moved(screen: Rect, x: i32, y: i32) -> INPUT {
    let (dx, dy) = absolute(screen, x, y);
    mouse(dx, dy, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK)
}

pub fn move_to(screen: Rect, x: i32, y: i32) -> bool {
    send(&[moved(screen, x, y)])
}

fn button_flags(button: MouseButton) -> (MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS) {
    match button {
        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
    }
}

/// Presses `button` at `(x, y)`. The pointer is put there in the same batch: Windows lets no
/// other input in between, so a move of the user's cannot take the press elsewhere.
pub fn press_at(screen: Rect, x: i32, y: i32, button: MouseButton) -> bool {
    send(&[moved(screen, x, y), mouse(0, 0, 0, button_flags(button).0)])
}

/// Lets `button` go at `(x, y)`, the pointer put there in the same batch.
pub fn release_at(screen: Rect, x: i32, y: i32, button: MouseButton) -> bool {
    send(&[moved(screen, x, y), mouse(0, 0, 0, button_flags(button).1)])
}

/// Lets `button` go wherever the pointer is.
pub fn release_button(button: MouseButton) -> bool {
    send(&[mouse(0, 0, 0, button_flags(button).1)])
}

/// Turns the wheel at `(x, y)`, the pointer put there in the same batch: positive `down` scrolls
/// down (the wheel turns toward the user), positive `right` scrolls right.
pub fn wheel_at(screen: Rect, x: i32, y: i32, down: i32, right: i32) -> bool {
    let mut events = vec![moved(screen, x, y)];
    if down != 0 {
        events.push(mouse(0, 0, -down.saturating_mul(WHEEL_DELTA), MOUSEEVENTF_WHEEL));
    }
    if right != 0 {
        events.push(mouse(0, 0, right.saturating_mul(WHEEL_DELTA), MOUSEEVENTF_HWHEEL));
    }
    send(&events)
}

fn key_flags(key: Key, up: bool) -> KEYBD_EVENT_FLAGS {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if key.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    flags
}

/// Holds the modifiers, presses the key, and lets go in reverse order.
pub fn chord(modifiers: &[Key], key: Key) -> bool {
    let mut events: Vec<INPUT> =
        modifiers.iter().map(|m| keyboard(m.code, 0, key_flags(*m, false))).collect();
    events.push(keyboard(key.code, 0, key_flags(key, false)));
    events.push(keyboard(key.code, 0, key_flags(key, true)));
    events.extend(modifiers.iter().rev().map(|m| keyboard(m.code, 0, key_flags(*m, true))));
    send(&events)
}

/// Types text as Unicode characters, past any input method; a line break is Enter and a tab is
/// Tab, as a keyboard would send them.
pub fn type_text(text: &str) -> bool {
    let mut events = Vec::new();
    for unit in text.replace("\r\n", "\n").encode_utf16() {
        match unit {
            0x0A => {
                let enter = Key { code: aura_desktop_domain::input::VK_RETURN, extended: false };
                events.push(keyboard(enter.code, 0, key_flags(enter, false)));
                events.push(keyboard(enter.code, 0, key_flags(enter, true)));
            }
            0x09 => {
                let tab = Key { code: aura_desktop_domain::input::VK_TAB, extended: false };
                events.push(keyboard(tab.code, 0, key_flags(tab, false)));
                events.push(keyboard(tab.code, 0, key_flags(tab, true)));
            }
            0x0D => {}
            unit => {
                events.push(keyboard(0, unit, KEYEVENTF_UNICODE));
                events.push(keyboard(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
            }
        }
    }
    events.chunks(200).all(send)
}

/// Puts `text` on the clipboard, in place of what was there.
pub fn set_clipboard_text(text: &str) -> anyhow::Result<()> {
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the global block is filled while locked, then owned by the clipboard once
    // SetClipboardData succeeds; the clipboard is closed on every path.
    unsafe {
        OpenClipboard(None)?;
        let result = (|| -> anyhow::Result<()> {
            EmptyClipboard()?;
            let block: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2)?;
            let target = GlobalLock(block).cast::<u16>();
            anyhow::ensure!(!target.is_null(), "锁不住剪贴板的内存");
            std::ptr::copy_nonoverlapping(units.as_ptr(), target, units.len());
            let _ = GlobalUnlock(block);
            SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(block.0)))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}
