//! Draws a scene with Direct2D into a premultiplied 32-bit bitmap, the same way for the window
//! and for the tests that look at it: shapes without antialiasing, the pixel face aliased, the
//! reading face in greyscale, never with ClearType's coloured edges.
use windows::{
    Win32::{
        Graphics::{
            Direct2D::{
                Common::{
                    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
                },
                D2D1_ANTIALIAS_MODE_ALIASED, D2D1_DRAW_TEXT_OPTIONS_CLIP,
                D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
                D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                D2D1_RENDER_TARGET_USAGE_NONE, D2D1_TEXT_ANTIALIAS_MODE_ALIASED,
                D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Factory,
                ID2D1RenderTarget, ID2D1SolidColorBrush,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Imaging::{
                CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
                WICBitmapCacheOnLoad,
            },
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    },
    core::Result,
};

use crate::{
    art,
    image::Image,
    page::Face,
    scene::{Item, Rect, Scene, Text},
    text::{Fonts, MEASURING},
    theme::{Rgb, SHADOW_OFFSET},
};

/// The factories a thread draws with, made once.
pub struct Painter {
    d2d: ID2D1Factory,
    wic: IWICImagingFactory,
}

impl Painter {
    /// Needs COM on this thread.
    pub fn new() -> Result<Self> {
        // SAFETY: creates the two factories; both are used only on this thread.
        unsafe {
            Ok(Self {
                d2d: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                wic: CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?,
            })
        }
    }

    /// The scene at whole scale `k`, its shadow included.
    pub fn paint(&self, scene: &Scene, k: i32, fonts: &Fonts) -> Result<Image> {
        let k = k.max(1);
        let width = ((scene.width + SHADOW_OFFSET) * k) as u32;
        let height = ((scene.height + SHADOW_OFFSET) * k) as u32;
        // SAFETY: draws into a bitmap this function owns, then copies its pixels out; every
        // pointer passed points at a local that outlives the call.
        unsafe {
            let bitmap = self.wic.CreateBitmap(
                width,
                height,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapCacheOnLoad,
            )?;
            let properties = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let target = self.d2d.CreateWicBitmapRenderTarget(&bitmap, &properties)?;
            target.BeginDraw();
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            target.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            let brush = target.CreateSolidColorBrush(&colour(0, 1.0), None)?;
            for item in &scene.items {
                draw(&target, &brush, item, k, fonts)?;
            }
            target.EndDraw(None, None)?;
            let stride = width * 4;
            let mut bytes = vec![0_u8; (stride * height) as usize];
            bitmap.CopyPixels(std::ptr::null(), stride, &mut bytes)?;
            let pixels = bytes
                .chunks_exact(4)
                .map(|pixel| u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]))
                .collect();
            Ok(Image { width, height, pixels })
        }
    }
}

fn colour(rgb: Rgb, alpha: f32) -> D2D1_COLOR_F {
    let channel = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
    D2D1_COLOR_F { r: channel(16), g: channel(8), b: channel(0), a: alpha }
}

fn rect(area: Rect, k: i32) -> D2D_RECT_F {
    let area = area.scaled(k);
    D2D_RECT_F {
        left: area.x as f32,
        top: area.y as f32,
        right: area.right() as f32,
        bottom: area.bottom() as f32,
    }
}

/// # Safety
/// `target` is between BeginDraw and EndDraw, and `brush` was made by it.
unsafe fn fill(
    target: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    area: Rect,
    rgb: Rgb,
    alpha: f32,
    k: i32,
) {
    // SAFETY: as the function's contract says.
    unsafe {
        brush.SetColor(&colour(rgb, alpha));
        target.FillRectangle(&rect(area, k), brush);
    }
}

/// # Safety
/// As for [`fill`].
unsafe fn draw(
    target: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    item: &Item,
    k: i32,
    fonts: &Fonts,
) -> Result<()> {
    // SAFETY: as the function's contract says.
    unsafe {
        match item {
            Item::Fill { rect: area, colour: rgb, alpha } => {
                fill(target, brush, *area, *rgb, *alpha, k)
            }
            Item::Art { art, x, y, cell, colour: rgb, .. } => {
                for layer in art.layers {
                    let ink = layer.colour.unwrap_or(*rgb);
                    for run in layer.cells() {
                        let area = Rect::new(
                            x + run.x * cell,
                            y + run.y * cell,
                            run.width * cell,
                            run.height * cell,
                        );
                        fill(target, brush, area, ink, 1.0, k);
                    }
                }
            }
            Item::Spinner { x, y, phase, colour: rgb } => {
                for (dot, (dx, dy)) in art::SPINNER_DOTS.iter().enumerate() {
                    let area = Rect::new(x + dx, y + dy, 2, 2);
                    fill(target, brush, area, *rgb, art::spinner_alpha(dot, *phase), k);
                }
            }
            Item::Text(text) => draw_text(target, brush, text, k, fonts)?,
        }
    }
    Ok(())
}

/// # Safety
/// As for [`fill`].
unsafe fn draw_text(
    target: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    text: &Text,
    k: i32,
    fonts: &Fonts,
) -> Result<()> {
    let format = fonts.format(
        text.face,
        text.size * k,
        text.line * k,
        text.baseline * k,
        text.wrap,
        text.right_align,
    )?;
    let wide: Vec<u16> = text.text.encode_utf16().collect();
    // SAFETY: as the function's contract says; the format and words outlive the call.
    unsafe {
        match text.face {
            Face::Pixel => {
                target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_ALIASED);
                target.SetTextRenderingParams(&fonts.pixel_params);
            }
            Face::Reading => {
                target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                target.SetTextRenderingParams(&fonts.reading_params);
            }
        }
        brush.SetColor(&colour(text.colour, 1.0));
        target.DrawText(
            &wide,
            &format,
            &rect(text.bounds(), k),
            brush,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            MEASURING,
        );
    }
    Ok(())
}
