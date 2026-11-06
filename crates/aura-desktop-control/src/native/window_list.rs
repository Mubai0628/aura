//! The desktop's top-level windows and what each says about the program that made it.
use super::text::{from_buffer, take_com_string};
use aura_desktop_domain::Rect;
use std::collections::BTreeSet;
use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow,
    GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

/// One visible top-level window, as Windows describes it.
#[derive(Clone, Debug, Default)]
pub struct RawWindow {
    pub handle: isize,
    pub title: String,
    pub class: String,
    pub process_id: u32,
    /// The process that draws the window's content: for a Store app's frame, the app's own
    /// process inside it.
    pub process_path: Option<String>,
    pub process_name: String,
    pub aumid: Option<String>,
    pub rect: Rect,
    pub minimized: bool,
    pub foreground: bool,
    pub tool_window: bool,
}

const FRAME_HOST_CLASS: &str = "ApplicationFrameWindow";

/// The visible, uncloaked top-level windows, front to back. The host's own (the banner and the
/// pointer's marks) are left out: no capture shows them, so they cover nothing.
#[must_use]
pub fn top_level() -> Vec<RawWindow> {
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into the Vec whose address it is given, during this call.
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&raw mut handles as isize));
    }
    // SAFETY: a plain query of the current foreground window.
    let foreground = unsafe { GetForegroundWindow() };
    handles
        .into_iter()
        .filter(|handle| visible(*handle))
        .map(|handle| describe(handle, handle == foreground))
        .filter(|window| window.process_id != std::process::id())
        .collect()
}

/// The visible top-level windows of the processes, front to back, by handle and bounds: a cheap
/// look for a window a program has just opened.
#[must_use]
pub fn of_processes(processes: &BTreeSet<u32>) -> Vec<(isize, Rect)> {
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into the Vec whose address it is given, during this call.
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&raw mut handles as isize));
    }
    handles
        .into_iter()
        .filter(|handle| {
            let mut process = 0_u32;
            // SAFETY: a query of a window handle; a window that is gone leaves `process` 0.
            unsafe { GetWindowThreadProcessId(*handle, Some(&raw mut process)) };
            processes.contains(&process) && visible(*handle)
        })
        .map(|handle| (handle.0 as isize, bounds(handle)))
        .collect()
}

unsafe extern "system" fn collect(handle: HWND, list: LPARAM) -> BOOL {
    // SAFETY: `list` is the address of the Vec that `top_level` keeps alive during EnumWindows.
    unsafe { (*(list.0 as *mut Vec<HWND>)).push(handle) };
    BOOL(1)
}

pub(super) fn visible(handle: HWND) -> bool {
    // SAFETY: queries of a window handle; a window that is gone answers false or fails.
    unsafe {
        if !IsWindowVisible(handle).as_bool() {
            return false;
        }
        let mut cloaked = 0_u32;
        let cloaked_known = DwmGetWindowAttribute(
            handle,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        )
        .is_ok();
        !(cloaked_known && cloaked != 0)
    }
}

fn describe(handle: HWND, foreground: bool) -> RawWindow {
    // SAFETY: queries of a window handle and of the process behind it; every handle opened here
    // is closed here.
    unsafe {
        let mut title = [0_u16; 512];
        let length = GetWindowTextW(handle, &mut title);
        let mut class = [0_u16; 256];
        let class_length = GetClassNameW(handle, &mut class);
        let class = String::from_utf16_lossy(&class[..class_length.max(0) as usize]);
        let mut process_id = 0_u32;
        GetWindowThreadProcessId(handle, Some(&raw mut process_id));
        let style = GetWindowLongPtrW(handle, GWL_EXSTYLE) as u32;
        let content =
            if class == FRAME_HOST_CLASS { inner_process(handle, process_id) } else { None };
        let process = content.unwrap_or(process_id);
        let process_path = process_image(process);
        let process_name = process_path
            .as_deref()
            .and_then(crate::catalog::file_name)
            .unwrap_or_default()
            .to_owned();
        let aumid = window_aumid(handle).or_else(|| process_aumid(process));
        RawWindow {
            handle: handle.0 as isize,
            title: String::from_utf16_lossy(&title[..length.max(0) as usize]),
            class,
            process_id: process,
            process_path,
            process_name,
            aumid,
            rect: bounds(handle),
            minimized: IsIconic(handle).as_bool(),
            foreground,
            tool_window: style & WS_EX_TOOLWINDOW.0 != 0,
        }
    }
}

/// A Store app's frame belongs to ApplicationFrameHost; the app draws in a child window of its
/// own process. `None` while the app is suspended and has no such child.
fn inner_process(frame: HWND, frame_process: u32) -> Option<u32> {
    struct Search {
        frame_process: u32,
        found: Option<u32>,
    }
    unsafe extern "system" fn each(child: HWND, search: LPARAM) -> BOOL {
        // SAFETY: `search` is the address of the Search below, alive during EnumChildWindows.
        unsafe {
            let search = &mut *(search.0 as *mut Search);
            let mut process = 0_u32;
            GetWindowThreadProcessId(child, Some(&raw mut process));
            if process != 0 && process != search.frame_process {
                search.found = Some(process);
                return BOOL(0);
            }
            BOOL(1)
        }
    }
    let mut search = Search { frame_process, found: None };
    // SAFETY: the callback only writes into `search` during this call.
    unsafe {
        let _ = EnumChildWindows(Some(frame), Some(each), LPARAM(&raw mut search as isize));
    }
    search.found
}

fn process_image(process: u32) -> Option<String> {
    // SAFETY: the handle is opened for a limited query and closed before returning.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process).ok()?;
        let mut buffer = vec![0_u16; 32_768];
        let mut length = buffer.len() as u32;
        let named = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut length,
        );
        let _ = CloseHandle(handle);
        named.ok()?;
        Some(String::from_utf16_lossy(&buffer[..length as usize]))
    }
}

fn window_aumid(handle: HWND) -> Option<String> {
    // SAFETY: the property store is released when it drops; the value is copied and freed.
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(handle).ok()?;
        let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let text = take_com_string(PropVariantToStringAlloc(&value).ok()?);
        (!text.trim().is_empty()).then_some(text)
    }
}

/// A packaged process's AUMID.
fn process_aumid(process: u32) -> Option<String> {
    // SAFETY: the handle is opened for a limited query and closed before returning.
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process).ok()?;
        let mut buffer = vec![0_u16; 1024];
        let mut length = buffer.len() as u32;
        let result =
            GetApplicationUserModelId(handle, &raw mut length, Some(PWSTR(buffer.as_mut_ptr())));
        let _ = CloseHandle(handle);
        (result == ERROR_SUCCESS).then(|| from_buffer(&buffer)).filter(|aumid| !aumid.is_empty())
    }
}

/// The window's visible bounds (without the invisible resize borders), in physical pixels.
pub(super) fn bounds(handle: HWND) -> Rect {
    let mut rect = RECT::default();
    // SAFETY: both calls write one RECT into the local above.
    unsafe {
        let framed = DwmGetWindowAttribute(
            handle,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut rect).cast(),
            size_of::<RECT>() as u32,
        );
        if framed.is_err() {
            let _ = GetWindowRect(handle, &raw mut rect);
        }
    }
    Rect {
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    }
}
