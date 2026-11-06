//! Synthetic stand-ins for physical input, confined to the unpublished test program.
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

static EMIT: OnceLock<(String, u64)> = OnceLock::new();
static DRAG_STARTED: AtomicBool = AtomicBool::new(false);

pub(super) fn drag_started() {
    DRAG_STARTED.store(true, Ordering::SeqCst);
}

pub(super) fn configure(value: &str) {
    if let Some((kind, delay)) = value.split_once(':')
        && matches!(
            kind,
            "user-esc"
                | "aura-esc"
                | "user-mouse"
                | "user-move-far"
                | "user-key"
                | "user-click"
                | "user-wheel"
                | "user-stop-keys"
                | "user-drag-move"
                | "user-drag-button"
                | "user-drag-key"
                | "user-drag-wheel"
        )
        && let Ok(delay) = delay.parse::<u64>()
    {
        let _ = EMIT.set((kind.into(), delay.min(10_000)));
    }
}

pub(super) fn start() {
    let Some((kind, delay)) = EMIT.get().cloned() else { return };
    std::thread::spawn(move || {
        if kind.starts_with("user-drag-") {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while !DRAG_STARTED.load(Ordering::SeqCst) {
                if std::time::Instant::now() >= deadline {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(delay));
        let mark = if kind == "aura-esc" {
            aura_desktop_control::native::input::AURA_INPUT_MARK
        } else {
            0x5445_5354
        };
        let far = kind == "user-move-far";
        let events = if kind == "user-mouse" || kind == "user-drag-move" || far {
            vec![INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: if far { 40 } else { 1 },
                        dy: if far { 40 } else { 0 },
                        dwFlags: MOUSEEVENTF_MOVE,
                        dwExtraInfo: mark,
                        ..Default::default()
                    },
                },
            }]
        } else if matches!(
            kind.as_str(),
            "user-drag-button" | "user-drag-wheel" | "user-click" | "user-wheel"
        ) {
            let wheel = kind.ends_with("-wheel");
            // The middle button opens nothing in the fixture's controls.
            let flags = if wheel {
                vec![MOUSEEVENTF_WHEEL]
            } else if kind == "user-click" {
                vec![MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP]
            } else {
                vec![MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP]
            };
            flags
                .into_iter()
                .map(|flags| INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dwFlags: flags,
                            mouseData: if wheel { 120 } else { 0 },
                            dwExtraInfo: mark,
                            ..Default::default()
                        },
                    },
                })
                .collect()
        } else if kind == "user-stop-keys" {
            // Control+Alt+Q, as the default stop keys are.
            let key = |code: u16, flags: KEYBD_EVENT_FLAGS| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(code),
                        dwFlags: flags,
                        dwExtraInfo: mark,
                        ..Default::default()
                    },
                },
            };
            let down = KEYBD_EVENT_FLAGS(0);
            vec![
                key(0x11, down),
                key(0x12, down),
                key(0x51, down),
                key(0x51, KEYEVENTF_KEYUP),
                key(0x12, KEYEVENTF_KEYUP),
                key(0x11, KEYEVENTF_KEYUP),
            ]
        } else {
            [KEYBD_EVENT_FLAGS(0), KEYEVENTF_KEYUP]
                .into_iter()
                .map(|flags| INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(if kind.ends_with("-key") { 0x10 } else { 0x1B }),
                            dwFlags: flags,
                            dwExtraInfo: mark,
                            ..Default::default()
                        },
                    },
                })
                .collect()
        };
        // SAFETY: fully initialized synthetic events; this binary is never shipped.
        unsafe {
            let sent = SendInput(&events, size_of::<INPUT>() as i32);
            let mut last =
                LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, ..Default::default() };
            if GetLastInputInfo(&raw mut last).as_bool() {
                super::log(&format!("test-input-tick {} sent {sent}", last.dwTime));
            }
        }
    });
}
