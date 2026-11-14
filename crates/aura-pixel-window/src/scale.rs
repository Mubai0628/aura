//! The window's whole scale. Pixel art stays sharp only at whole multiples, so 100 % and 200 %
//! are exact and 125 % and 150 % take the nearest whole multiple; a window that would not fit
//! its screen's work area at that scale is drawn one step smaller.
use crate::theme::SHADOW_OFFSET;

/// The nearest whole multiple of 96 DPI, at least 1; halves round up.
pub fn whole_scale(dpi: u32) -> i32 {
    ((dpi as f32 / 96.0).round() as i32).max(1)
}

/// `whole_scale`, made smaller until a window of `size` at 100 %, with its shadow, fits `work`.
pub fn fitting_scale(dpi: u32, size: (i32, i32), work: (i32, i32)) -> i32 {
    let mut k = whole_scale(dpi);
    while k > 1 && ((size.0 + SHADOW_OFFSET) * k > work.0 || (size.1 + SHADOW_OFFSET) * k > work.1)
    {
        k -= 1;
    }
    k
}
