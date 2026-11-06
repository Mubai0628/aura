//! Screenshots and the screen they show: monitors, and how a point of a screenshot is a point of
//! the screen. Every screen coordinate is a physical pixel of the virtual screen, whose origin
//! is the primary monitor's top left; monitors left of or above it have negative coordinates.
use crate::Rect;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Monitor {
    pub rect: Rect,
    /// Dots per inch: 96 is 100 %, 144 is 150 %.
    pub dpi: u32,
    pub primary: bool,
}

/// Where a screenshot came from: the screen area at (`left`, `top`) of `screen_width` ×
/// `screen_height` pixels, shown as an image of `width` × `height`. Each side is scaled on its
/// own, since a side made smaller is rounded to whole pixels.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenMap {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub screen_width: u32,
    pub screen_height: u32,
}

impl ScreenMap {
    /// Screen pixels per screenshot pixel across and down: 1 when it was not made smaller.
    #[must_use]
    pub fn scale(&self) -> (f64, f64) {
        (
            f64::from(self.screen_width) / f64::from(self.width.max(1)),
            f64::from(self.screen_height) / f64::from(self.height.max(1)),
        )
    }

    /// The screen point in the middle of what a screenshot point shows; `None` outside the
    /// screenshot.
    #[must_use]
    pub fn to_screen(&self, x: f64, y: f64) -> Option<(i32, i32)> {
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
            return None;
        }
        if x >= f64::from(self.width) || y >= f64::from(self.height) {
            return None;
        }
        let (across, down) = self.scale();
        let screen_x = f64::from(self.left) + (x + 0.5) * across;
        let screen_y = f64::from(self.top) + (y + 0.5) * down;
        Some((screen_x.floor() as i32, screen_y.floor() as i32))
    }

    /// The screenshot point that shows a screen point; `None` when the screenshot does not.
    #[must_use]
    pub fn to_image(&self, x: i32, y: i32) -> Option<(u32, u32)> {
        let (across, down) = self.scale();
        let image_x = (f64::from(x) - f64::from(self.left)) / across;
        let image_y = (f64::from(y) - f64::from(self.top)) / down;
        (image_x >= 0.0
            && image_y >= 0.0
            && image_x < f64::from(self.width)
            && image_y < f64::from(self.height))
        .then(|| (image_x.floor() as u32, image_y.floor() as u32))
    }

    /// The screen rectangle the screenshot shows.
    #[must_use]
    pub fn screen_rect(&self) -> Rect {
        Rect {
            x: self.left,
            y: self.top,
            width: i32::try_from(self.screen_width).unwrap_or(i32::MAX),
            height: i32::try_from(self.screen_height).unwrap_or(i32::MAX),
        }
    }
}

/// The size a screenshot of `width` × `height` screen pixels is made, so that its longer side
/// is at most `max_long_edge`; never larger than the screen, never thinner than a pixel.
#[must_use]
pub fn fitted(width: u32, height: u32, max_long_edge: u32) -> (u32, u32) {
    let long = width.max(height).max(1);
    if max_long_edge == 0 || long <= max_long_edge {
        return (width, height);
    }
    let scale = f64::from(long) / f64::from(max_long_edge);
    let fit = |side: u32| ((f64::from(side) / scale).round() as u32).clamp(1, max_long_edge);
    (fit(width), fit(height))
}
