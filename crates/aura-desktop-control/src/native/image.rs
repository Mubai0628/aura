//! Encoding a screenshot or an icon as PNG, made smaller first when asked, with the Windows
//! Imaging Component.
use windows::Win32::Graphics::Gdi::{HBITMAP, HPALETTE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat24bppBGR,
    GUID_WICPixelFormat32bppBGRA, GUID_WICPixelFormat32bppPBGRA, IWICBitmapFrameEncode,
    IWICBitmapSource, IWICImagingFactory, IWICPalette, WICBitmapDitherTypeNone,
    WICBitmapEncoderNoCache, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
    WICBitmapUsePremultipliedAlpha,
};
use windows::Win32::System::Com::StructuredStorage::IPropertyBag2;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::{GUID, Interface};

/// A PNG of `width` × `height` rows of B, G, R pixels, scaled to `out_width` × `out_height`.
/// The calling thread must have joined COM.
pub fn png(
    width: u32,
    height: u32,
    bgr: &[u8],
    out_width: u32,
    out_height: u32,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(bgr.len() == width as usize * height as usize * 3, "截图的像素数不对");
    // SAFETY: COM calls on objects this function creates and releases; the pixel buffer outlives
    // the bitmap made from it.
    unsafe {
        let factory = factory()?;
        let bitmap = factory.CreateBitmapFromMemory(
            width,
            height,
            &GUID_WICPixelFormat24bppBGR,
            width * 3,
            bgr,
        )?;
        let source: IWICBitmapSource = if (out_width, out_height) == (width, height) {
            bitmap.cast()?
        } else {
            let scaler = factory.CreateBitmapScaler()?;
            scaler.Initialize(&bitmap, out_width, out_height, WICBitmapInterpolationModeFant)?;
            scaler.cast()?
        };
        encode(&factory, &source, out_width, out_height, GUID_WICPixelFormat24bppBGR)
    }
}

/// A PNG with transparency of a bitmap the Shell drew, whose colours are premultiplied by their
/// alpha. The bitmap stays the caller's. The calling thread must have joined COM.
pub fn png_of_bitmap(bitmap: HBITMAP) -> anyhow::Result<Vec<u8>> {
    let (width, height, mut pixels) = premultiplied(bitmap, None)?;
    crate::alpha::straighten(&mut pixels);
    png_of_bgra(width, height, &pixels)
}

/// The pixels of a bitmap the Shell drew, made `side` pixels square, as 0xAARRGGBB with the
/// colours premultiplied by their alpha, row by row. The calling thread must have joined COM.
pub fn premultiplied_square(bitmap: HBITMAP, side: u32) -> anyhow::Result<Vec<u32>> {
    let (_, _, bytes) = premultiplied(bitmap, Some(side))?;
    Ok(bytes
        .chunks_exact(4)
        .map(|pixel| u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]))
        .collect())
}

/// A PNG with transparency of `width` × `height` rows of B, G, R, A pixels whose colours are not
/// premultiplied. The calling thread must have joined COM.
pub fn png_of_bgra(width: u32, height: u32, bgra: &[u8]) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(bgra.len() == width as usize * height as usize * 4, "图像的像素数不对");
    // SAFETY: COM calls on objects this function creates and releases; the pixel buffer outlives
    // the bitmap made from it.
    unsafe {
        let factory = factory()?;
        let bitmap = factory.CreateBitmapFromMemory(
            width,
            height,
            &GUID_WICPixelFormat32bppBGRA,
            width * 4,
            bgra,
        )?;
        encode(&factory, &bitmap.cast()?, width, height, GUID_WICPixelFormat32bppBGRA)
    }
}

/// The bitmap's size and its premultiplied B, G, R, A bytes, made `side` pixels square when asked.
fn premultiplied(bitmap: HBITMAP, side: Option<u32>) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    // SAFETY: COM calls on objects this function creates and releases; WIC copies the bitmap's
    // pixels and keeps no handle.
    unsafe {
        let factory = factory()?;
        let drawn = factory.CreateBitmapFromHBITMAP(
            bitmap,
            HPALETTE::default(),
            WICBitmapUsePremultipliedAlpha,
        )?;
        let (mut width, mut height) = (0_u32, 0_u32);
        drawn.GetSize(&raw mut width, &raw mut height)?;
        let source: IWICBitmapSource = match side {
            Some(side) if (width, height) != (side, side) => {
                let scaler = factory.CreateBitmapScaler()?;
                scaler.Initialize(&drawn, side, side, WICBitmapInterpolationModeFant)?;
                (width, height) = (side, side);
                scaler.cast()?
            }
            _ => drawn.cast()?,
        };
        let converted = factory.CreateFormatConverter()?;
        converted.Initialize(
            &source,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None::<&IWICPalette>,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let stride = width * 4;
        let mut pixels = vec![0_u8; stride as usize * height as usize];
        converted.CopyPixels(std::ptr::null(), stride, &mut pixels)?;
        Ok((width, height, pixels))
    }
}

unsafe fn factory() -> anyhow::Result<IWICImagingFactory> {
    // SAFETY: an in-process COM object of the calling thread's apartment.
    Ok(unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? })
}

/// `source`, of `width` × `height` pixels in `format`, as the bytes of a PNG.
unsafe fn encode(
    factory: &IWICImagingFactory,
    source: &IWICBitmapSource,
    width: u32,
    height: u32,
    format: GUID,
) -> anyhow::Result<Vec<u8>> {
    // SAFETY: COM calls on objects this function creates and releases; the stream is read into
    // an owned Vec.
    unsafe {
        let stream = SHCreateMemStream(None).ok_or_else(|| anyhow::anyhow!("建不了内存流"))?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame: Option<IWICBitmapFrameEncode> = None;
        let mut options: Option<IPropertyBag2> = None;
        encoder.CreateNewFrame(&raw mut frame, &raw mut options)?;
        let frame = frame.ok_or_else(|| anyhow::anyhow!("PNG 编码器没有给出图像帧"))?;
        frame.Initialize(options.as_ref())?;
        frame.SetSize(width, height)?;
        let mut pixels = format;
        frame.SetPixelFormat(&raw mut pixels)?;
        frame.WriteSource(source, std::ptr::null())?;
        frame.Commit()?;
        encoder.Commit()?;
        let mut stat = STATSTG::default();
        stream.Stat(&raw mut stat, STATFLAG_NONAME)?;
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        let mut bytes = vec![0_u8; usize::try_from(stat.cbSize)?];
        let mut read = 0_u32;
        stream.Read(bytes.as_mut_ptr().cast(), bytes.len() as u32, Some(&raw mut read)).ok()?;
        bytes.truncate(read as usize);
        Ok(bytes)
    }
}
