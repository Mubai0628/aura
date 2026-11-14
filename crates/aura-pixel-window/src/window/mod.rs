//! The window: a layered top-level window that shows the page's picture with its shadow, at the
//! whole scale its screen's DPI calls for, centred in the work area. A page runs until the user
//! picks a button or link or closes it; work runs on another thread while the window keeps
//! drawing its progress.
mod input;
mod proc;
mod surface;

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Once,
        atomic::{AtomicBool, Ordering},
    },
};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
        Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
        System::{
            Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx},
            LibraryLoader::GetModuleHandleW,
        },
        UI::{
            HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos,
                GetMessageW, IDC_ARROW, KillTimer, LoadCursorW, MSG, PostMessageW, PostQuitMessage,
                RegisterClassExW, SW_SHOW, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
                ShowWindow, TranslateMessage, WM_APP, WM_TIMER, WNDCLASSEXW, WS_EX_APPWINDOW,
                WS_EX_LAYERED, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
            },
        },
    },
    core::{HSTRING, Result, w},
};

use crate::{
    a11y::Shared,
    layout::{Laid, Look, Target, lay_out},
    page::{Action, Page},
    render::Painter,
    scale::fitting_scale,
    text::{Fonts, fonts},
};

pub(crate) const WM_WORK_DONE: u32 = WM_APP + 1;
pub(crate) const WM_A11Y_INVOKE: u32 = WM_APP + 2;
pub(crate) const WM_A11Y_FOCUS: u32 = WM_APP + 3;
const TIMER: usize = 1;
const TICK_MS: u32 = 100;
const CLASS: windows::core::PCWSTR = w!("AuraPixelWindow");

/// Set by tests: windows are made but never shown, so nothing takes the screen or the keyboard.
pub(crate) static HIDDEN: AtomicBool = AtomicBool::new(false);

pub(crate) struct State {
    pub(crate) hwnd: HWND,
    pub(crate) page: Page,
    pub(crate) laid: Laid,
    pub(crate) look: Look,
    pub(crate) k: i32,
    pub(crate) dpi: u32,
    /// The window's top-left on the screen.
    pub(crate) position: (i32, i32),
    pub(crate) outcome: Option<Action>,
    pub(crate) work_done: bool,
    pub(crate) tracking: bool,
    pub(crate) ticking: bool,
    pub(crate) fonts: Rc<Fonts>,
    pub(crate) painter: Painter,
    pub(crate) surface: Option<surface::Surface>,
    pub(crate) a11y: Rc<Shared>,
}

impl State {
    /// Lays the page out again for the current look and scale and tells screen readers.
    pub(crate) fn relayout(&mut self) {
        self.laid = lay_out(&self.page, &self.look, &*self.fonts, self.k);
        let focus = self
            .look
            .focus
            .and_then(|index| self.laid.controls.get(index))
            .map(|control| control.node);
        self.a11y.update(self.laid.nodes.clone(), focus, self.position, self.k);
    }

    /// Lays out, draws and puts the picture on the screen.
    pub(crate) fn paint(&mut self) {
        self.relayout();
        let Ok(image) = self.painter.paint(&self.laid.scene, self.k, &self.fonts) else { return };
        let fits = self.surface.as_ref().is_some_and(|surface| surface.fits(&image));
        if !fits {
            self.surface = surface::Surface::new(image.width, image.height).ok();
        }
        if let Some(surface) = &mut self.surface {
            let _ = surface.show(self.hwnd, &image, self.position);
        }
    }

    /// A new page: focus on its primary button (or the first control), shown when the keyboard
    /// moves it. A page with the same controls as the one before (shown again after a change)
    /// keeps the focus where it was and is the same page to screen readers.
    pub(crate) fn set_page(&mut self, page: Page) {
        let before: Vec<Target> = self.laid.controls.iter().map(|control| control.target).collect();
        let kept = (self.look.focus, self.look.cues);
        self.page = page;
        self.look = Look { phase: self.look.phase, ..Look::default() };
        self.outcome = None;
        self.relayout();
        let same =
            before == self.laid.controls.iter().map(|control| control.target).collect::<Vec<_>>();
        if same
            && kept.0.is_some_and(|focus| {
                self.laid.controls.get(focus).is_some_and(|control| control.tab_stop)
            })
        {
            (self.look.focus, self.look.cues) = kept;
            self.paint();
            return;
        }
        let primary =
            self.laid.controls.iter().position(|control| control.primary && control.enabled);
        self.look.focus =
            primary.or_else(|| self.laid.controls.iter().position(|control| control.tab_stop));
        self.a11y.next_generation();
        self.paint();
        self.a11y.page_changed();
    }

    fn set_ticking(&mut self, on: bool) {
        if on == self.ticking {
            return;
        }
        // SAFETY: the timer belongs to this state's own window.
        unsafe {
            if on {
                SetTimer(Some(self.hwnd), TIMER, TICK_MS, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER);
            }
        }
        self.ticking = on;
    }
}

pub struct PixelWindow {
    hwnd: HWND,
    state: Rc<RefCell<State>>,
}

/// Where a new window goes: the screen under the pointer, its DPI and its work area.
fn placement() -> (u32, RECT) {
    // SAFETY: plain queries into locals.
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let (mut dpi, mut unused) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
        let mut info =
            MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        (dpi, info.rcWork)
    }
}

fn register_class() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        // SAFETY: registers this crate's window class once for the process.
        unsafe {
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc::window_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassExW(&class);
        }
    });
}

impl PixelWindow {
    /// Makes the window for `page` and shows it.
    pub fn open(page: &Page) -> Result<Self> {
        // COM stays on for the thread: the thread's fonts and factories outlive any one window.
        // A thread already in the multithreaded apartment keeps it.
        // SAFETY: initialises COM for this thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        register_class();
        let fonts = fonts()?;
        let painter = Painter::new()?;
        let (dpi, work) = placement();
        let first = lay_out(page, &Look::default(), &*fonts, 1);
        let work_size = (work.right - work.left, work.bottom - work.top);
        let k = fitting_scale(dpi, (first.scene.width, first.scene.height), work_size);
        let laid = lay_out(page, &Look::default(), &*fonts, k);
        let (width, height) = (laid.scene.width * k, laid.scene.height * k);
        let position =
            (work.left + (work_size.0 - width) / 2, work.top + (work_size.1 - height) / 2);
        // SAFETY: creates a window of the class registered above; its state is attached below,
        // before any message that reads it can arrive from the message loop.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_APPWINDOW,
                CLASS,
                &HSTRING::from(page.title.as_str()),
                WS_POPUP | WS_SYSMENU | WS_MINIMIZEBOX,
                position.0,
                position.1,
                width,
                height,
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?
        };
        let state = Rc::new(RefCell::new(State {
            hwnd,
            page: page.clone(),
            laid,
            look: Look::default(),
            k,
            dpi,
            position,
            outcome: None,
            work_done: false,
            tracking: false,
            ticking: false,
            fonts,
            painter,
            surface: None,
            a11y: Shared::new(hwnd),
        }));
        // SAFETY: the window keeps one count of the state, given back in WM_NCDESTROY.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::into_raw(Rc::clone(&state)) as isize);
        }
        state.borrow_mut().set_page(page.clone());
        if !HIDDEN.load(Ordering::Acquire) {
            // SAFETY: shows the window just made.
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
            }
        }
        Ok(Self { hwnd, state })
    }

    /// The window's handle, as a number, for dialogs it owns.
    pub fn handle(&self) -> isize {
        self.hwnd.0 as isize
    }

    /// Shows `page` until the user picks something; the page keeps what they checked.
    pub fn show(&mut self, page: &mut Page) -> Result<Action> {
        {
            let mut state = self.state.borrow_mut();
            state.set_page(page.clone());
            let animated = state.page.animated();
            state.set_ticking(animated);
        }
        self.pump(|state| state.outcome.is_some(), &mut |_| {});
        let mut state = self.state.borrow_mut();
        state.set_ticking(false);
        *page = state.page.clone();
        Ok(state.outcome.take().unwrap_or(Action::Close))
    }

    /// Shows `page` while `work` runs on another thread; `refresh` brings the page up to date
    /// every tick. Closing does nothing while the work runs. Returns what the work returned.
    pub fn run_work<R: Send>(
        &mut self,
        page: &Page,
        mut refresh: impl FnMut(&mut Page),
        work: impl FnOnce() -> R + Send,
    ) -> R {
        {
            let mut state = self.state.borrow_mut();
            state.set_page(page.clone());
            state.work_done = false;
            state.set_ticking(true);
        }
        let window = self.hwnd.0 as isize;
        let result = std::thread::scope(|scope| {
            let worker = scope.spawn(move || {
                // Posted even when the work panics, so the loop below always ends.
                struct Done(isize);
                impl Drop for Done {
                    fn drop(&mut self) {
                        // SAFETY: posts to the window, which outlives this thread.
                        unsafe {
                            let _ = PostMessageW(
                                Some(HWND(self.0 as *mut _)),
                                WM_WORK_DONE,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    }
                }
                let _done = Done(window);
                work()
            });
            self.pump(|state| state.work_done, &mut refresh);
            worker.join()
        });
        self.state.borrow_mut().set_ticking(false);
        match result {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// Shows `page` until `until` says the work outside this thread ended; `refresh` brings the
    /// page up to date every tick and `pressed` hears each button pressed meanwhile.
    pub fn run_until(
        &mut self,
        page: &Page,
        until: impl Fn() -> bool,
        mut refresh: impl FnMut(&mut Page),
        mut pressed: impl FnMut(Action),
    ) {
        {
            let mut state = self.state.borrow_mut();
            state.set_page(page.clone());
            state.set_ticking(true);
        }
        loop {
            self.pump(|state| until() || state.outcome.is_some(), &mut refresh);
            let outcome = self.state.borrow_mut().outcome.take();
            match outcome {
                // The work ended, or the thread was asked to quit: the window goes.
                None | Some(Action::Close) => break,
                Some(action) => pressed(action),
            }
        }
        self.state.borrow_mut().set_ticking(false);
    }

    /// Runs the thread's messages until `done`; `tick` refreshes the page on each timer tick.
    fn pump(&self, done: impl Fn(&State) -> bool, tick: &mut dyn FnMut(&mut Page)) {
        loop {
            if done(&self.state.borrow()) {
                return;
            }
            let mut message = MSG::default();
            // SAFETY: the thread's own message loop.
            let got = unsafe { GetMessageW(&mut message, None, 0, 0) };
            if got.0 <= 0 {
                // Someone asked the thread to quit: the page ends as if closed, and the quit is
                // passed on to any loop outside this one.
                // SAFETY: re-posts the quit request.
                unsafe { PostQuitMessage(message.wParam.0 as i32) };
                self.state.borrow_mut().outcome.get_or_insert(Action::Close);
                self.state.borrow_mut().work_done = true;
                return;
            }
            // SAFETY: dispatches the message just taken off the queue.
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if message.message == WM_TIMER && message.hwnd == self.hwnd {
                let mut state = self.state.borrow_mut();
                tick(&mut state.page);
                state.paint();
            }
        }
    }
}

impl Drop for PixelWindow {
    fn drop(&mut self) {
        // SAFETY: destroys the window this value made.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
