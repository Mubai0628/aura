//! The two faces through DirectWrite: Fusion Pixel 12, carried in the program and loaded from
//! memory, and the system's reading face for names and paths. Text is measured with whole-pixel
//! advances, so the pixel face's squares land on pixels.
use std::{cell::RefCell, collections::HashMap, rc::Rc};

use windows::{
    Win32::Graphics::DirectWrite::{
        DWRITE_CONTAINER_TYPE_WOFF2, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_METRICS,
        DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
        DWRITE_GRID_FIT_MODE_DISABLED, DWRITE_LINE_SPACING_METHOD_UNIFORM,
        DWRITE_MEASURING_MODE_GDI_CLASSIC, DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
        DWRITE_PIXEL_GEOMETRY_FLAT, DWRITE_RENDERING_MODE_GDI_CLASSIC,
        DWRITE_RENDERING_MODE1_ALIASED, DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
        DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_EMERGENCY_BREAK,
        DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteFactory5,
        IDWriteFont, IDWriteFontCollection, IDWriteFontSetBuilder1, IDWriteRenderingParams,
        IDWriteTextFormat,
    },
    core::{HSTRING, Interface, Result},
};

use crate::{layout::Measure, page::Face};

/// Fusion Pixel Font 12px proportional, simplified Chinese (OFL-1.1), the same file the desktop
/// serves; its licences are in resources/third-party-notices.
static PIXEL_FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/desktop/renderer/src/fonts/fusion-pixel-12px-proportional-zh_hans.otf.woff2"
));
const READING_FAMILY: &str = "Microsoft YaHei UI";
const LOCALE: &str = "zh-CN";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct FormatKey {
    face: Face,
    size: i32,
    line: i32,
    baseline: i32,
    wrap: bool,
    right: bool,
}

pub struct Fonts {
    pub(crate) factory: IDWriteFactory,
    pixel: IDWriteFontCollection,
    pixel_family: HSTRING,
    pixel_font: IDWriteFont,
    pixel_metrics: DWRITE_FONT_METRICS,
    reading_metrics: DWRITE_FONT_METRICS,
    pub(crate) pixel_params: IDWriteRenderingParams,
    pub(crate) reading_params: IDWriteRenderingParams,
    formats: RefCell<HashMap<FormatKey, IDWriteTextFormat>>,
    widths: RefCell<HashMap<(String, Face, i32), i32>>,
}

thread_local! {
    static FONTS: RefCell<Option<Rc<Fonts>>> = const { RefCell::new(None) };
}

/// This thread's fonts, loaded on first use.
pub fn fonts() -> Result<Rc<Fonts>> {
    FONTS.with(|cell| {
        if let Some(fonts) = cell.borrow().as_ref() {
            return Ok(Rc::clone(fonts));
        }
        let fonts = Rc::new(Fonts::load()?);
        *cell.borrow_mut() = Some(Rc::clone(&fonts));
        Ok(fonts)
    })
}

impl Fonts {
    fn load() -> Result<Self> {
        // SAFETY: plain DirectWrite calls on objects this function owns; the unpacked font is
        // copied out of the stream's fragment before the fragment is released, and the in-memory
        // reference copies the bytes it is given (no owner object is passed).
        unsafe {
            let factory5: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let stream = factory5.UnpackFontFile(
                DWRITE_CONTAINER_TYPE_WOFF2,
                PIXEL_FONT.as_ptr().cast(),
                PIXEL_FONT.len() as u32,
            )?;
            let size = stream.GetFileSize()?;
            let mut start = std::ptr::null_mut();
            let mut context = std::ptr::null_mut();
            stream.ReadFileFragment(&mut start, 0, size, &mut context)?;
            let bytes = std::slice::from_raw_parts(start.cast::<u8>(), size as usize).to_vec();
            stream.ReleaseFileFragment(context);
            let loader = factory5.CreateInMemoryFontFileLoader()?;
            factory5.RegisterFontFileLoader(&loader)?;
            let file = loader.CreateInMemoryFontFileReference(
                &factory5,
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                None,
            )?;
            let builder: IDWriteFontSetBuilder1 = factory5.CreateFontSetBuilder()?;
            builder.AddFontFile(&file)?;
            let set = builder.CreateFontSet()?;
            let pixel: IDWriteFontCollection =
                factory5.CreateFontCollectionFromFontSet(&set)?.cast()?;
            let family = pixel.GetFontFamily(0)?;
            let names = family.GetFamilyNames()?;
            let length = names.GetStringLength(0)?;
            let mut name = vec![0_u16; length as usize + 1];
            names.GetString(0, &mut name)?;
            let pixel_family = HSTRING::from_wide(&name[..length as usize]);
            let pixel_font = family.GetFirstMatchingFont(
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
            )?;
            let mut pixel_metrics = DWRITE_FONT_METRICS::default();
            pixel_font.GetMetrics(&mut pixel_metrics);
            let factory: IDWriteFactory = factory5.cast()?;
            let reading_metrics = reading_metrics(&factory).unwrap_or(pixel_metrics);
            // The pixel face's outlines already lie on whole pixels; grid fitting would move some
            // of their edges at 12 px and drop squares, so it is off.
            let pixel_params: IDWriteRenderingParams = factory5
                .CreateCustomRenderingParams(
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    DWRITE_PIXEL_GEOMETRY_FLAT,
                    DWRITE_RENDERING_MODE1_ALIASED,
                    DWRITE_GRID_FIT_MODE_DISABLED,
                )?
                .cast()?;
            let reading_params = factory.CreateCustomRenderingParams(
                1.8,
                0.5,
                0.0,
                DWRITE_PIXEL_GEOMETRY_FLAT,
                DWRITE_RENDERING_MODE_GDI_CLASSIC,
            )?;
            Ok(Self {
                factory,
                pixel,
                pixel_family,
                pixel_font,
                pixel_metrics,
                reading_metrics,
                pixel_params,
                reading_params,
                formats: RefCell::default(),
                widths: RefCell::default(),
            })
        }
    }

    /// The characters of `text` the pixel face does not have; the system draws those in another
    /// face, which is not on the pixel grid.
    pub fn missing_from_pixel_face(&self, text: &str) -> Vec<char> {
        text.chars()
            .filter(|character| !character.is_whitespace())
            // SAFETY: asks the loaded font whether it maps the code point.
            .filter(|character| {
                !unsafe { self.pixel_font.HasCharacter(*character as u32) }
                    .is_ok_and(|has| has.as_bool())
            })
            .collect()
    }

    /// A text format for `face` at `size` px on lines `line` px high with the baseline `baseline`
    /// px down, wrapped or ending in an ellipsis, aligned left or right. Sizes here are the ones
    /// drawn, already multiplied by the scale.
    pub(crate) fn format(
        &self,
        face: Face,
        size: i32,
        line: i32,
        baseline: i32,
        wrap: bool,
        right: bool,
    ) -> Result<IDWriteTextFormat> {
        let key = FormatKey { face, size, line, baseline, wrap, right };
        if let Some(format) = self.formats.borrow().get(&key) {
            return Ok(format.clone());
        }
        let (family, collection) = match face {
            Face::Pixel => (self.pixel_family.clone(), Some(&self.pixel)),
            Face::Reading => (HSTRING::from(READING_FAMILY), None),
        };
        // SAFETY: plain DirectWrite calls; the trimming sign is created from the format itself.
        let format = unsafe {
            let format = self.factory.CreateTextFormat(
                &family,
                collection,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size as f32,
                &HSTRING::from(LOCALE),
            )?;
            format.SetLineSpacing(
                DWRITE_LINE_SPACING_METHOD_UNIFORM,
                line as f32,
                baseline as f32,
            )?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            format.SetTextAlignment(if right {
                DWRITE_TEXT_ALIGNMENT_TRAILING
            } else {
                DWRITE_TEXT_ALIGNMENT_LEADING
            })?;
            if wrap {
                format.SetWordWrapping(DWRITE_WORD_WRAPPING_EMERGENCY_BREAK)?;
            } else {
                format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                let sign = self.factory.CreateEllipsisTrimmingSign(&format)?;
                let trimming = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                format.SetTrimming(&trimming, &sign)?;
            }
            format
        };
        self.formats.borrow_mut().insert(key, format.clone());
        Ok(format)
    }

    fn metrics(&self, face: Face) -> &DWRITE_FONT_METRICS {
        match face {
            Face::Pixel => &self.pixel_metrics,
            Face::Reading => &self.reading_metrics,
        }
    }

    fn text_metrics(
        &self,
        text: &str,
        face: Face,
        size: i32,
        wrap_at: Option<i32>,
    ) -> Result<DWRITE_TEXT_METRICS> {
        let line = size * 2;
        let format = self.format(face, size, line, size, wrap_at.is_some(), false)?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: lays out text this function owns with a format it made; GDI-classic measuring
        // gives the whole-pixel advances the text is drawn with.
        unsafe {
            let layout = self.factory.CreateGdiCompatibleTextLayout(
                &wide,
                &format,
                wrap_at.map_or(1_000_000.0, |width| width as f32),
                1_000_000.0,
                1.0,
                None,
                false,
            )?;
            layout.GetMetrics(&mut metrics)?;
        }
        Ok(metrics)
    }
}

/// The reading face's metrics, from the system's fonts.
fn reading_metrics(factory: &IDWriteFactory) -> Result<DWRITE_FONT_METRICS> {
    // SAFETY: plain DirectWrite calls on the system font collection.
    unsafe {
        let mut collection = None;
        factory.GetSystemFontCollection(&mut collection, false)?;
        let collection = collection.ok_or_else(windows::core::Error::empty)?;
        let mut index = 0;
        let mut exists = windows::core::BOOL::default();
        collection.FindFamilyName(&HSTRING::from(READING_FAMILY), &mut index, &mut exists)?;
        if !exists.as_bool() {
            return Err(windows::core::Error::empty());
        }
        let font = collection.GetFontFamily(index)?.GetFirstMatchingFont(
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
        )?;
        let mut metrics = DWRITE_FONT_METRICS::default();
        font.GetMetrics(&mut metrics);
        Ok(metrics)
    }
}

impl Measure for Fonts {
    fn width(&self, text: &str, face: Face, size: i32) -> i32 {
        if text.is_empty() {
            return 0;
        }
        let key = (text.to_owned(), face, size);
        if let Some(width) = self.widths.borrow().get(&key) {
            return *width;
        }
        let width = self
            .text_metrics(text, face, size, None)
            .map(|metrics| metrics.widthIncludingTrailingWhitespace.ceil() as i32)
            .unwrap_or(text.chars().count() as i32 * size);
        self.widths.borrow_mut().insert(key, width);
        width
    }

    fn baseline(&self, face: Face, size: i32, line: i32) -> i32 {
        let metrics = self.metrics(face);
        let em = f32::from(metrics.designUnitsPerEm.max(1));
        let ascent = f32::from(metrics.ascent) * size as f32 / em;
        let descent = f32::from(metrics.descent) * size as f32 / em;
        ((line as f32 - (ascent + descent)) / 2.0 + ascent).round() as i32
    }

    fn lines(&self, text: &str, face: Face, size: i32, width: i32, k: i32) -> i32 {
        if text.is_empty() {
            return 1;
        }
        self.text_metrics(text, face, size * k, Some(width * k))
            .map(|metrics| metrics.lineCount as i32)
            .unwrap_or(1)
    }
}

/// How DirectWrite measures what the renderer draws.
pub(crate) const MEASURING: windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE =
    DWRITE_MEASURING_MODE_GDI_CLASSIC;
