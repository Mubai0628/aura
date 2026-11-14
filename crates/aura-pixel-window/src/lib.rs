//! Aura's own windows for the setup program and the launcher, in the desktop's grey-and-white
//! pixel theme: layered Win32 windows drawn with Direct2D and DirectWrite at a whole scale, the
//! pixel face carried in the program, keyboard operation and UI Automation for screen readers.
//! Pages are plain data ([`page`]); [`PixelWindow`] shows them and says what the user chose.
mod a11y;
pub mod art;
pub mod audit;
mod folder;
pub mod format;
mod image;
pub mod layout;
pub mod page;
mod progress;
mod render;
pub mod scale;
pub mod scene;
pub mod testing;
mod text;
pub mod theme;
mod window;

pub use folder::choose_folder;
pub use image::Image;
pub use layout::{Laid, Look, lay_out};
pub use page::*;
pub use progress::{Progress, SHOW_AFTER, STOP, Snapshot, with_progress_window};
pub use render::Painter;
pub use text::{Fonts, fonts};
pub use window::PixelWindow;

use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};

/// Draws `page` at whole scale `k` without a window, as the window would show it at rest.
pub fn draw(page: &Page, k: i32) -> windows::core::Result<(Laid, Image)> {
    // SAFETY: COM for this thread, as the window's thread has it.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let fonts = fonts()?;
    let laid = lay_out(page, &Look::default(), &*fonts, k);
    let image = Painter::new()?.paint(&laid.scene, k, &fonts)?;
    Ok((laid, image))
}

#[cfg(test)]
mod tests;
