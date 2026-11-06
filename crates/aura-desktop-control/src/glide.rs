//! The pointer glides to where Aura acts instead of jumping there, so the user sees where it
//! goes: one point every [`STEP_MS`], slow at both ends, over [`SHORTEST_MS`] to [`LONGEST_MS`]
//! by distance, in a slight arc as a hand moves. A held button goes straight at an even pace
//! instead, so that a drag passes every point it was given and keeps moving between them.
use aura_desktop_domain::Point;

pub const STEP_MS: u64 = 8;
pub const SHORTEST_MS: u64 = 150;
pub const LONGEST_MS: u64 = 400;
/// The pointer rests on its point before a button goes down, as a hand does, so that what the
/// program shows on hover comes first.
pub const HOVER_MS: u64 = 80;
/// How long a button stays down in a click.
pub const PRESS_MS: u64 = 60;
/// Between the clicks of a double or triple click, after the point is checked again; with
/// [`PRESS_MS`] well within the shortest double-click time Windows offers (200 ms).
pub const BETWEEN_CLICKS_MS: u64 = 50;
/// Between the notches of one turn of the wheel.
pub const NOTCH_MS: u64 = 30;
/// How far a held button moves in one step.
pub const DRAG_STEP_PX: f64 = 12.0;
/// Closer than this the pointer just moves.
const NEAR_PX: f64 = 2.0;
/// How far the arc bows out from the straight line: a share of the distance, at most
/// `BOW_MAX_PX`.
const BOW_SHARE: f64 = 0.06;
const BOW_MAX_PX: f64 = 48.0;

/// How long the pointer takes for `distance` pixels.
#[must_use]
pub fn duration_ms(distance: f64) -> u64 {
    (SHORTEST_MS as f64 + distance / 4.0).clamp(SHORTEST_MS as f64, LONGEST_MS as f64) as u64
}

/// The points the pointer passes from `from` to `to`, one each [`STEP_MS`], the last exactly
/// `to`.
#[must_use]
pub fn path(from: Point, to: Point, curved: bool) -> Vec<Point> {
    let (start_x, start_y) = (f64::from(from.x), f64::from(from.y));
    let (dx, dy) = (f64::from(to.x) - start_x, f64::from(to.y) - start_y);
    let distance = dx.hypot(dy);
    if distance < NEAR_PX {
        return vec![to];
    }
    let steps = duration_ms(distance).div_ceil(STEP_MS).max(1);
    // A quadratic curve bows out half as far as its control point stands off the line.
    let offset = if curved { 2.0 * (distance * BOW_SHARE).min(BOW_MAX_PX) } else { 0.0 };
    let control_x = start_x + dx / 2.0 - dy / distance * offset;
    let control_y = start_y + dy / 2.0 + dx / distance * offset;
    let mut points: Vec<Point> = (1..=steps)
        .map(|step| {
            let time = step as f64 / steps as f64;
            let along = time * time * (3.0 - 2.0 * time);
            let rest = 1.0 - along;
            let x = rest * rest * start_x
                + 2.0 * rest * along * control_x
                + along * along * f64::from(to.x);
            let y = rest * rest * start_y
                + 2.0 * rest * along * control_y
                + along * along * f64::from(to.y);
            Point { x: x.round() as i32, y: y.round() as i32 }
        })
        .collect();
    if let Some(last) = points.last_mut() {
        *last = to;
    }
    points
}

/// The points a held button passes from `from` to `to`: straight, [`DRAG_STEP_PX`] apart, the
/// last exactly `to`.
#[must_use]
pub fn drag_path(from: Point, to: Point) -> Vec<Point> {
    let (start_x, start_y) = (f64::from(from.x), f64::from(from.y));
    let (dx, dy) = (f64::from(to.x) - start_x, f64::from(to.y) - start_y);
    let steps = (dx.hypot(dy) / DRAG_STEP_PX).ceil().max(1.0) as u32;
    let mut points: Vec<Point> = (1..=steps)
        .map(|step| {
            let along = f64::from(step) / f64::from(steps);
            Point {
                x: (start_x + dx * along).round() as i32,
                y: (start_y + dy * along).round() as i32,
            }
        })
        .collect();
    if let Some(last) = points.last_mut() {
        *last = to;
    }
    points
}
