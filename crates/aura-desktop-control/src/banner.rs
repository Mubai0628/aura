//! The banner Aura shows while it keeps the desktop, independent of Windows: what it says, where
//! each of its parts is drawn, where on the screen it stands, how it steps out of the way of
//! Aura's own pointer, and how its pixels are finished. It looks like Aura's own windows: a white
//! face with a hard edge and a hard shadow, a graphite mark and button, and the program as a chip
//! with its icon and its name in the code font.
use aura_desktop_domain::{Point, Rect};

/// The most characters of a program's name the banner shows, the ellipsis included.
pub const NAME_CHARS: usize = 24;

/// The colours, 0xRRGGBB, as Aura's own windows name them.
pub mod colour {
    /// `--surface`.
    pub const FACE: u32 = 0xff_ff_ff;
    /// `--line-strong`.
    pub const EDGE: u32 = 0xcb_cb_c7;
    /// `--pixel-shadow`.
    pub const SHADOW: u32 = 0xda_da_d6;
    /// `--ink`.
    pub const INK: u32 = 0x1c_1c_1b;
    /// `--muted`.
    pub const MUTED: u32 = 0x52_52_4e;
    /// A program's block in a message: 7 % of the ink over the face.
    pub const CHIP: u32 = 0xef_ef_ef;
    /// `--accent` and `--on-accent`.
    pub const ACCENT: u32 = 0x2a_2a_28;
    pub const ON_ACCENT: u32 = 0xff_ff_ff;
}

/// What the banner says.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Words {
    pub lead: &'static str,
    /// The program's name, cut to [`NAME_CHARS`].
    pub name: String,
    /// How the user takes the desktop over.
    pub hint: String,
    pub stop: &'static str,
}

/// The banner's words for the program `name`; `stop_keys`: the stop shortcut besides Escape.
#[must_use]
pub fn words(name: &str, stop_keys: Option<&str>) -> Words {
    let name = name.trim();
    let name = if name.chars().count() > NAME_CHARS {
        let mut cut: String = name.chars().take(NAME_CHARS - 1).collect();
        cut.push('…');
        cut
    } else {
        name.to_owned()
    };
    let hint = match stop_keys.map(str::trim).filter(|keys| !keys.is_empty()) {
        Some(keys) => format!("点鼠标、按 Esc 或 {keys} 即可接管"),
        None => "点鼠标、按 Esc 即可接管".to_owned(),
    };
    Words { lead: "Aura 正在操作", name, hint, stop: "停止" }
}

/// How wide the banner's fonts draw its words, and how tall a line of them is, in pixels.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Measured {
    pub lead: i32,
    pub name: i32,
    pub hint: i32,
    pub stop: i32,
    pub text: i32,
}

/// Each part of the banner, in pixels from the top left of its face; the shadow lies `shadow`
/// pixels to the right and below.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    pub width: i32,
    pub height: i32,
    pub border: i32,
    pub shadow: i32,
    pub mark: Rect,
    pub lead: Rect,
    pub chip: Rect,
    pub icon: Rect,
    pub name: Rect,
    pub hint: Rect,
    pub button: Rect,
    pub stop: Rect,
}

/// `value` pixels at 100 %, at `scale`.
fn px(value: f32, scale: f32) -> i32 {
    (value * scale).round() as i32
}

/// How many pixels square the program's icon is drawn at `scale`.
#[must_use]
pub fn icon_side(scale: f32) -> u32 {
    px(16.0, scale.max(0.5)).unsigned_abs()
}

/// The banner's parts in one line, left to right: the mark, the lead, the program's chip, the
/// hint and the stop button, each in the middle of the line.
#[must_use]
pub fn layout(measured: Measured, scale: f32) -> Layout {
    let scale = scale.max(0.5);
    let height = px(40.0, scale);
    let (pad, gap) = (px(12.0, scale), px(8.0, scale));
    let middle = |tall: i32| (height - tall) / 2;
    let line =
        |x: i32, width: i32| Rect { x, y: middle(measured.text), width, height: measured.text };
    let side = px(8.0, scale);
    let mark = Rect { x: pad, y: middle(side), width: side, height: side };
    let lead = line(mark.x + side + gap, measured.lead);
    let (chip_pad, icon_side, icon_gap) = (px(6.0, scale), icon_side(scale) as i32, px(5.0, scale));
    let chip_x = lead.x + lead.width + gap;
    let icon =
        Rect { x: chip_x + chip_pad, y: middle(icon_side), width: icon_side, height: icon_side };
    let name = line(icon.x + icon_side + icon_gap, measured.name);
    let chip_height = px(24.0, scale);
    let chip = Rect {
        x: chip_x,
        y: middle(chip_height),
        width: name.x + name.width + chip_pad - chip_x,
        height: chip_height,
    };
    let hint = line(chip.x + chip.width + gap, measured.hint);
    let (button_pad, button_height) = (px(12.0, scale), px(26.0, scale));
    let button_x = hint.x + hint.width + px(12.0, scale);
    let button = Rect {
        x: button_x,
        y: middle(button_height),
        width: button_pad + measured.stop + button_pad,
        height: button_height,
    };
    let stop = line(button_x + button_pad, measured.stop);
    Layout {
        width: button.x + button.width + pad,
        height,
        border: px(1.0, scale).max(1),
        shadow: px(3.0, scale).max(1),
        mark,
        lead,
        chip,
        icon,
        name,
        hint,
        button,
        stop,
    }
}

/// Where on its screen the banner stands.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Spot {
    #[default]
    Top,
    Bottom,
    /// Out of sight until the command ends: Aura's pointer goes under both places.
    Hidden,
}

/// The banner's window of `size` (its shadow included) in the work area `area`: in the middle, a
/// little below its top or above its bottom.
#[must_use]
pub fn place(area: Rect, size: (i32, i32), spot: Spot, scale: f32) -> Rect {
    let margin = px(8.0, scale);
    let y = match spot {
        Spot::Bottom => area.y + area.height - margin - size.1,
        Spot::Top | Spot::Hidden => area.y + margin,
    };
    Rect { x: area.x + (area.width - size.0) / 2, y, width: size.0, height: size.1 }
}

/// Where the banner stands so that none of `points`, where Aura's pointer goes next, is under it
/// or right beside it: where it is now when it can, else in its other place, else nowhere.
#[must_use]
pub fn spot_for(points: &[Point], now: Spot, top: Rect, bottom: Rect, scale: f32) -> Spot {
    let margin = px(8.0, scale);
    let clear = |spot: Spot| {
        let rect = if spot == Spot::Top { top } else { bottom };
        let room = Rect {
            x: rect.x - margin,
            y: rect.y - margin,
            width: rect.width + 2 * margin,
            height: rect.height + 2 * margin,
        };
        !points.iter().any(|point| room.contains(point.x, point.y))
    };
    let order =
        if now == Spot::Bottom { [Spot::Bottom, Spot::Top] } else { [Spot::Top, Spot::Bottom] };
    order.into_iter().find(|spot| clear(*spot)).unwrap_or(Spot::Hidden)
}

/// How many times within a second the banner goes back on top at once when it is covered.
pub const RAISES_AT_ONCE: usize = 10;
/// After that, how long it waits between two times: a window of a program that also goes back on
/// top at once as soon as it is covered would otherwise race the banner without end.
pub const RAISE_AGAIN_MS: u64 = 250;

/// Whether the banner may go back on top at `now` (milliseconds), given when it last did (the
/// latest last).
#[must_use]
pub fn may_raise(raised: &[u64], now: u64) -> bool {
    let recent = raised.iter().filter(|at| now.saturating_sub(**at) < 1000).count();
    recent < RAISES_AT_ONCE || raised.last().is_none_or(|last| now >= last + RAISE_AGAIN_MS)
}

/// Finishes the banner's pixels (0xAARRGGBB, `width` to a row), which GDI drew without alpha:
/// opaque inside `opaque`, the face and its shadow, and clear everywhere else.
pub fn finish(pixels: &mut [u32], width: usize, opaque: &[Rect]) {
    let width = width.max(1);
    for (index, pixel) in pixels.iter_mut().enumerate() {
        let (x, y) = ((index % width) as i32, (index / width) as i32);
        if opaque.iter().any(|rect| rect.contains(x, y)) {
            *pixel |= 0xff00_0000;
        } else {
            *pixel = 0;
        }
    }
}

/// Draws `icon` (`side` pixels to a row, 0xAARRGGBB with colours premultiplied by their alpha)
/// over the opaque `pixels` (`width` to a row) with its top left at `at`; what falls outside is
/// left out.
pub fn blend(pixels: &mut [u32], width: usize, at: (i32, i32), icon: &[u32], side: usize) {
    let (width, side) = (width.max(1), side.max(1));
    let height = (pixels.len() / width) as i32;
    for (index, source) in icon.iter().enumerate() {
        let (x, y) = (at.0 + (index % side) as i32, at.1 + (index / side) as i32);
        let alpha = source >> 24;
        if alpha == 0 || x < 0 || y < 0 || x >= width as i32 || y >= height {
            continue;
        }
        let target = &mut pixels[y as usize * width + x as usize];
        let under = *target;
        let channel = |shift: u32| {
            let (over, below) = ((source >> shift) & 0xff, (under >> shift) & 0xff);
            (over + (below * (255 - alpha) + 127) / 255).min(255) << shift
        };
        *target = 0xff00_0000 | channel(16) | channel(8) | channel(0);
    }
}
