//! Puts a drawn picture on the screen: a 32-bit section in its own drawing context, handed to
//! `UpdateLayeredWindow` with per-pixel alpha, so the shadow shows what is behind it.
use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, POINT, SIZE},
        Graphics::Gdi::{
            AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
            CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC,
            HBITMAP, HDC, HGDIOBJ, ReleaseDC, SelectObject,
        },
        UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow},
    },
    core::Result,
};

use crate::image::Image;

pub(crate) struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u32,
    width: u32,
    height: u32,
}

impl Surface {
    pub(crate) fn new(width: u32, height: u32) -> Result<Self> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                // Negative: rows run top to bottom, as the picture's do.
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: makes a drawing context and a section of `width` × `height` pixels selected
        // into it; both are deleted on drop.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            {
                Ok(bitmap) => bitmap,
                Err(error) => {
                    let _ = DeleteDC(dc);
                    return Err(error);
                }
            };
            let previous = SelectObject(dc, bitmap.into());
            Ok(Self { dc, bitmap, previous, bits: bits.cast(), width, height })
        }
    }

    pub(crate) fn fits(&self, image: &Image) -> bool {
        self.width == image.width && self.height == image.height
    }

    /// Copies `image` in and shows it with its top-left at `position`.
    pub(crate) fn show(&mut self, window: HWND, image: &Image, position: (i32, i32)) -> Result<()> {
        let count = (self.width * self.height) as usize;
        // SAFETY: the section holds `count` pixels for as long as this value lives, and the
        // picture has as many.
        unsafe {
            std::slice::from_raw_parts_mut(self.bits, count)
                .copy_from_slice(&image.pixels[..count]);
        }
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let at = POINT { x: position.0, y: position.1 };
        let size = SIZE { cx: self.width as i32, cy: self.height as i32 };
        let origin = POINT::default();
        // SAFETY: the screen's context is released right after; every pointer is to a local.
        unsafe {
            let screen = GetDC(None);
            let result = UpdateLayeredWindow(
                window,
                Some(screen),
                Some(&at),
                Some(&size),
                Some(self.dc),
                Some(&origin),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            ReleaseDC(None, screen);
            result
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: puts back what the section replaced, then deletes the section and the context.
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}
