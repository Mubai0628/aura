//! Painting over what a screenshot must not show: everything that is not a window of a program
//! the user let Aura operate. Windows are judged front to back, so a window in front of an
//! allowed one covers it, and an allowed window in front of another program shows.
use aura_desktop_domain::Rect;

/// The colour unauthorized areas are painted, as blue, green, red.
pub const PAINT_BGR: [u8; 3] = [0x40, 0x40, 0x40];

/// One window of the screen: where it is and whether it may be seen.
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub rect: Rect,
    pub shown: bool,
}

/// Which pixels of `area` may be seen, row by row: a pixel shows when the frontmost window over
/// it is shown; the desktop behind every window never does.
#[must_use]
pub fn visible(area: Rect, front_to_back: &[Layer]) -> Vec<bool> {
    let (width, height) = (area.width.max(0) as usize, area.height.max(0) as usize);
    let mut seen = vec![false; width * height];
    for layer in front_to_back.iter().rev() {
        let Some(part) = intersect(area, layer.rect) else { continue };
        let (left, top) = ((part.x - area.x) as usize, (part.y - area.y) as usize);
        for row in top..top + part.height as usize {
            let start = row * width + left;
            seen[start..start + part.width as usize].fill(layer.shown);
        }
    }
    seen
}

/// Paints every pixel that may not be seen, in rows of `bytes_per_pixel` B, G, R(, A) pixels.
pub fn paint(pixels: &mut [u8], bytes_per_pixel: usize, seen: &[bool]) {
    for (pixel, shown) in pixels.chunks_exact_mut(bytes_per_pixel).zip(seen) {
        if !shown {
            pixel[..3].copy_from_slice(&PAINT_BGR);
        }
    }
}

/// The part two rectangles share.
#[must_use]
pub fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (i64::from(a.x) + i64::from(a.width)).min(i64::from(b.x) + i64::from(b.width));
    let bottom = (i64::from(a.y) + i64::from(a.height)).min(i64::from(b.y) + i64::from(b.height));
    (right > i64::from(left) && bottom > i64::from(top)).then(|| Rect {
        x: left,
        y: top,
        width: (right - i64::from(left)) as i32,
        height: (bottom - i64::from(top)) as i32,
    })
}

/// Opaque B, G, R rows from the screen's B, G, R, A rows (GDI leaves the alpha byte unset).
#[must_use]
pub fn without_alpha(bgra: &[u8]) -> Vec<u8> {
    bgra.chunks_exact(4).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect()
}
