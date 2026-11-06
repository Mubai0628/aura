//! What Aura draws over the screen while it keeps the desktop: a small ring beside the pointer,
//! a ripple where it clicks and a fading trail where it drags, as one picture of premultiplied
//! BGRA pixels for a layered window. The ring stays clear of the pointer's tip, so nothing of the
//! picture lies under the point the pointer acts on until a click is done.
use aura_desktop_domain::Point;

pub const RIPPLE_MS: u64 = 450;
pub const TRAIL_MS: u64 = 350;

/// Where the ring's middle is from the pointer's tip, at 100 %.
const RING_OFFSET: (f32, f32) = (17.0, 19.0);
const RING_RADIUS: f32 = 6.0;
const RIPPLE_FROM: f32 = 5.0;
const RIPPLE_TO: f32 = 28.0;
/// The blue line of every mark, and the white halo around it on each side that keeps it seen
/// on any background.
const LINE: f32 = 2.5;
const HALO: f32 = 2.0;
/// Trail points further apart in time belong to different drags.
const TRAIL_GAP_MS: u64 = 60;
const BLUE: [f32; 3] = [37.0, 99.0, 235.0];
const WHITE: [f32; 3] = [255.0, 255.0, 255.0];

pub struct Effects {
    ripples: Vec<(Point, u64)>,
    trail: Vec<(Point, u64)>,
}

impl Default for Effects {
    fn default() -> Self {
        Self::new()
    }
}

impl Effects {
    #[must_use]
    pub const fn new() -> Self {
        Self { ripples: Vec::new(), trail: Vec::new() }
    }

    pub fn click(&mut self, at: Point, now: u64) {
        self.forget(now);
        self.ripples.push((at, now));
    }

    /// The pointer is at `at` with a button held.
    pub fn drag(&mut self, at: Point, now: u64) {
        self.forget(now);
        self.trail.push((at, now));
    }

    /// Whether a ripple or a trail still shows at `now`.
    #[must_use]
    pub fn showing(&self, now: u64) -> bool {
        self.ripples.iter().any(|&(_, at)| now.saturating_sub(at) < RIPPLE_MS)
            || self.trail.iter().any(|&(_, at)| now.saturating_sub(at) < TRAIL_MS)
    }

    fn forget(&mut self, now: u64) {
        self.ripples.retain(|&(_, at)| now.saturating_sub(at) < RIPPLE_MS);
        self.trail.retain(|&(_, at)| now.saturating_sub(at) < TRAIL_MS);
    }
}

/// The picture and where its top left is on the screen; `pixels` row by row from the top, each
/// `0xAARRGGBB` with its colour already multiplied by its alpha.
#[derive(Debug, Eq, PartialEq)]
pub struct Frame {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

enum Shape {
    Ring { x: f32, y: f32, radius: f32 },
    Line { from: (f32, f32), to: (f32, f32) },
}

/// The picture at `now`: the ring beside `pointer` when there is one, with the effects still
/// showing; `scale` is the screen's, 1 at 100 %. `None` when nothing shows.
#[must_use]
pub fn frame(effects: &Effects, pointer: Option<Point>, scale: f32, now: u64) -> Option<Frame> {
    let mut shapes: Vec<(Shape, f32)> = Vec::new();
    if let Some(pointer) = pointer {
        shapes.push((
            Shape::Ring {
                x: pointer.x as f32 + RING_OFFSET.0 * scale,
                y: pointer.y as f32 + RING_OFFSET.1 * scale,
                radius: RING_RADIUS * scale,
            },
            1.0,
        ));
    }
    for &(at, start) in &effects.ripples {
        let age = now.saturating_sub(start);
        if age < RIPPLE_MS {
            let done = age as f32 / RIPPLE_MS as f32;
            let grown = 1.0 - (1.0 - done) * (1.0 - done);
            let radius = (RIPPLE_FROM + (RIPPLE_TO - RIPPLE_FROM) * grown) * scale;
            shapes
                .push((Shape::Ring { x: at.x as f32, y: at.y as f32, radius }, 0.9 * (1.0 - done)));
        }
    }
    for pair in effects.trail.windows(2) {
        let ((from, then), (to, at)) = (pair[0], pair[1]);
        let age = now.saturating_sub(at);
        if age < TRAIL_MS && at.saturating_sub(then) <= TRAIL_GAP_MS {
            let shown = 0.85 * (1.0 - age as f32 / TRAIL_MS as f32);
            shapes.push((
                Shape::Line {
                    from: (from.x as f32, from.y as f32),
                    to: (to.x as f32, to.y as f32),
                },
                shown,
            ));
        }
    }
    let reach = (LINE / 2.0 + HALO) * scale + 1.0;
    let bounds = shapes
        .iter()
        .map(|(shape, _)| extent(shape, reach))
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))?;
    let (left, top) = (bounds.0.floor() as i32, bounds.1.floor() as i32);
    let (width, height) = (
        (bounds.2.ceil() as i32 - left).max(1) as usize,
        (bounds.3.ceil() as i32 - top).max(1) as usize,
    );
    // The strongest line and halo each pixel gets, so that marks that meet join instead of
    // darkening where they overlap.
    let mut line = vec![0.0_f32; width * height];
    let mut halo = vec![0.0_f32; width * height];
    let (half, outer) = (LINE / 2.0 * scale, (LINE / 2.0 + HALO) * scale);
    for (shape, shown) in &shapes {
        let (x0, y0, x1, y1) = extent(shape, reach);
        let columns =
            (x0.floor() as i32 - left).max(0)..(x1.ceil() as i32 - left).min(width as i32);
        for row in (y0.floor() as i32 - top).max(0)..(y1.ceil() as i32 - top).min(height as i32) {
            for column in columns.clone() {
                let point = ((left + column) as f32 + 0.5, (top + row) as f32 + 0.5);
                let distance = match *shape {
                    Shape::Ring { x, y, radius } => {
                        ((point.0 - x).hypot(point.1 - y) - radius).abs()
                    }
                    Shape::Line { from, to } => to_segment(point, from, to),
                };
                let index = row as usize * width + column as usize;
                line[index] = line[index].max(cover(half, distance) * shown);
                halo[index] = halo[index].max(cover(outer, distance) * shown);
            }
        }
    }
    let pixels = line
        .iter()
        .zip(&halo)
        .map(|(&line, &halo)| {
            let alpha = line + halo * (1.0 - line);
            let channel = |index: usize| BLUE[index] * line + WHITE[index] * halo * (1.0 - line);
            let alpha_byte = (alpha * 255.0).round().clamp(0.0, 255.0) as u32;
            let byte = |index: usize| (channel(index).round() as u32).min(alpha_byte);
            (alpha_byte << 24) | (byte(0) << 16) | (byte(1) << 8) | byte(2)
        })
        .collect();
    Some(Frame { left, top, width: width as u32, height: height as u32, pixels })
}

/// The box a shape and `reach` around it take: left, top, right, bottom.
fn extent(shape: &Shape, reach: f32) -> (f32, f32, f32, f32) {
    match *shape {
        Shape::Ring { x, y, radius } => {
            (x - radius - reach, y - radius - reach, x + radius + reach, y + radius + reach)
        }
        Shape::Line { from, to } => (
            from.0.min(to.0) - reach,
            from.1.min(to.1) - reach,
            from.0.max(to.0) + reach,
            from.1.max(to.1) + reach,
        ),
    }
}

/// How much of a pixel a line `half` wide on each side covers, with a pixel of soft edge.
fn cover(half: f32, distance: f32) -> f32 {
    (half + 0.5 - distance).clamp(0.0, 1.0)
}

fn to_segment(point: (f32, f32), from: (f32, f32), to: (f32, f32)) -> f32 {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx * dx + dy * dy;
    let along = if length == 0.0 {
        0.0
    } else {
        (((point.0 - from.0) * dx + (point.1 - from.1) * dy) / length).clamp(0.0, 1.0)
    };
    (point.0 - from.0 - along * dx).hypot(point.1 - from.1 - along * dy)
}
