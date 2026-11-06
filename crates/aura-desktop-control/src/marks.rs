//! Numbered boxes drawn on a screenshot over the elements of a read, so that a model that looks
//! at the image can name an element by the number on its box. They are drawn on the screen's own
//! pixels, thick enough to stay legible once the screenshot is made smaller.
use aura_desktop_domain::{Mark, Rect};

/// Blue, green, red.
pub const BOX: [u8; 3] = [0x1e, 0x1e, 0xe6];
pub const TEXT: [u8; 3] = [0xff, 0xff, 0xff];
/// The digits, 3 pixels wide and 5 high: one row a byte, the leftmost pixel in bit 2.
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

/// The pixels of `area`, rows of blue, green, red, top to bottom.
pub struct Canvas<'a> {
    pub bgr: &'a mut [u8],
    pub area: Rect,
}

impl Canvas<'_> {
    /// Fills a screen rectangle, as far as it is inside the area.
    pub fn fill(&mut self, rect: Rect, colour: [u8; 3]) {
        let left = rect.x.max(self.area.x);
        let top = rect.y.max(self.area.y);
        let right = rect.x.saturating_add(rect.width).min(self.area.x + self.area.width);
        let bottom = rect.y.saturating_add(rect.height).min(self.area.y + self.area.height);
        for y in top..bottom {
            for x in left..right {
                let at = ((y - self.area.y) as usize * self.area.width as usize
                    + (x - self.area.x) as usize)
                    * 3;
                if let Some(pixel) = self.bgr.get_mut(at..at + 3) {
                    pixel.copy_from_slice(&colour);
                }
            }
        }
    }
}

/// Draws each mark: its box, and its number in the box's top left corner. `scale` is how many
/// screen pixels one pixel of the screenshot stands for.
pub fn draw(canvas: &mut Canvas<'_>, marks: &[Mark], scale: f64) {
    // One pixel of the screenshot, in screen pixels.
    let unit = scale.max(1.0).ceil() as i32;
    let line = 2 * unit;
    let dot = 2 * unit;
    for mark in marks {
        let Rect { x, y, width, height } = mark.rect;
        if mark.rect.is_empty() {
            continue;
        }
        canvas.fill(Rect { x, y, width, height: line }, BOX);
        canvas.fill(Rect { x, y: y + height - line, width, height: line }, BOX);
        canvas.fill(Rect { x, y, width: line, height }, BOX);
        canvas.fill(Rect { x: x + width - line, y, width: line, height }, BOX);
        label(canvas, x, y, mark.number, dot);
    }
}

/// The number on a filled box whose top left corner is at (x, y).
fn label(canvas: &mut Canvas<'_>, x: i32, y: i32, number: u32, dot: i32) {
    let digits: Vec<usize> =
        number.to_string().bytes().map(|digit| usize::from(digit - b'0')).collect();
    let width = (digits.len() as i32 * 4 + 1) * dot;
    canvas.fill(Rect { x, y, width, height: 7 * dot }, BOX);
    for (place, digit) in digits.iter().enumerate() {
        let left = x + (place as i32 * 4 + 1) * dot;
        for (row, bits) in DIGITS[*digit].iter().enumerate() {
            for column in 0..3 {
                if bits & (4 >> column) != 0 {
                    let at = Rect {
                        x: left + column * dot,
                        y: y + (row as i32 + 1) * dot,
                        width: dot,
                        height: dot,
                    };
                    canvas.fill(at, TEXT);
                }
            }
        }
    }
}
