//! The grey-and-white pixel theme of the desktop's style sheet, for windows drawn outside it.
//! Colours are 0xRRGGBB; lengths are pixels at 100 %, multiplied by the window's whole scale.

pub type Rgb = u32;

pub const INK: Rgb = 0x1c1c1b;
pub const MUTED: Rgb = 0x52524e;
pub const QUIET: Rgb = 0x6f6f6b;
pub const FAINT: Rgb = 0x8a8a86;
pub const LINE: Rgb = 0xe3e3e0;
pub const LINE_STRONG: Rgb = 0xcbcbc7;
pub const LINE_SOFT: Rgb = 0xededea;
pub const CONTROL_LINE: Rgb = 0xbdbdb8;
pub const PANE: Rgb = 0xf2f2f0;
pub const SURFACE_MUTED: Rgb = 0xf4f4f2;
pub const SURFACE_HOVER: Rgb = 0xf0f0ee;
pub const SURFACE_SELECTED: Rgb = 0xe8e8e5;
pub const ACCENT: Rgb = 0x2a2a28;
pub const ACCENT_HOVER: Rgb = 0x3a3a37;
pub const WARNING: Rgb = 0x8a6220;
pub const DANGER: Rgb = 0x9e3a3a;
pub const DANGER_SOFT: Rgb = 0xf6e8e8;
pub const DANGER_LINE: Rgb = 0xe5c5c5;
pub const DANGER_SHADE: Rgb = 0xf0dada;
pub const PIXEL_SHADOW: Rgb = 0xdadad6;
pub const WHITE: Rgb = 0xffffff;

/// The dithered sea under the column's art: its light dots and the deep band's darker dots.
pub const SEA: Rgb = 0xdadad6;
pub const SEA_DEEP: Rgb = 0xcbcbc7;

/// The window's hard shadow: this far right and down, the ink at this strength, never blurred.
pub const SHADOW_OFFSET: i32 = 6;
pub const SHADOW_ALPHA: f32 = 0.14;

/// The pixel face is drawn on a 12 px grid, so it is sharp only at these sizes.
pub const PIXEL_SIZES: [i32; 2] = [12, 24];
/// Names and paths are in the reading face, at these sizes.
pub const READING_SIZES: [i32; 2] = [12, 13];
/// A pixel icon is one of these sizes, square.
pub const ICON_SIZES: [i32; 4] = [12, 16, 32, 64];
