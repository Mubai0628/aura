//! The part of the screen a screenshot of a program takes when no region is asked for: around
//! all of its windows on the screen, its menus and dialogs too, so the program fills the image
//! instead of being a small part of every monitor scaled down to the image's size.
use crate::redact::intersect;
use aura_desktop_domain::Rect;

/// Screen pixels kept around the windows.
pub const MARGIN: i32 = 16;

/// The rectangle around the parts of `windows` on `screen`, [`MARGIN`] wider on every side and
/// kept on the screen; none when no window is on it.
#[must_use]
pub fn around(windows: impl IntoIterator<Item = Rect>, screen: Rect) -> Option<Rect> {
    let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for part in windows.into_iter().filter_map(|window| intersect(window, screen)) {
        left = left.min(part.x);
        top = top.min(part.y);
        right = right.max(part.x + part.width);
        bottom = bottom.max(part.y + part.height);
    }
    if left > right {
        return None;
    }
    let grown = Rect {
        x: left.saturating_sub(MARGIN),
        y: top.saturating_sub(MARGIN),
        width: (right - left).saturating_add(2 * MARGIN),
        height: (bottom - top).saturating_add(2 * MARGIN),
    };
    intersect(grown, screen)
}
