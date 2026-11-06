//! The pointer's marks over the screen: a layered window that lets every click through, never
//! takes the focus and is left out of every capture, drawn from [`crate::pointer_marks`].
use super::{EFFECTS, current, lock, now};
use crate::pointer_marks::{self, Frame};
use aura_desktop_domain::{Point, Rect};
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, MONITOR_DEFAULTTONEAREST, MonitorFromPoint, SelectObject,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

/// A drawing surface a little larger than needed, so that a growing ripple does not make a new
/// one each frame.
const GROW_PX: u32 = 64;

struct Surface {
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u32,
    width: u32,
    height: u32,
}

pub(super) struct Marker {
    window: HWND,
    memory: HDC,
    surface: Option<Surface>,
    /// What the window shows now; nothing is drawn again while it stays the same.
    shown: Option<Frame>,
}

/// Whether the marks' window shows, whether captures leave it out, and where it is.
#[must_use]
pub fn marker_status() -> Option<(bool, bool, Rect)> {
    // SAFETY: queries only the guard's known window class; no other window content is read.
    unsafe {
        let window = FindWindowW(w!("AuraDesktopPointerMarks"), None).ok()?;
        let mut affinity = 0;
        GetWindowDisplayAffinity(window, &raw mut affinity).ok()?;
        let mut rect = RECT::default();
        GetWindowRect(window, &raw mut rect).ok()?;
        Some((
            IsWindowVisible(window).as_bool(),
            affinity == WDA_EXCLUDEFROMCAPTURE.0,
            Rect {
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
            },
        ))
    }
}

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: standard handling for this thread's window.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// The screen's scale where the pointer is, 1 at 100 %.
fn scale_at(x: i32, y: i32) -> f32 {
    let (mut dpi, mut unused) = (96_u32, 96_u32);
    // SAFETY: plain queries of the monitor nearest a point.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut dpi, &raw mut unused);
    }
    dpi as f32 / 96.0
}

impl Marker {
    pub(super) unsafe fn new(instance: HINSTANCE) -> windows::core::Result<Self> {
        // SAFETY: the class, the window and the drawing context are owned by the hook thread.
        unsafe {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: w!("AuraDesktopPointerMarks"),
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                return Err(windows::core::Error::from_thread());
            }
            let window = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                class.lpszClassName,
                w!(""),
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
            let memory = CreateCompatibleDC(None);
            if memory.is_invalid() {
                let _ = DestroyWindow(window);
                return Err(windows::core::Error::from_thread());
            }
            Ok(Self { window, memory, surface: None, shown: None })
        }
    }

    /// Draws the marks as they are now: the ring beside the pointer while the desktop is kept,
    /// and the ripples and trails still fading. Returns whether anything shows.
    pub(super) fn refresh(&mut self) -> bool {
        let kept = current().is_some_and(|control| !control.taken_over());
        let pointer = crate::native::input::cursor();
        let scale = pointer.map_or(1.0, |(x, y)| scale_at(x, y));
        let frame = pointer_marks::frame(
            &lock(&EFFECTS),
            pointer.filter(|_| kept).map(|(x, y)| Point { x, y }),
            scale,
            now(),
        );
        match frame {
            Some(frame) => {
                if self.shown.as_ref() != Some(&frame) && self.draw(&frame) {
                    self.shown = Some(frame);
                }
                true
            }
            None => {
                if self.shown.take().is_some() {
                    // SAFETY: hides this thread's own window.
                    unsafe {
                        let _ = ShowWindow(self.window, SW_HIDE);
                    }
                }
                false
            }
        }
    }

    fn draw(&mut self, frame: &Frame) -> bool {
        if !self
            .surface
            .as_ref()
            .is_some_and(|surface| surface.width >= frame.width && surface.height >= frame.height)
            && !self.grow(frame.width, frame.height)
        {
            return false;
        }
        let Some(surface) = &self.surface else { return false };
        let width = frame.width as usize;
        // SAFETY: the section holds `surface.width` × `surface.height` pixels, at least the
        // frame's, and lives until it is replaced or the marker is dropped.
        unsafe {
            for (row, pixels) in frame.pixels.chunks_exact(width).enumerate() {
                let start = surface.bits.add(row * surface.width as usize);
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), start, width);
            }
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let (place, size, origin) = (
                POINT { x: frame.left, y: frame.top },
                SIZE { cx: frame.width as i32, cy: frame.height as i32 },
                POINT { x: 0, y: 0 },
            );
            let drawn = UpdateLayeredWindow(
                self.window,
                None,
                Some(&raw const place),
                Some(&raw const size),
                Some(self.memory),
                Some(&raw const origin),
                COLORREF(0),
                Some(&raw const blend),
                ULW_ALPHA,
            )
            .is_ok();
            if drawn && self.shown.is_none() {
                let _ = ShowWindow(self.window, SW_SHOWNOACTIVATE);
            }
            drawn
        }
    }

    fn grow(&mut self, width: u32, height: u32) -> bool {
        self.drop_surface();
        let (width, height) = (width.next_multiple_of(GROW_PX), height.next_multiple_of(GROW_PX));
        let header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let info = BITMAPINFO { bmiHeader: header, ..Default::default() };
        let mut bits = std::ptr::null_mut();
        // SAFETY: a section of this thread's drawing context, selected into it until replaced.
        unsafe {
            let Ok(bitmap) = CreateDIBSection(
                Some(self.memory),
                &raw const info,
                DIB_RGB_COLORS,
                &raw mut bits,
                None,
                0,
            ) else {
                return false;
            };
            let previous = SelectObject(self.memory, bitmap.into());
            self.surface = Some(Surface { bitmap, previous, bits: bits.cast(), width, height });
        }
        true
    }

    fn drop_surface(&mut self) {
        if let Some(surface) = self.surface.take() {
            // SAFETY: puts back what the section replaced, then deletes the section.
            unsafe {
                SelectObject(self.memory, surface.previous);
                let _ = DeleteObject(surface.bitmap.into());
            }
        }
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        self.drop_surface();
        // SAFETY: deletes this thread's drawing context and destroys its window.
        unsafe {
            let _ = DeleteDC(self.memory);
            let _ = DestroyWindow(self.window);
        }
    }
}
