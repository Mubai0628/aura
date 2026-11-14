//! What the renderer draws, in pixels at 100 %: filled rectangles, pixel pictures and words. The
//! renderer multiplies every length by one whole scale, so nothing lands between pixels.
use crate::{art::Art, page::Face, theme::Rgb};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }

    pub const fn right(&self) -> i32 {
        self.x + self.width
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub const fn inflate(&self, by: i32) -> Self {
        Self::new(self.x - by, self.y - by, self.width + 2 * by, self.height + 2 * by)
    }

    pub const fn offset(&self, dx: i32, dy: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    pub const fn scaled(&self, k: i32) -> Self {
        Self::new(self.x * k, self.y * k, self.width * k, self.height * k)
    }
}

#[derive(Clone, Debug)]
pub enum Item {
    Fill {
        rect: Rect,
        colour: Rgb,
        alpha: f32,
    },
    /// A picture drawn `cell` pixels to a grid cell; `icon` pictures are held to the icon sizes.
    Art {
        art: &'static Art,
        x: i32,
        y: i32,
        cell: i32,
        colour: Rgb,
        icon: bool,
    },
    /// The 16 px spinner at animation step `phase`.
    Spinner {
        x: i32,
        y: i32,
        phase: u32,
        colour: Rgb,
    },
    Text(Text),
}

/// Words on lines `line` px high whose first baseline is `baseline` px below `top`. Wider than
/// `width` they end in an ellipsis, or wrap when `wrap` is set.
#[derive(Clone, Debug)]
pub struct Text {
    pub text: String,
    pub face: Face,
    pub size: i32,
    pub colour: Rgb,
    /// What the words are drawn on, for the check that their edges hold no other colour.
    pub background: Rgb,
    pub x: i32,
    pub top: i32,
    pub width: i32,
    pub line: i32,
    pub lines: i32,
    pub baseline: i32,
    pub wrap: bool,
    pub right_align: bool,
}

impl Text {
    pub fn bounds(&self) -> Rect {
        Rect::new(self.x, self.top, self.width, self.line * self.lines)
    }
}

/// A window's picture: `width` × `height` with its border, plus its shadow to the right and below.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub width: i32,
    pub height: i32,
    pub items: Vec<Item>,
}

impl Scene {
    pub fn fill(&mut self, rect: Rect, colour: Rgb) {
        if rect.width > 0 && rect.height > 0 {
            self.items.push(Item::Fill { rect, colour, alpha: 1.0 });
        }
    }

    /// A rectangle's one-pixel edge, as four fills.
    pub fn frame(&mut self, rect: Rect, colour: Rgb) {
        self.fill(Rect::new(rect.x, rect.y, rect.width, 1), colour);
        self.fill(Rect::new(rect.x, rect.bottom() - 1, rect.width, 1), colour);
        self.fill(Rect::new(rect.x, rect.y + 1, 1, rect.height - 2), colour);
        self.fill(Rect::new(rect.right() - 1, rect.y + 1, 1, rect.height - 2), colour);
    }

    /// A ring `thickness` wide around the outside of `rect`, `gap` away from it.
    pub fn ring(&mut self, rect: Rect, gap: i32, thickness: i32, colour: Rgb) {
        let outer = rect.inflate(gap + thickness);
        self.fill(Rect::new(outer.x, outer.y, outer.width, thickness), colour);
        self.fill(Rect::new(outer.x, outer.bottom() - thickness, outer.width, thickness), colour);
        self.fill(
            Rect::new(outer.x, outer.y + thickness, thickness, outer.height - 2 * thickness),
            colour,
        );
        self.fill(
            Rect::new(
                outer.right() - thickness,
                outer.y + thickness,
                thickness,
                outer.height - 2 * thickness,
            ),
            colour,
        );
    }

    pub fn art(&mut self, art: &'static Art, x: i32, y: i32, cell: i32, colour: Rgb, icon: bool) {
        self.items.push(Item::Art { art, x, y, cell, colour, icon });
    }

    pub fn text(&mut self, text: Text) {
        if !text.text.is_empty() {
            self.items.push(Item::Text(text));
        }
    }
}
