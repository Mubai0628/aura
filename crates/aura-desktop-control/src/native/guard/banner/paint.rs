//! Drawing the banner with GDI: its words in Aura's fonts, with ClearType on its opaque face,
//! then its pixels finished and the program's icon blended in, as [`crate::banner`] lays them out.
use super::Icon;
use crate::banner::{self, Layout, Measured, Words, colour};
use aura_desktop_domain::Rect;
use windows::Win32::Foundation::{COLORREF, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, FONT_WEIGHT, FW_BOLD, FW_NORMAL, FillRect, GdiFlush,
    GetStockObject, GetTextExtentPoint32W, GetTextFaceW, GetTextMetricsW, HBITMAP, HDC, HFONT,
    HGDIOBJ, OUT_TT_PRECIS, SYSTEM_FONT, SelectObject, SetBkMode, SetTextAlign, SetTextColor,
    TA_BASELINE, TA_LEFT, TEXTMETRICW, TRANSPARENT, TextOutW,
};
use windows::core::HSTRING;

/// The words' sizes at 100 %: the plain and bold words, and a program's name in the code font.
const TEXT_PX: f32 = 13.0;
const CODE_PX: f32 = 12.0;
const UI_FACE: &str = "Microsoft YaHei UI";
const CODE_FACES: [&str; 2] = ["Cascadia Mono", "Consolas"];

struct Dc(HDC);

impl Drop for Dc {
    fn drop(&mut self) {
        // SAFETY: deletes the drawing context this value made.
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

struct Font(HFONT);

impl Drop for Font {
    fn drop(&mut self) {
        // SAFETY: deletes the font this value made; it is no longer selected anywhere.
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

/// A drawn banner, its shadow included: a section of 32-bit pixels selected into its own drawing
/// context, ready for `UpdateLayeredWindow`.
pub(super) struct Canvas {
    dc: Dc,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u32,
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) layout: Layout,
}

impl Canvas {
    pub(super) fn dc(&self) -> HDC {
        self.dc.0
    }

    /// 0xAARRGGBB, row by row: opaque on the banner and its shadow, clear elsewhere.
    pub(super) fn pixels(&self) -> &[u32] {
        // SAFETY: the section holds `width` × `height` pixels for as long as the canvas lives.
        unsafe { std::slice::from_raw_parts(self.bits, (self.width * self.height) as usize) }
    }

    fn pixels_mut(&mut self) -> &mut [u32] {
        // SAFETY: as in `pixels`; GDI has finished drawing into it (GdiFlush).
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize) }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: puts back what the section replaced, then deletes the section; the drawing
        // context goes with `dc`.
        unsafe {
            SelectObject(self.dc.0, self.previous);
            let _ = DeleteObject(self.bitmap.into());
        }
    }
}

fn colorref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff))
}

fn font(px: i32, weight: FONT_WEIGHT, face: &str) -> Font {
    // SAFETY: makes a font from plain values; it is deleted when the value drops.
    Font(unsafe {
        CreateFontW(
            -px,
            0,
            0,
            0,
            weight.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            &HSTRING::from(face),
        )
    })
}

fn select(dc: HDC, font: &Font) {
    // SAFETY: selects a live font into this module's own drawing context.
    unsafe {
        SelectObject(dc, font.0.into());
    }
}

/// Selects the system's own font, so that this module's fonts can be deleted.
fn deselect(dc: HDC) {
    // SAFETY: a stock object, which is never deleted.
    unsafe {
        SelectObject(dc, GetStockObject(SYSTEM_FONT));
    }
}

/// The code font: the first of [`CODE_FACES`] the computer has; Windows otherwise draws another.
fn code_font(dc: HDC, px: i32) -> Font {
    for face in CODE_FACES {
        let font = font(px, FW_NORMAL, face);
        select(dc, &font);
        let mut name = [0_u16; 64];
        // SAFETY: writes the selected font's face name into the buffer above.
        let length = unsafe { GetTextFaceW(dc, Some(&mut name)) };
        deselect(dc);
        let found = String::from_utf16_lossy(&name[..length.clamp(0, 64) as usize]);
        if found.trim_end_matches('\0').eq_ignore_ascii_case(face) {
            return font;
        }
    }
    font(px, FW_NORMAL, CODE_FACES[1])
}

fn width(dc: HDC, font: &Font, text: &[u16]) -> i32 {
    select(dc, font);
    let mut size = SIZE::default();
    // SAFETY: measures the text with the selected font into the local above.
    let _ = unsafe { GetTextExtentPoint32W(dc, text, &raw mut size) };
    size.cx
}

/// How far the font reaches above and below its baseline.
fn reach(dc: HDC, font: &Font) -> (i32, i32) {
    select(dc, font);
    let mut metrics = TEXTMETRICW::default();
    // SAFETY: reads the selected font's metrics into the local above.
    let _ = unsafe { GetTextMetricsW(dc, &raw mut metrics) };
    (metrics.tmAscent, metrics.tmDescent)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// A program's name in runs, each with whether it is drawn in the code font: ASCII is, other
/// characters are drawn in the plain font, as a browser falls back for a code font without them.
fn runs(name: &str) -> Vec<(bool, Vec<u16>)> {
    let mut runs: Vec<(bool, String)> = Vec::new();
    for character in name.chars() {
        let code = character.is_ascii();
        match runs.last_mut() {
            Some((last, run)) if *last == code => run.push(character),
            _ => runs.push((code, character.to_string())),
        }
    }
    runs.into_iter().map(|(code, run)| (code, wide(&run))).collect()
}

fn fill(dc: HDC, rect: Rect, rgb: u32) {
    let area = RECT {
        left: rect.x,
        top: rect.y,
        right: rect.x + rect.width,
        bottom: rect.y + rect.height,
    };
    // SAFETY: fills a rectangle of this module's own section with a brush deleted right after.
    unsafe {
        let brush = CreateSolidBrush(colorref(rgb));
        FillRect(dc, &raw const area, brush);
        let _ = DeleteObject(brush.into());
    }
}

fn text(dc: HDC, font: &Font, rgb: u32, x: i32, baseline: i32, words: &[u16]) {
    select(dc, font);
    // SAFETY: draws text into this module's own section.
    unsafe {
        SetTextColor(dc, colorref(rgb));
        let _ = TextOutW(dc, x, baseline, words);
    }
}

/// A small window drawn in the muted colour where a program has no icon.
fn no_icon(pixels: &mut [u32], width: usize, at: Rect, scale: f32) {
    let line = (scale.round() as i32).max(1);
    let bar = ((3.0 * scale).round() as i32).max(2);
    let frame = Rect {
        x: at.x + line,
        y: at.y + 2 * line,
        width: at.width - 2 * line,
        height: at.height - 4 * line,
    };
    for y in frame.y..frame.y + frame.height {
        for x in frame.x..frame.x + frame.width {
            let edge = x < frame.x + line
                || x >= frame.x + frame.width - line
                || y < frame.y + bar
                || y >= frame.y + frame.height - line;
            if edge
                && x >= 0
                && y >= 0
                && let Some(pixel) = pixels.get_mut(y as usize * width + x as usize)
            {
                *pixel = 0xff00_0000 | colour::MUTED;
            }
        }
    }
}

/// The banner saying `words`, with the program's `icon` when it is drawn at this scale's size.
pub(super) fn paint(words: &Words, icon: Option<&Icon>, scale: f32) -> anyhow::Result<Canvas> {
    let scale = scale.clamp(0.5, 4.0);
    // SAFETY: a drawing context of this function's own, deleted when `dc` drops.
    let dc = Dc(unsafe { CreateCompatibleDC(None) });
    anyhow::ensure!(!dc.0.is_invalid(), "建不了提示条的绘图环境");
    let px = |value: f32| ((value * scale).round() as i32).max(1);
    let (plain, bold) =
        (font(px(TEXT_PX), FW_NORMAL, UI_FACE), font(px(TEXT_PX), FW_BOLD, UI_FACE));
    let code = code_font(dc.0, px(CODE_PX));
    let (lead, hint, stop) = (wide(words.lead), wide(&words.hint), wide(words.stop));
    let name = runs(&words.name);
    let pick = |in_code: bool| if in_code { &code } else { &plain };
    let (ascent, descent) = [&plain, &bold, &code]
        .into_iter()
        .map(|font| reach(dc.0, font))
        .fold((0, 0), |(up, down), (a, d)| (up.max(a), down.max(d)));
    let measured = Measured {
        lead: width(dc.0, &bold, &lead),
        name: name.iter().map(|(in_code, run)| width(dc.0, pick(*in_code), run)).sum(),
        hint: width(dc.0, &plain, &hint),
        stop: width(dc.0, &bold, &stop),
        text: ascent + descent,
    };
    deselect(dc.0);
    let layout = banner::layout(measured, scale);
    let (full_width, full_height) = (layout.width + layout.shadow, layout.height + layout.shadow);
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: full_width,
            biHeight: -full_height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    // SAFETY: a section of this function's drawing context, selected into it until the canvas
    // drops.
    let mut canvas = unsafe {
        let bitmap =
            CreateDIBSection(Some(dc.0), &raw const info, DIB_RGB_COLORS, &raw mut bits, None, 0)?;
        let previous = SelectObject(dc.0, bitmap.into());
        Canvas {
            dc,
            bitmap,
            previous,
            bits: bits.cast(),
            width: full_width,
            height: full_height,
            layout,
        }
    };
    let dc = canvas.dc();
    let face = Rect { x: 0, y: 0, width: layout.width, height: layout.height };
    let shadow = Rect { x: layout.shadow, y: layout.shadow, ..face };
    let inner = Rect {
        x: layout.border,
        y: layout.border,
        width: layout.width - 2 * layout.border,
        height: layout.height - 2 * layout.border,
    };
    fill(dc, shadow, colour::SHADOW);
    fill(dc, face, colour::EDGE);
    fill(dc, inner, colour::FACE);
    fill(dc, layout.mark, colour::ACCENT);
    fill(dc, layout.chip, colour::CHIP);
    fill(dc, layout.button, colour::ACCENT);
    // SAFETY: drawing settings of this function's own drawing context.
    unsafe {
        SetBkMode(dc, TRANSPARENT);
        SetTextAlign(dc, TA_BASELINE | TA_LEFT);
    }
    let baseline = layout.lead.y + ascent;
    text(dc, &bold, colour::INK, layout.lead.x, baseline, &lead);
    let mut x = layout.name.x;
    for (in_code, run) in &name {
        text(dc, pick(*in_code), colour::INK, x, baseline, run);
        x += width(dc, pick(*in_code), run);
    }
    text(dc, &plain, colour::MUTED, layout.hint.x, baseline, &hint);
    text(dc, &bold, colour::ON_ACCENT, layout.stop.x, baseline, &stop);
    deselect(dc);
    // SAFETY: lets GDI finish drawing before the pixels are read and written.
    unsafe {
        let _ = GdiFlush();
    }
    let row = full_width as usize;
    let pixels = canvas.pixels_mut();
    banner::finish(pixels, row, &[face, shadow]);
    let side = layout.icon.width;
    match icon
        .filter(|icon| icon.side as i32 == side && icon.pixels.len() == (side * side) as usize)
    {
        Some(icon) => {
            banner::blend(pixels, row, (layout.icon.x, layout.icon.y), &icon.pixels, side as usize)
        }
        None => no_icon(pixels, row, layout.icon, scale),
    }
    Ok(canvas)
}
