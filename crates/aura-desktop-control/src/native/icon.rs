//! A program's icon: the Shell's image of its item, as the Start menu and Explorer draw it.
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{DeleteObject, HBITMAP};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY,
};
use windows::core::HSTRING;

/// The icon of the item at `parsing_name` (a file's path, or `shell:AppsFolder\` and an item of
/// All apps), `size` pixels square, as a PNG. Never a thumbnail of a file's contents. The
/// calling thread must have joined COM.
pub fn icon(parsing_name: &str, size: u32) -> anyhow::Result<Vec<u8>> {
    drawn(parsing_name, size, super::image::png_of_bitmap)
}

/// The same icon as 0xAARRGGBB pixels, row by row, with the colours premultiplied by their alpha.
pub fn pixels(parsing_name: &str, size: u32) -> anyhow::Result<Vec<u32>> {
    drawn(parsing_name, size, |bitmap| super::image::premultiplied_square(bitmap, size))
}

fn drawn<T>(
    parsing_name: &str,
    size: u32,
    read: impl FnOnce(HBITMAP) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let side = i32::try_from(size)?;
    // SAFETY: COM calls on an item this function creates; the bitmap the factory hands over is
    // owned here and deleted once it is read.
    unsafe {
        let item: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(parsing_name), None)?;
        let bitmap = item.GetImage(SIZE { cx: side, cy: side }, SIIGBF_ICONONLY)?;
        let read = read(bitmap);
        let _ = DeleteObject(bitmap.into());
        read
    }
}
