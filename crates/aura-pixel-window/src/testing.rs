//! For the tests of programs that use these windows: the windows stay hidden, and a test finds
//! them, presses keys in them and reads them the way a screen reader does, through UI Automation
//! on a thread of its own.
// UI Automation's identifiers keep their Windows names where they are matched.
#![allow(non_upper_case_globals)]
use std::{
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
            Threading::GetCurrentProcessId,
        },
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, IUIAutomationInvokePattern,
                IUIAutomationTogglePattern, IUIAutomationValuePattern, ToggleState_On,
                TreeScope_Children, UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId,
                UIA_EditControlTypeId, UIA_HyperlinkControlTypeId, UIA_InvokePatternId,
                UIA_ProgressBarControlTypeId, UIA_TextControlTypeId, UIA_TogglePatternId,
                UIA_ValuePatternId,
            },
            Input::KeyboardAndMouse::{
                VIRTUAL_KEY, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_SPACE, VK_TAB,
            },
            WindowsAndMessaging::{
                EnumWindows, GetClassNameW, GetWindowThreadProcessId, PostMessageW, WM_KEYDOWN,
                WM_KEYUP,
            },
        },
    },
    core::{BOOL, Result},
};

use crate::window::HIDDEN;

/// Windows made from now on in this process are never shown.
pub fn hide_windows() {
    HIDDEN.store(true, Ordering::Release);
}

/// This process's windows of this crate.
pub fn windows() -> Vec<isize> {
    unsafe extern "system" fn each(window: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the vector `windows` passed, alive for the enumeration.
        unsafe {
            let mut process = 0;
            GetWindowThreadProcessId(window, Some(&mut process));
            let mut class = [0_u16; 64];
            let length = GetClassNameW(window, &mut class).max(0) as usize;
            if process == GetCurrentProcessId()
                && String::from_utf16_lossy(&class[..length]) == "AuraPixelWindow"
            {
                (*(found.0 as *mut Vec<isize>)).push(window.0 as isize);
            }
        }
        true.into()
    }
    let mut found: Vec<isize> = Vec::new();
    // SAFETY: the callback only pushes into `found`, which outlives the call.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut Vec<isize> as isize));
    }
    found
}

/// Waits for one of this crate's windows to exist.
pub fn wait_for_window(timeout: Duration) -> Option<isize> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(window) = windows().first() {
            return Some(*window);
        }
        thread::sleep(Duration::from_millis(20));
    }
    None
}

#[derive(Clone, Copy, Debug)]
pub enum Key {
    Tab,
    Down,
    Enter,
    Space,
    Escape,
}

/// Presses and lets go of `key` in `window`, as a keyboard would.
pub fn press(window: isize, key: Key) {
    let code: VIRTUAL_KEY = match key {
        Key::Tab => VK_TAB,
        Key::Down => VK_DOWN,
        Key::Enter => VK_RETURN,
        Key::Space => VK_SPACE,
        Key::Escape => VK_ESCAPE,
    };
    // SAFETY: posts key messages to a window of this process.
    unsafe {
        let window = Some(HWND(window as *mut _));
        let _ = PostMessageW(window, WM_KEYDOWN, WPARAM(usize::from(code.0)), LPARAM(1));
        let _ = PostMessageW(window, WM_KEYUP, WPARAM(usize::from(code.0)), LPARAM((3 << 30) | 1));
    }
}

/// One part of a window as UI Automation reads it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Part {
    pub role: &'static str,
    pub name: String,
    pub automation_id: String,
    pub value: Option<String>,
    pub checked: Option<bool>,
    pub enabled: bool,
    pub status: String,
}

fn on_own_thread<T: Send>(read: impl FnOnce() -> Result<T> + Send) -> Result<T> {
    thread::scope(|scope| {
        scope
            .spawn(|| {
                // SAFETY: a fresh thread in the multithreaded apartment, as UI Automation clients
                // should be.
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                read()
            })
            .join()
            .unwrap_or_else(|_| Err(windows::core::Error::empty()))
    })
}

/// The window's parts, in the order UI Automation lists them.
pub fn parts(window: isize) -> Result<Vec<Part>> {
    on_own_thread(move || {
        // SAFETY: plain UI Automation client calls on objects this function owns.
        unsafe {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let root = automation.ElementFromHandle(HWND(window as *mut _))?;
            let all = root.FindAll(TreeScope_Children, &automation.CreateTrueCondition()?)?;
            let mut parts = Vec::new();
            for index in 0..all.Length()? {
                let element = all.GetElement(index)?;
                let kind = element.CurrentControlType()?;
                let role = match kind {
                    UIA_ButtonControlTypeId => "button",
                    UIA_CheckBoxControlTypeId => "check box",
                    UIA_EditControlTypeId => "edit",
                    UIA_HyperlinkControlTypeId => "link",
                    UIA_ProgressBarControlTypeId => "progress bar",
                    UIA_TextControlTypeId => "text",
                    _ => "other",
                };
                let value = element
                    .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                    .and_then(|pattern| pattern.CurrentValue())
                    .ok()
                    .map(|value| value.to_string());
                let checked = element
                    .GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
                    .and_then(|pattern| pattern.CurrentToggleState())
                    .ok()
                    .map(|state| state == ToggleState_On);
                parts.push(Part {
                    role,
                    name: element.CurrentName()?.to_string(),
                    automation_id: element.CurrentAutomationId()?.to_string(),
                    value,
                    checked,
                    enabled: element.CurrentIsEnabled()?.as_bool(),
                    status: element
                        .CurrentItemStatus()
                        .map(|status| status.to_string())
                        .unwrap_or_default(),
                });
            }
            Ok(parts)
        }
    })
}

/// Invokes the part with `automation_id`, as a screen reader's user would.
pub fn invoke(window: isize, automation_id: &str) -> Result<()> {
    let wanted = automation_id.to_owned();
    on_own_thread(move || {
        // SAFETY: plain UI Automation client calls on objects this function owns.
        unsafe {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let root = automation.ElementFromHandle(HWND(window as *mut _))?;
            let all = root.FindAll(TreeScope_Children, &automation.CreateTrueCondition()?)?;
            for index in 0..all.Length()? {
                let element = all.GetElement(index)?;
                if element.CurrentAutomationId()? == wanted {
                    return element
                        .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)?
                        .Invoke();
                }
            }
            Err(windows::core::Error::empty())
        }
    })
}

/// The heading of the page a window shows, once it shows `heading`.
pub fn wait_for_heading(window: isize, heading: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        let shown = parts(window)
            .ok()
            .and_then(|parts| parts.into_iter().find(|part| part.automation_id == "heading"));
        if shown.is_some_and(|part| part.name == heading) {
            return true;
        }
        thread::sleep(Duration::from_millis(30));
    }
    false
}
