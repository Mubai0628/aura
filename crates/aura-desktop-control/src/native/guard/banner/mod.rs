//! The banner while Aura keeps the desktop, drawn as [`crate::banner`] lays it out: a layered
//! window above the others that never takes the focus and that no capture shows. It stands on the
//! screen of the program Aura operates, goes back on top at once when another window covers it,
//! and steps out of the way of Aura's own pointer. A click on it gives the desktop back to the user.
mod look;
mod paint;

pub use look::{banner_on_top, banner_place, banner_status};

use super::{current, lock};
use crate::banner::{self, Spot};
use crate::native::input;
use aura_desktop_domain::{AppEntry, Point, Rect};
use look::{handle, on_top, screen_of};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

/// How often the banner looks again at which screen its program is on.
const LOOK_MS: u64 = 250;
/// Asks the banner's thread to step out of the way of the points in [`Place::points`].
const ROOM: u32 = WM_APP + 1;
const CLASS: PCWSTR = w!("AuraDesktopGuardBanner");
const ICONS_KEPT: usize = 16;

/// A program's icon on the banner: `side` pixels square, 0xAARRGGBB premultiplied, row by row.
pub struct Icon {
    side: u32,
    pixels: Vec<u32>,
}

/// Where the banner stands, shared with the commands that make room for Aura's pointer.
struct Place {
    spot: Spot,
    /// The banner's two places on its screen while it shows.
    top: Option<Rect>,
    bottom: Option<Rect>,
    /// Where the command about to run takes Aura's pointer.
    points: Vec<Point>,
}

static PLACE: Mutex<Place> =
    Mutex::new(Place { spot: Spot::Top, top: None, bottom: None, points: Vec::new() });
static WINDOW: AtomicIsize = AtomicIsize::new(0);
type Kept = (String, u32, Option<Arc<Icon>>);
static ICONS: Mutex<Vec<Kept>> = Mutex::new(Vec::new());

thread_local! {
    static BANNER: RefCell<Option<Banner>> = const { RefCell::new(None) };
}

/// What the banner shows; it is drawn again only when this changes.
#[derive(Clone, PartialEq)]
struct Shown {
    app: String,
    name: String,
    keys: Option<String>,
    icon: Option<u32>,
    scale: u32,
}

struct Banner {
    window: HWND,
    canvas: Option<paint::Canvas>,
    shown: Option<Shown>,
    /// Where the window stands; `None` while it is hidden.
    at: Option<Rect>,
    /// The work area and scale of the program's screen, and when they were looked at.
    screen: (Rect, f32),
    looked: Option<u64>,
    /// The command looked at last, by address.
    control: usize,
    /// The window events watched while the banner shows.
    watch: Option<Watch>,
    /// When the banner last went back on top, the latest last.
    raised: VecDeque<u64>,
}

/// The window events after which another window may stand above the banner: a window shown, the
/// order of the windows changed, another window in front. They reach [`on_event`] on the thread
/// that started watching, between the messages it pumps.
struct Watch([HWINEVENTHOOK; 2]);

impl Watch {
    fn start() -> Self {
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        // SAFETY: hooks for the calling thread's message loop, removed on the same thread.
        unsafe {
            Self([
                SetWinEventHook(
                    EVENT_OBJECT_SHOW,
                    EVENT_OBJECT_REORDER,
                    None,
                    Some(on_event),
                    0,
                    0,
                    flags,
                ),
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(on_event),
                    0,
                    0,
                    flags,
                ),
            ])
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        for hook in self.0 {
            if !hook.0.is_null() {
                // SAFETY: a hook this thread set.
                unsafe {
                    let _ = UnhookWinEvent(hook);
                }
            }
        }
    }
}

/// A window shown, the order of the windows changed, or another window in front: the banner goes
/// back on top if it is covered now.
unsafe extern "system" fn on_event(
    _: HWINEVENTHOOK,
    event: u32,
    _: HWND,
    object: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    if event == EVENT_OBJECT_HIDE || (event == EVENT_OBJECT_SHOW && object != OBJID_WINDOW.0) {
        return;
    }
    BANNER.with(|banner| {
        if let Ok(mut banner) = banner.try_borrow_mut()
            && let Some(banner) = banner.as_mut()
        {
            banner.raise();
        }
    });
}

/// The scale of the screen the program's window `front` is on.
pub(super) fn scale_of(front: Option<isize>) -> f32 {
    screen_of(front).1
}

/// The icon of `app` for a banner at `scale`, from the Shell the first time and kept after. The
/// calling thread must have joined COM.
pub(super) fn icon_for(app: &AppEntry, scale: f32) -> Option<Arc<Icon>> {
    let side = banner::icon_side(scale);
    if let Some((_, _, icon)) = lock(&ICONS).iter().find(|(id, at, _)| *id == app.id && *at == side)
    {
        return icon.clone();
    }
    let icon = crate::native::icon::pixels(&crate::desktop::parsing_name(app), side)
        .ok()
        .map(|pixels| Arc::new(Icon { side, pixels }));
    let mut icons = lock(&ICONS);
    if icons.len() >= ICONS_KEPT {
        icons.remove(0);
    }
    icons.push((app.id.clone(), side, icon.clone()));
    icon
}

impl Banner {
    /// Draws, places and shows the banner as the kept desktop's command wants it, every few
    /// milliseconds and when a command asks it to make room.
    fn refresh(&mut self) {
        let Some(control) = current().filter(|control| !control.taken_over()) else {
            self.hide();
            let mut place = lock(&PLACE);
            (place.top, place.bottom) = (None, None);
            return;
        };
        let now = super::now();
        let id = Arc::as_ptr(&control) as usize;
        let look = id != self.control || self.looked.is_none_or(|at| now >= at + LOOK_MS);
        if look {
            self.screen = screen_of(control.front);
            (self.looked, self.control) = (Some(now), id);
        }
        let (area, scale) = self.screen;
        let wanted = Shown {
            app: control.app_id.clone(),
            name: control.name.clone(),
            keys: control.stop_keys.as_ref().map(|(_, keys)| keys.clone()),
            icon: control.icon.as_ref().map(|icon| icon.side),
            scale: scale.to_bits(),
        };
        if self.shown.as_ref() != Some(&wanted) {
            let words = banner::words(&control.name, wanted.keys.as_deref());
            self.hide();
            self.canvas = paint::paint(&words, control.icon.as_deref(), scale).ok();
            self.shown = self.canvas.is_some().then_some(wanted);
        }
        let Some(canvas) = &self.canvas else { return };
        let size = (canvas.width, canvas.height);
        let top = banner::place(area, size, Spot::Top, scale);
        let bottom = banner::place(area, size, Spot::Bottom, scale);
        let spot = {
            let mut place = lock(&PLACE);
            (place.top, place.bottom) = (Some(top), Some(bottom));
            if !place.points.is_empty() {
                let points = std::mem::take(&mut place.points);
                place.spot = banner::spot_for(&points, place.spot, top, bottom, scale);
            }
            place.spot
        };
        match spot {
            Spot::Hidden => self.hide(),
            Spot::Top => self.show(top),
            Spot::Bottom => self.show(bottom),
        }
        // Also on every tick: a window that went on top without an event is covered here.
        self.raise();
    }

    /// Puts the banner back above another program's window that covers it, at once unless it
    /// has done so too often in the last second.
    fn raise(&mut self) {
        if self.at.is_none() || on_top(self.window) {
            return;
        }
        let now = super::now();
        if !banner::may_raise(self.raised.make_contiguous(), now) {
            return;
        }
        if self.raised.len() == banner::RAISES_AT_ONCE {
            self.raised.pop_front();
        }
        self.raised.push_back(now);
        // SAFETY: raises this thread's own window among the topmost ones, without the focus.
        unsafe {
            let _ = SetWindowPos(
                self.window,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }

    fn show(&mut self, rect: Rect) {
        if self.at == Some(rect) {
            return;
        }
        let Some(canvas) = &self.canvas else { return };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let (place, size, origin) = (
            POINT { x: rect.x, y: rect.y },
            SIZE { cx: canvas.width, cy: canvas.height },
            POINT { x: 0, y: 0 },
        );
        // SAFETY: draws this thread's own window from its canvas, and shows it on top without
        // the focus.
        unsafe {
            let drawn = UpdateLayeredWindow(
                self.window,
                None,
                Some(&raw const place),
                Some(&raw const size),
                Some(canvas.dc()),
                Some(&raw const origin),
                COLORREF(0),
                Some(&raw const blend),
                ULW_ALPHA,
            )
            .is_ok();
            if !drawn {
                return;
            }
            if self.at.is_none() {
                let _ = SetWindowPos(
                    self.window,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
        self.watch.get_or_insert_with(Watch::start);
        self.at = Some(rect);
    }

    fn hide(&mut self) {
        self.watch = None;
        if self.at.take().is_some() {
            // SAFETY: hides this thread's own window.
            unsafe {
                let _ = ShowWindow(self.window, SW_HIDE);
            }
        }
    }

    /// The stop button on the screen while the banner shows.
    fn button(&self) -> Option<Rect> {
        let (at, button) = (self.at?, self.canvas.as_ref()?.layout.button);
        Some(Rect { x: at.x + button.x, y: at.y + button.y, ..button })
    }
}

impl Drop for Banner {
    fn drop(&mut self) {
        // SAFETY: destroys this thread's own window.
        unsafe {
            let _ = DestroyWindow(self.window);
        }
    }
}

fn over_button() -> bool {
    let button = BANNER.with(|banner| {
        banner.try_borrow().ok().and_then(|banner| banner.as_ref().and_then(Banner::button))
    });
    button.zip(input::cursor()).is_some_and(|(button, (x, y))| button.contains(x, y))
}

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
            // SAFETY: the extra information of the input this message came from.
            let aura = unsafe { GetMessageExtraInfo() }.0 as usize == input::AURA_INPUT_MARK;
            if !aura && let Some(control) = current() {
                control.take_over();
            }
            return LRESULT(0);
        }
        WM_SETCURSOR if (lparam.0 & 0xffff) as u32 == HTCLIENT && over_button() => {
            // SAFETY: a system cursor, set for this window while the pointer is over it.
            unsafe {
                if let Ok(hand) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(Some(hand));
                }
            }
            return LRESULT(1);
        }
        ROOM => {
            refresh();
            return LRESULT(1);
        }
        WM_DPICHANGED => return LRESULT(0),
        _ => {}
    }
    // SAFETY: standard handling for this thread's window.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// The banner of the guard's thread, for as long as this lives.
pub(super) struct Started;

impl Drop for Started {
    fn drop(&mut self) {
        WINDOW.store(0, Ordering::SeqCst);
        let banner = BANNER.with_borrow_mut(Option::take);
        drop(banner);
        let mut place = lock(&PLACE);
        (place.top, place.bottom) = (None, None);
    }
}

/// Makes the banner's window on the calling thread, which pumps its messages and calls
/// [`refresh`] every few milliseconds.
pub(super) unsafe fn start(instance: HINSTANCE) -> windows::core::Result<Started> {
    // SAFETY: the class and the window are this thread's; the window is destroyed when `Started`
    // drops on this thread.
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let window = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!("Aura 电脑操控"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance),
            None,
        )?;
        if let Err(error) = SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE) {
            let _ = DestroyWindow(window);
            return Err(error);
        }
        BANNER.with_borrow_mut(|banner| {
            *banner = Some(Banner {
                window,
                canvas: None,
                shown: None,
                at: None,
                screen: (Rect::default(), 1.0),
                looked: None,
                control: 0,
                watch: None,
                raised: VecDeque::new(),
            });
        });
        WINDOW.store(window.0 as isize, Ordering::SeqCst);
        Ok(Started)
    }
}

/// Looked at every few milliseconds by the guard's thread.
pub(super) fn refresh() {
    BANNER.with(|banner| {
        if let Ok(mut banner) = banner.try_borrow_mut()
            && let Some(banner) = banner.as_mut()
        {
            banner.refresh();
        }
    });
}

/// A new hold begins: its banner stands at the top of the screen.
pub(super) fn hold_started() {
    lock(&PLACE).spot = Spot::Top;
}

/// A command ended: a banner hidden for it shows again, at the top.
pub(super) fn command_ended() {
    let mut place = lock(&PLACE);
    if place.spot == Spot::Hidden {
        place.spot = Spot::Top;
    }
}

/// Before a command takes Aura's pointer to `points`: the banner steps out of their way, to its
/// other place on the screen, or out of sight for the command when both places are in the way.
pub fn make_room(points: &[Point]) {
    let window = WINDOW.load(Ordering::SeqCst);
    if points.is_empty() || window == 0 {
        return;
    }
    lock(&PLACE).points = points.to_vec();
    // SAFETY: a message to the banner's window, which its thread answers between input events;
    // this thread holds no lock that thread takes.
    unsafe {
        let _ = SendMessageTimeoutW(
            handle(window),
            ROOM,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            1000,
            None,
        );
    }
}

/// The banner for `app` at `scale` as a PNG, for a person to look at: no capture shows the
/// banner itself. The calling thread must have joined COM.
pub fn banner_picture(
    app: &AppEntry,
    stop_keys: Option<&str>,
    scale: f32,
) -> anyhow::Result<Vec<u8>> {
    let icon = icon_for(app, scale);
    let canvas = paint::paint(&banner::words(&app.name, stop_keys), icon.as_deref(), scale)?;
    let bytes: Vec<u8> = canvas.pixels().iter().flat_map(|pixel| pixel.to_le_bytes()).collect();
    crate::native::image::png_of_bgra(
        canvas.width.unsigned_abs(),
        canvas.height.unsigned_abs(),
        &bytes,
    )
}
