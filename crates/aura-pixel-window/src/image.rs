//! A drawn window: premultiplied 0xAARRGGBB pixels, row by row, and a PNG of it for the record.
use std::path::Path;

use windows::{
    Win32::{
        Foundation::GENERIC_WRITE,
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
            IWICImagingFactory, WICBitmapEncoderNoCache,
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    },
    core::{HSTRING, Result},
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

impl Image {
    pub fn pixel(&self, x: u32, y: u32) -> u32 {
        self.pixels[(y * self.width + x) as usize]
    }

    /// Writes the image as a PNG with straight alpha. Needs COM on this thread.
    pub fn save_png(&self, path: &Path) -> Result<()> {
        let mut bytes = Vec::with_capacity(self.pixels.len() * 4);
        for pixel in &self.pixels {
            let alpha = pixel >> 24;
            let straight = |shift: u32| -> u8 {
                let value = (pixel >> shift) & 0xff;
                (value * 255 + alpha / 2).checked_div(alpha).map_or(0, |value| value.min(255) as u8)
            };
            bytes.extend_from_slice(&[straight(0), straight(8), straight(16), alpha as u8]);
        }
        // SAFETY: plain WIC calls on objects this function owns; the pixel buffer outlives them.
        unsafe {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let stream = factory.CreateStream()?;
            stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), GENERIC_WRITE.0)?;
            let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut frame = None;
            let mut options = None;
            encoder.CreateNewFrame(&mut frame, &mut options)?;
            let frame = frame.ok_or_else(windows::core::Error::empty)?;
            frame.Initialize(options.as_ref())?;
            frame.SetSize(self.width, self.height)?;
            let mut format = GUID_WICPixelFormat32bppBGRA;
            frame.SetPixelFormat(&mut format)?;
            frame.WritePixels(self.height, self.width * 4, &bytes)?;
            frame.Commit()?;
            encoder.Commit()?;
        }
        Ok(())
    }
}
