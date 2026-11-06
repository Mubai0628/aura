//! The picture-in-picture window: a top-level window with a row of controls above the Remote
//! Desktop control. Everything the window hears is handed to the app as an [`Input`], on the
//! window's own thread, from its message loop: what the control fires is posted first, so that
//! the app never calls into the control from inside one of its events.
use super::hook;
use super::rdp::{Rdp, RdpEvent};
use aura_desktop_domain::pip::PipCommand;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Mutex;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_WINDOW, CreateFontIndirectW, DeleteObject, HBRUSH, HFONT, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, HOT_KEY_MODIFIERS, IsWindowEnabled, RegisterHotKey, SetFocus,
    UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_PUSHBUTTON, BS_PUSHLIKE, CS_HREDRAW, CS_VREDRAW,
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetSystemMetrics, HMENU, HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW, IDOK, IsChild, KillTimer,
    LoadCursorW, MB_ICONWARNING, MB_OKCANCEL, MSG, MessageBoxW, MoveWindow, NONCLIENTMETRICSW,
    PostMessageW, PostQuitMessage, RegisterClassExW, SC_MINIMIZE, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETNONCLIENTMETRICS, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SendMessageW, SetTimer, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_DPICHANGED,
    WM_HOTKEY, WM_SETFONT, WM_SIZE, WM_SYSCOMMAND, WM_TIMER, WNDCLASSEXW, WS_CAPTION, WS_CHILD,
    WS_CLIPCHILDREN, WS_DISABLED, WS_EX_APPWINDOW, WS_MAXIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
    WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

#[cfg(test)]
#[path = "window_tests.rs"]
mod tests;

/// What the window hands to the app.
#[derive(Debug)]
pub(crate) enum Input {
    Created,
    Rdp(RdpEvent),
    Command(PipCommand),
    /// Core closed the window's standard input.
    CommandsEnded,
    /// A click of the user's into the picture while it is watched.
    Clicked,
    StopKeys,
    StopButton,
    ReconnectButton,
    CloseAsked,
    Tick,
}

const COMMAND: u32 = WM_APP + 1;
static COMMAND_QUEUE: Mutex<VecDeque<PipCommand>> = Mutex::new(VecDeque::new());
const COMMANDS_ENDED: u32 = WM_APP + 2;
const RDP_EVENT: u32 = WM_APP + 3;
pub(crate) const CLICKED: u32 = WM_APP + 4;
const STOP_ID: usize = 101;
const RECONNECT_ID: usize = 102;
const TOPMOST_ID: usize = 103;
const STOP_KEYS_ID: i32 = 1;
const TICK_ID: usize = 1;
/// The row of controls, at 96 dots per inch.
const ROW: i32 = 36;

/// Posts to the window from other threads.
#[derive(Clone, Copy)]
pub(crate) struct Poster(isize);

impl Poster {
    pub(crate) fn command(self, command: PipCommand) {
        COMMAND_QUEUE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push_back(command);
        // SAFETY: the message only wakes the window; the command stays in a process-owned queue.
        let _ = unsafe { PostMessageW(Some(self.hwnd()), COMMAND, WPARAM(0), LPARAM(0)) };
    }

    pub(crate) fn commands_ended(self) {
        // SAFETY: a plain message to a window handle, which may be gone.
        let _ = unsafe { PostMessageW(Some(self.hwnd()), COMMANDS_ENDED, WPARAM(0), LPARAM(0)) };
    }

    fn hwnd(self) -> HWND {
        HWND(self.0 as *mut _)
    }
}

/// The window's parts, for the app.
pub(crate) struct Ui {
    main: HWND,
    status: HWND,
    stop: HWND,
    reconnect: HWND,
    topmost: HWND,
    rdp: Option<Rdp>,
    font: Option<HFONT>,
    stop_keys: Option<(HOT_KEY_MODIFIERS, u32)>,
}

type Handler = Box<dyn FnMut(&mut Ui, Input)>;

thread_local! {
    static APP: RefCell<Option<(Ui, Handler)>> = const { RefCell::new(None) };
    static PENDING: RefCell<VecDeque<Input>> = const { RefCell::new(VecDeque::new()) };
}

impl Ui {
    pub(crate) fn poster(&self) -> Poster {
        Poster(self.main.0 as isize)
    }

    pub(crate) fn rdp(&self) -> Option<&Rdp> {
        self.rdp.as_ref()
    }

    pub(crate) fn set_status(&self, text: &str) {
        // SAFETY: a child window of ours and a string valid for the call.
        let _ = unsafe { SetWindowTextW(self.status, &HSTRING::from(text)) };
    }

    pub(crate) fn enable_stop(&self, on: bool) {
        // SAFETY: a child window of ours.
        let _ = unsafe { EnableWindow(self.stop, on) };
    }

    pub(crate) fn show_reconnect(&self, on: bool) {
        // SAFETY: a child window of ours.
        let _ = unsafe { ShowWindow(self.reconnect, if on { SW_SHOW } else { SW_HIDE }) };
    }

    /// Whether the picture passes the user's input to the child session. Without input it also
    /// gives up the keyboard focus.
    pub(crate) fn pass_input(&self, on: bool) -> bool {
        if let Some(rdp) = &self.rdp {
            // SAFETY: windows of ours.
            unsafe {
                let _ = EnableWindow(rdp.window, on);
                let focus = GetFocus();
                if !on && (focus == rdp.window || IsChild(rdp.window, focus).as_bool()) {
                    let _ = SetFocus(Some(self.main));
                }
                return IsWindowEnabled(rdp.window).as_bool() == on;
            }
        }
        false
    }

    /// Watches the clicks into the picture, or stops.
    pub(crate) fn watch(&self, on: bool) -> bool {
        match (&self.rdp, on) {
            (Some(rdp), true) => hook::watch(self.main, rdp.window),
            _ => {
                hook::unwatch();
                !on
            }
        }
    }

    /// Registers the stop shortcut; false when another program has it.
    pub(crate) fn register_stop_keys(&mut self, modifiers: HOT_KEY_MODIFIERS, key: u32) -> bool {
        if self.stop_keys.take().is_some() {
            // SAFETY: this window registered this id; it is released before changing the chord.
            let _ = unsafe { UnregisterHotKey(Some(self.main), STOP_KEYS_ID) };
        }
        // SAFETY: a window of ours and a hotkey id it owns.
        let done = unsafe { RegisterHotKey(Some(self.main), STOP_KEYS_ID, modifiers, key) }.is_ok();
        if done {
            self.stop_keys = Some((modifiers, key));
        }
        done
    }

    pub(crate) fn confirm(&self, text: &str) -> bool {
        // SAFETY: a modal box owned by our window, with strings valid for the call.
        let answer = unsafe {
            MessageBoxW(
                Some(self.main),
                &HSTRING::from(text),
                w!("Aura 小窗"),
                MB_OKCANCEL | MB_ICONWARNING,
            )
        };
        answer == IDOK
    }

    pub(crate) fn tick(&self, on: bool) {
        // SAFETY: a timer of our own window.
        unsafe {
            if on {
                SetTimer(Some(self.main), TICK_ID, 250, None);
            } else {
                let _ = KillTimer(Some(self.main), TICK_ID);
            }
        }
    }

    /// Destroys the window, which ends the message loop.
    pub(crate) fn close(&mut self) {
        hook::unwatch();
        self.rdp = None;
        // SAFETY: our own window, destroyed once.
        let _ = unsafe { DestroyWindow(self.main) };
    }

    fn layout(&self) {
        let mut client = RECT::default();
        // SAFETY: windows of ours and writable storage for the rectangle.
        unsafe {
            let _ = GetClientRect(self.main, &raw mut client);
            let scale = |value: i32| value * GetDpiForWindow(self.main) as i32 / 96;
            let (row, gap, button) = (scale(ROW), scale(6), scale(84));
            let height = row - 2 * gap;
            let mut right = client.right - gap;
            for control in [self.topmost, self.stop, self.reconnect] {
                right -= button;
                let _ = MoveWindow(control, right, gap, button, height, true);
                right -= gap;
            }
            let _ = MoveWindow(self.status, gap, gap, (right - gap).max(0), height, true);
            if let Some(rdp) = &self.rdp {
                let _ = MoveWindow(
                    rdp.window,
                    0,
                    row,
                    client.right,
                    (client.bottom - row).max(0),
                    true,
                );
            }
        }
    }

    fn refresh_font(&mut self) {
        // SAFETY: reads the system's message font for the window's dots per inch, makes a font of
        // it, gives it to the controls and frees the one before.
        unsafe {
            let dpi = GetDpiForWindow(self.main);
            let mut metrics = NONCLIENTMETRICSW {
                cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
                ..Default::default()
            };
            if SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS.0,
                metrics.cbSize,
                Some((&raw mut metrics).cast()),
                0,
                dpi,
            )
            .is_err()
            {
                return;
            }
            let font = CreateFontIndirectW(&raw const metrics.lfMessageFont);
            for control in [self.status, self.stop, self.reconnect, self.topmost] {
                SendMessageW(control, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            }
            if let Some(old) = self.font.replace(font) {
                let _ = DeleteObject(HGDIOBJ(old.0));
            }
        }
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        // SAFETY: releases what this window registered and made.
        unsafe {
            if self.stop_keys.is_some() {
                let _ = UnregisterHotKey(Some(self.main), STOP_KEYS_ID);
            }
            if let Some(font) = self.font.take() {
                let _ = DeleteObject(HGDIOBJ(font.0));
            }
        }
    }
}

/// Makes the window, shows it without taking the focus, and runs its message loop until it is
/// destroyed. `handler` hears every [`Input`], starting with [`Input::Created`].
pub(crate) fn run(handler: impl FnMut(&mut Ui, Input) + 'static) -> Result<()> {
    // SAFETY: the window class and windows are made on this thread, which runs their loop below.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut _),
            lpszClassName: w!("AuraPipWindow"),
            ..Default::default()
        };
        if RegisterClassExW(&raw const class) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let (width, height) = (720, 450 + ROW);
        let (x, y) = (
            GetSystemMetrics(SM_CXSCREEN) - width - 48,
            GetSystemMetrics(SM_CYSCREEN) - height - 96,
        );
        let main = CreateWindowExW(
            WS_EX_APPWINDOW,
            w!("AuraPipWindow"),
            w!("Aura 小窗"),
            WS_OVERLAPPED
                | WS_CAPTION
                | WS_SYSMENU
                | WS_THICKFRAME
                | WS_MAXIMIZEBOX
                | WS_CLIPCHILDREN,
            x.max(0),
            y.max(0),
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let child = |class: PCWSTR, text: PCWSTR, style: u32, id: usize| {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                text,
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                0,
                0,
                10,
                10,
                Some(main),
                Some(HMENU(id as *mut _)),
                Some(instance.into()),
                None,
            )
        };
        // Static styles: centred vertically, ellipsis at the end, no mnemonic prefix.
        let status = child(w!("STATIC"), w!("正在连接"), 0x200 | 0x4000 | 0x80, 0)?;
        let stop = child(w!("BUTTON"), w!("停止"), BS_PUSHBUTTON as u32 | WS_DISABLED.0, STOP_ID)?;
        let reconnect = child(w!("BUTTON"), w!("重新连接"), BS_PUSHBUTTON as u32, RECONNECT_ID)?;
        let topmost =
            child(w!("BUTTON"), w!("置顶"), (BS_AUTOCHECKBOX | BS_PUSHLIKE) as u32, TOPMOST_ID)?;
        let _ = ShowWindow(reconnect, SW_HIDE);
        let poster = Poster(main.0 as isize);
        let rdp = Rdp::create(main, move |event| {
            let (kind, code) = encode(event);
            // SAFETY: a plain message to our own window.
            let _ =
                PostMessageW(Some(poster.hwnd()), RDP_EVENT, WPARAM(kind), LPARAM(code as isize));
        })?;
        let mut ui = Ui {
            main,
            status,
            stop,
            reconnect,
            topmost,
            rdp: Some(rdp),
            font: None,
            stop_keys: None,
        };
        ui.refresh_font();
        ui.layout();
        APP.with(|app| *app.borrow_mut() = Some((ui, Box::new(handler))));
        let _ = ShowWindow(main, SW_SHOWNOACTIVATE);
        hand(Input::Created);
        let mut message = MSG::default();
        while GetMessageW(&raw mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
        APP.with(|app| app.borrow_mut().take());
    }
    Ok(())
}

fn encode(event: RdpEvent) -> (usize, i32) {
    match event {
        RdpEvent::Connecting => (1, 0),
        RdpEvent::Connected => (2, 0),
        RdpEvent::LoginComplete => (3, 0),
        RdpEvent::Disconnected(reason) => (4, reason),
        RdpEvent::FatalError(code) => (5, code),
        RdpEvent::SigningIn => (6, 0),
    }
}

fn decode(kind: usize, code: i32) -> Option<RdpEvent> {
    Some(match kind {
        1 => RdpEvent::Connecting,
        2 => RdpEvent::Connected,
        3 => RdpEvent::LoginComplete,
        4 => RdpEvent::Disconnected(code),
        5 => RdpEvent::FatalError(code),
        6 => RdpEvent::SigningIn,
        _ => return None,
    })
}

/// Hands `input` to the app, in order. A modal box the handler shows runs a message loop of its
/// own: what comes in meanwhile waits in line, and the handler gets it once the box is closed.
fn hand(input: Input) {
    PENDING.with(|pending| pending.borrow_mut().push_back(input));
    APP.with(|app| {
        let Ok(mut app) = app.try_borrow_mut() else {
            return;
        };
        let Some((ui, handler)) = app.as_mut() else {
            return;
        };
        while let Some(next) = PENDING.with(|pending| pending.borrow_mut().pop_front()) {
            handler(ui, next);
        }
    });
}

fn with_ui(work: impl FnOnce(&mut Ui)) {
    APP.with(|app| {
        if let Ok(mut app) = app.try_borrow_mut()
            && let Some((ui, _)) = app.as_mut()
        {
            work(ui);
        }
    });
}

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        COMMAND => {
            let command =
                COMMAND_QUEUE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).pop_front();
            if let Some(command) = command {
                hand(Input::Command(command));
            }
            LRESULT(0)
        }
        COMMANDS_ENDED => {
            hand(Input::CommandsEnded);
            LRESULT(0)
        }
        RDP_EVENT => {
            if let Some(event) = decode(wparam.0, lparam.0 as i32) {
                hand(Input::Rdp(event));
            }
            LRESULT(0)
        }
        CLICKED => {
            hand(Input::Clicked);
            LRESULT(0)
        }
        WM_HOTKEY if wparam.0 == STOP_KEYS_ID as usize => {
            hand(Input::StopKeys);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TICK_ID => {
            hand(Input::Tick);
            LRESULT(0)
        }
        WM_COMMAND if (wparam.0 >> 16) as u32 == BN_CLICKED => {
            match wparam.0 & 0xffff {
                STOP_ID => hand(Input::StopButton),
                RECONNECT_ID => hand(Input::ReconnectButton),
                TOPMOST_ID => with_ui(|ui| {
                    // SAFETY: windows of ours.
                    unsafe {
                        let on = SendMessageW(ui.topmost, BM_GETCHECK, None, None).0 == 1;
                        let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
                        let _ = SetWindowPos(
                            ui.main,
                            Some(after),
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                    }
                }),
                _ => {}
            }
            LRESULT(0)
        }
        WM_SIZE => {
            with_ui(|ui| ui.layout());
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // SAFETY: Windows passes the suggested rectangle for the new dots per inch.
            let suggested = unsafe { &*(lparam.0 as *const RECT) };
            // SAFETY: our own window.
            let _ = unsafe {
                SetWindowPos(
                    window,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOACTIVATE,
                )
            };
            with_ui(|ui| {
                ui.refresh_font();
                ui.layout();
            });
            LRESULT(0)
        }
        // A minimized Remote Desktop window may stop the child session's picture.
        WM_SYSCOMMAND if (wparam.0 & 0xfff0) as u32 == SC_MINIMIZE => LRESULT(0),
        WM_CLOSE => {
            hand(Input::CloseAsked);
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: everything else as Windows does it.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
