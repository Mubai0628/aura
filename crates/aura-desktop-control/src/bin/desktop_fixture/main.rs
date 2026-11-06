//! `aura-desktop-fixture`: Aura's own test window for computer use, made of ordinary Win32
//! controls whose names, values and states the tests read and change. It never takes the focus
//! when it opens.
//!
//! Arguments: `--title <text>` (default "Aura 测试窗口 <process id>"), `--at <x>,<y>` (physical
//! pixels), `--hang-ms <n>` (how long the window stops answering, default 8000), `--hang-at <n>`
//! (stop answering by itself `n` ms after it shows), `--log <file>`, `--no-maximize` (no
//! maximize button), `--host <program>` (starts the program once the window shows, with this
//! one's standard input and output, as Core starts the desktop host), `--close-ms <n>` (asked to
//! close, the window closes `n` ms later, as a program that first saves its work does), `--menu`
//! (a right click opens the window's own menu, see `menu.rs`), `--topmost` (always on top).
//!
//! With `--log`, one line is added to the file per thing that happened, in screen pixels:
//! `place <control> <x> <y> <width> <height>` for each control and for `client`, the client
//! area, once the window shows, then `ready`; `click ok <count>`, `click pay`, `click hang`,
//! `hang` (the timer's), `close asked`, `check <0|1>`, `edit <text>`, `password <length>`, `list <index>`,
//! `fruits replaced`, `combo <index>`; `down <x> <y>` and `up <x> <y>` for the left button on
//! the client area, `right <x> <y>`, `wheel <delta>` and `hwheel <delta>`; and the menu's own.
#![windows_subsystem = "windows"]
mod driver;
mod menu;

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_WINDOW, ClientToScreen, DEFAULT_GUI_FONT, GetStockObject, HBRUSH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CB_ADDSTRING, CB_GETCURSEL,
    CBN_SELCHANGE, CBS_DROPDOWNLIST, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, EN_CHANGE, ES_AUTOHSCROLL, ES_PASSWORD, GetClientRect, GetDlgItem,
    GetMessageW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, KillTimer,
    LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, LBN_SELCHANGE, LBS_NOTIFY, LoadCursorW, MSG,
    PostQuitMessage, RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SendMessageW, SetTimer,
    SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE,
    WM_COMMAND, WM_CREATE, WM_DESTROY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEWHEEL,
    WM_RBUTTONUP, WM_SETFONT, WM_TIMER, WNDCLASSEXW, WS_BORDER, WS_CHILD, WS_DISABLED,
    WS_EX_TOPMOST, WS_MAXIMIZEBOX, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{HSTRING, PCWSTR, w};

const ID_OK: i32 = 101;
const ID_LABEL: i32 = 102;
const ID_EDIT: i32 = 103;
const ID_CHECK: i32 = 104;
const ID_PASSWORD: i32 = 105;
const ID_LIST: i32 = 106;
const ID_PAY: i32 = 107;
const ID_HANG: i32 = 108;
const ID_REPLACE: i32 = 109;
const ID_DISABLED: i32 = 110;
const ID_HIDDEN: i32 = 111;
const ID_COMBO: i32 = 112;
const ID_LONG: i32 = 113;
/// The edit's own label. UI Automation names an unnamed Win32 edit after the static control just
/// before it; without this one that would be the click counter, whose text changes.
const ID_EDIT_LABEL: i32 = 114;
const ID_TRANSFER: i32 = 115;
const CONTROLS: [(i32, &str); 14] = [
    (ID_TRANSFER, "transfer"),
    (ID_OK, "ok"),
    (ID_LABEL, "label"),
    (ID_EDIT, "edit"),
    (ID_CHECK, "check"),
    (ID_PASSWORD, "password"),
    (ID_LIST, "list"),
    (ID_PAY, "pay"),
    (ID_HANG, "hang"),
    (ID_REPLACE, "replace"),
    (ID_DISABLED, "disabled"),
    (ID_HIDDEN, "hidden"),
    (ID_COMBO, "combo"),
    (ID_LONG, "long"),
];
const HANG_TIMER: usize = 1;
const CLOSE_TIMER: usize = 2;

static CLICKS: AtomicU32 = AtomicU32::new(0);
static HANG_MS: AtomicU64 = AtomicU64::new(8_000);
static HANG_AT: AtomicU32 = AtomicU32::new(0);
static CLOSE_MS: AtomicU32 = AtomicU32::new(0);
static LOG: OnceLock<PathBuf> = OnceLock::new();
static HOST: OnceLock<PathBuf> = OnceLock::new();
static PASSWORD: OnceLock<String> = OnceLock::new();

fn log(line: &str) {
    let Some(path) = LOG.get() else { return };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(format!("{line}\n").as_bytes());
    }
}

fn main() {
    let mut title = format!("Aura 测试窗口 {}", std::process::id());
    let mut at = (40, 40);
    let mut style = WS_OVERLAPPEDWINDOW;
    let mut extended = WINDOW_EX_STYLE(0);
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--no-maximize" {
            style = WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 & !WS_MAXIMIZEBOX.0);
            continue;
        }
        if argument == "--topmost" {
            extended = WS_EX_TOPMOST;
            continue;
        }
        if argument == "--menu" {
            menu::ENABLED.store(true, Ordering::Relaxed);
            continue;
        }
        match (argument.as_str(), arguments.next()) {
            ("--title", Some(value)) => title = value,
            ("--emit", Some(value)) => driver::configure(&value),
            ("--at", Some(value)) => {
                if let Some((x, y)) = value.split_once(',')
                    && let (Ok(x), Ok(y)) = (x.trim().parse(), y.trim().parse())
                {
                    at = (x, y);
                }
            }
            ("--hang-ms", Some(value)) => {
                HANG_MS.store(value.parse().unwrap_or(8_000), Ordering::Relaxed);
            }
            ("--hang-at", Some(value)) => {
                HANG_AT.store(value.parse().unwrap_or(0), Ordering::Relaxed);
            }
            ("--close-ms", Some(value)) => {
                CLOSE_MS.store(value.parse().unwrap_or(0), Ordering::Relaxed);
            }
            ("--log", Some(value)) => {
                let _ = LOG.set(PathBuf::from(value));
            }
            ("--host", Some(value)) => {
                let _ = HOST.set(PathBuf::from(value));
            }
            ("--password", Some(value)) => {
                let _ = PASSWORD.set(value);
            }
            _ => {}
        }
    }
    // SAFETY: the window procedure below is the only code that touches these windows, all on
    // this thread; strings passed to Windows outlive the calls that use them.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut _),
            lpszClassName: w!("AuraDesktopFixture"),
            ..Default::default()
        };
        RegisterClassExW(&raw const class);
        let Ok(window) = CreateWindowExW(
            extended,
            w!("AuraDesktopFixture"),
            &HSTRING::from(title),
            style,
            at.0,
            at.1,
            520,
            420,
            None,
            None,
            Some(instance),
            None,
        ) else {
            return;
        };
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        log_places(window);
        driver::start();
        let _host = HOST.get().and_then(|program| std::process::Command::new(program).spawn().ok());
        let hang_at = HANG_AT.load(Ordering::Relaxed);
        if hang_at > 0 {
            SetTimer(Some(window), HANG_TIMER, hang_at, None);
        }
        let mut message = MSG::default();
        while GetMessageW(&raw mut message, None, 0, 0).as_bool() {
            if message.message == windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN
                && message.wParam.0 == 0x1B
            {
                log("key escape");
            }
            if matches!(
                message.message,
                windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN
                    | windows::Win32::UI::WindowsAndMessaging::WM_SYSKEYDOWN
            ) && message.wParam.0 == 0x51
            {
                log("key q");
            }
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
}

fn place_line(name: &str, x: i32, y: i32, width: i32, height: i32) -> String {
    format!("place {name} {x} {y} {width} {height}")
}

unsafe fn log_places(window: HWND) {
    // SAFETY: queries of this thread's own windows.
    unsafe {
        for (id, name) in CONTROLS {
            let mut rect = RECT::default();
            if let Ok(control) = GetDlgItem(Some(window), id)
                && GetWindowRect(control, &raw mut rect).is_ok()
            {
                log(&place_line(
                    name,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                ));
            }
        }
        let mut client = RECT::default();
        let mut origin = POINT::default();
        if GetClientRect(window, &raw mut client).is_ok()
            && ClientToScreen(window, &raw mut origin).as_bool()
        {
            log(&place_line("client", origin.x, origin.y, client.right, client.bottom));
        }
    }
    log("ready");
}

unsafe fn child(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    id: i32,
    place: (i32, i32, i32, i32),
) {
    // SAFETY: creates one child control of `parent` on the window's own thread.
    unsafe {
        let Ok(control) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            &HSTRING::from(text),
            WS_CHILD | WS_VISIBLE | style,
            place.0,
            place.1,
            place.2,
            place.3,
            Some(parent),
            Some(HMENU(id as isize as *mut _)),
            None,
            None,
        ) else {
            return;
        };
        let font = GetStockObject(DEFAULT_GUI_FONT);
        SendMessageW(control, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
    }
}

/// Adds items to a list box or a combo box (`message` is its add-string message).
unsafe fn add_items(control: HWND, message: u32, items: &[&str]) {
    for item in items {
        let text = HSTRING::from(*item);
        // SAFETY: the string outlives the call, which copies it.
        unsafe {
            SendMessageW(control, message, None, Some(LPARAM(text.as_ptr() as isize)));
        }
    }
}

unsafe fn text_of(control: HWND) -> String {
    // SAFETY: reads a control's text into a buffer one longer than its length.
    unsafe {
        let mut buffer = vec![0_u16; GetWindowTextLengthW(control).max(0) as usize + 1];
        let length = GetWindowTextW(control, &mut buffer).max(0) as usize;
        String::from_utf16_lossy(&buffer[..length])
    }
}

/// The screen point of a mouse message's client coordinates.
unsafe fn screen_point(window: HWND, lparam: LPARAM) -> POINT {
    let mut point = POINT {
        x: i32::from(lparam.0 as u16 as i16),
        y: i32::from((lparam.0 >> 16) as u16 as i16),
    };
    // SAFETY: converts a point of this thread's own window.
    unsafe {
        let _ = ClientToScreen(window, &raw mut point);
    }
    point
}

unsafe fn command(window: HWND, id: i32, code: u32) {
    // SAFETY: reads and changes this thread's own controls.
    unsafe {
        let control = GetDlgItem(Some(window), id).ok();
        let label = GetDlgItem(Some(window), ID_LABEL).ok();
        match (id, code) {
            (ID_OK, BN_CLICKED) => {
                let clicks = CLICKS.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(label) = label {
                    let _ = SetWindowTextW(label, &HSTRING::from(format!("点了 {clicks} 次")));
                }
                log(&format!("click ok {clicks}"));
            }
            (ID_PAY, BN_CLICKED) => {
                if let Some(label) = label {
                    let _ = SetWindowTextW(label, w!("付款了"));
                }
                log("click pay");
            }
            (ID_TRANSFER, BN_CLICKED) => log("click transfer"),
            (ID_HANG, BN_CLICKED) => {
                log("click hang");
                std::thread::sleep(std::time::Duration::from_millis(
                    HANG_MS.load(Ordering::Relaxed),
                ));
            }
            (ID_CHECK, BN_CLICKED) => {
                if let Some(control) = control {
                    log(&format!("check {}", SendMessageW(control, BM_GETCHECK, None, None).0));
                }
            }
            (ID_EDIT, EN_CHANGE) => {
                if let Some(control) = control {
                    log(&format!("edit {}", text_of(control)));
                }
            }
            (ID_PASSWORD, EN_CHANGE) => {
                if let Some(control) = control {
                    log(&format!("password {}", text_of(control).chars().count()));
                }
            }
            (ID_LIST, LBN_SELCHANGE) => {
                if let Some(control) = control {
                    log(&format!("list {}", SendMessageW(control, LB_GETCURSEL, None, None).0));
                }
            }
            (ID_REPLACE, BN_CLICKED) => {
                if let Ok(list) = GetDlgItem(Some(window), ID_LIST) {
                    SendMessageW(list, LB_RESETCONTENT, None, None);
                    add_items(list, LB_ADDSTRING, &["葡萄", "西瓜", "芒果"]);
                }
                log("fruits replaced");
            }
            (ID_COMBO, CBN_SELCHANGE) => {
                if let Some(control) = control {
                    log(&format!("combo {}", SendMessageW(control, CB_GETCURSEL, None, None).0));
                }
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: Windows calls this on the window's thread with a live window handle.
    unsafe {
        match message {
            WM_CREATE => {
                let button = WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP;
                child(window, w!("BUTTON"), "确定", button, ID_OK, (16, 16, 120, 32));
                child(
                    window,
                    w!("STATIC"),
                    "还没点",
                    WINDOW_STYLE(0),
                    ID_LABEL,
                    (150, 22, 300, 24),
                );
                child(
                    window,
                    w!("STATIC"),
                    "文字",
                    WINDOW_STYLE(0),
                    ID_EDIT_LABEL,
                    (322, 68, 60, 24),
                );
                child(
                    window,
                    w!("EDIT"),
                    "",
                    WS_BORDER | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                    ID_EDIT,
                    (16, 64, 300, 28),
                );
                child(
                    window,
                    w!("BUTTON"),
                    "选项",
                    WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
                    ID_CHECK,
                    (16, 104, 200, 24),
                );
                child(
                    window,
                    w!("EDIT"),
                    "",
                    WS_BORDER | WS_TABSTOP | WINDOW_STYLE((ES_PASSWORD | ES_AUTOHSCROLL) as u32),
                    ID_PASSWORD,
                    (16, 140, 300, 28),
                );
                if let Some(password) = PASSWORD.get()
                    && let Ok(field) = GetDlgItem(Some(window), ID_PASSWORD)
                {
                    let _ = SetWindowTextW(field, &HSTRING::from(password));
                    log(&format!("seeded-password {}", GetWindowTextLengthW(field)));
                }
                child(
                    window,
                    w!("LISTBOX"),
                    "",
                    WS_BORDER | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
                    ID_LIST,
                    (16, 180, 200, 100),
                );
                if let Ok(list) = GetDlgItem(Some(window), ID_LIST) {
                    add_items(list, LB_ADDSTRING, &["苹果", "香蕉", "橙子"]);
                }
                child(window, w!("BUTTON"), "付款", button, ID_PAY, (240, 180, 120, 32));
                child(window, w!("BUTTON"), "转账", button, ID_TRANSFER, (370, 224, 120, 32));
                child(window, w!("BUTTON"), "卡住", button, ID_HANG, (240, 224, 120, 32));
                child(window, w!("BUTTON"), "换水果", button, ID_REPLACE, (370, 180, 120, 32));
                child(
                    window,
                    w!("BUTTON"),
                    "不能点",
                    button | WS_DISABLED,
                    ID_DISABLED,
                    (370, 224, 120, 32),
                );
                child(window, w!("BUTTON"), "看不见", button, ID_HIDDEN, (240, 268, 120, 32));
                if let Ok(hidden) = GetDlgItem(Some(window), ID_HIDDEN) {
                    let _ = ShowWindow(hidden, SW_HIDE);
                }
                child(
                    window,
                    w!("COMBOBOX"),
                    "",
                    WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_TABSTOP | WS_VSCROLL,
                    ID_COMBO,
                    (16, 292, 200, 160),
                );
                if let Ok(combo) = GetDlgItem(Some(window), ID_COMBO) {
                    add_items(combo, CB_ADDSTRING, &["小", "中", "大"]);
                }
                child(
                    window,
                    w!("LISTBOX"),
                    "",
                    WS_BORDER | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
                    ID_LONG,
                    (370, 268, 120, 100),
                );
                if let Ok(long) = GetDlgItem(Some(window), ID_LONG) {
                    let items: Vec<String> = (1..=20).map(|item| format!("第 {item} 项")).collect();
                    let items: Vec<&str> = items.iter().map(String::as_str).collect();
                    add_items(long, LB_ADDSTRING, &items);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == HANG_TIMER => {
                let _ = KillTimer(Some(window), HANG_TIMER);
                log("hang");
                std::thread::sleep(std::time::Duration::from_millis(
                    HANG_MS.load(Ordering::Relaxed),
                ));
                LRESULT(0)
            }
            WM_CLOSE if CLOSE_MS.load(Ordering::Relaxed) > 0 => {
                log("close asked");
                SetTimer(Some(window), CLOSE_TIMER, CLOSE_MS.load(Ordering::Relaxed), None);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == CLOSE_TIMER => {
                let _ = KillTimer(Some(window), CLOSE_TIMER);
                let _ = DestroyWindow(window);
                LRESULT(0)
            }
            WM_COMMAND => {
                command(window, (wparam.0 & 0xffff) as i32, ((wparam.0 >> 16) & 0xffff) as u32);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                driver::drag_started();
                let _ = SetCapture(window);
                let point = screen_point(window, lparam);
                log(&format!("down {} {}", point.x, point.y));
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                let point = screen_point(window, lparam);
                log(&format!("up {} {}", point.x, point.y));
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let point = screen_point(window, lparam);
                log(&format!("right {} {}", point.x, point.y));
                if menu::ENABLED.load(Ordering::Relaxed) {
                    menu::open(window, point);
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta = (wparam.0 >> 16) as u16 as i16;
                log(&format!(
                    "{} {delta}",
                    if message == WM_MOUSEWHEEL { "wheel" } else { "hwheel" }
                ));
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}
