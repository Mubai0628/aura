//! What Aura draws over the screen while it keeps the desktop: a small ring beside the pointer, a
//! ripple where it clicks and a fading trail where it drags, as one picture in premultiplied
//! BGRA. The picture never covers the pointer's tip, so nothing of it is under a click.
use aura_desktop_control::pointer_marks::{Effects, Frame, RIPPLE_MS, TRAIL_MS, frame};
use aura_desktop_domain::Point;

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

fn covers(frame: &Frame, point: Point) -> bool {
    (frame.left..frame.left + frame.width as i32).contains(&point.x)
        && (frame.top..frame.top + frame.height as i32).contains(&point.y)
}

fn alpha(frame: &Frame, point: Point) -> u32 {
    if !covers(frame, point) {
        return 0;
    }
    let (x, y) = ((point.x - frame.left) as usize, (point.y - frame.top) as usize);
    frame.pixels[y * frame.width as usize + x] >> 24
}

/// How far from `center` the farthest pixel that shows is.
fn reach(frame: &Frame, center: Point) -> f64 {
    let mut farthest: f64 = 0.0;
    for row in 0..frame.height as i32 {
        for column in 0..frame.width as i32 {
            let point = at(frame.left + column, frame.top + row);
            if alpha(frame, point) > 16 {
                let distance = f64::from(point.x - center.x).hypot(f64::from(point.y - center.y));
                farthest = farthest.max(distance);
            }
        }
    }
    farthest
}

fn strongest(frame: &Frame) -> u32 {
    frame.pixels.iter().map(|pixel| pixel >> 24).max().unwrap_or(0)
}

#[test]
fn nothing_to_show_draws_nothing() {
    assert!(frame(&Effects::default(), None, 1.0, 0).is_none());
}

#[test]
fn the_ring_sits_beside_the_pointer_and_never_under_its_tip() {
    let pointer = at(500, 400);
    let shown = frame(&Effects::default(), Some(pointer), 1.0, 0).expect("the ring");
    assert!(!covers(&shown, pointer), "{:?}", (shown.left, shown.top, shown.width, shown.height));
    assert!(strongest(&shown) >= 200, "the ring shows clearly");
    assert!(shown.left > pointer.x - 4 && shown.top > pointer.y - 4, "below and to the right");
}

#[test]
fn a_click_leaves_a_ripple_that_grows_and_fades_out() {
    let mut effects = Effects::default();
    let click = at(300, 300);
    effects.click(click, 0);
    let early = frame(&effects, None, 1.0, 60).expect("a ripple");
    let late = frame(&effects, None, 1.0, 300).expect("a ripple");
    assert!(reach(&late, click) > reach(&early, click) + 5.0);
    assert!(strongest(&late) < strongest(&early));
    assert!(frame(&effects, None, 1.0, RIPPLE_MS).is_none(), "gone");
    assert!(effects.showing(RIPPLE_MS - 1) && !effects.showing(RIPPLE_MS));
}

#[test]
fn a_drag_leaves_a_trail_that_fades_behind_the_pointer() {
    let mut effects = Effects::default();
    for step in 0..=30 {
        effects.drag(at(100 + step * 10, 200), step as u64 * 10);
    }
    let shown = frame(&effects, None, 1.0, 300).expect("a trail");
    let (old, new) = (alpha(&shown, at(130, 200)), alpha(&shown, at(395, 200)));
    assert!(new > old && old > 0, "old {old}, new {new}");
    assert_eq!(alpha(&shown, at(250, 230)), 0, "only along the drag");
    assert!(frame(&effects, None, 1.0, 300 + TRAIL_MS).is_none(), "faded out");
}

#[test]
fn two_drags_a_moment_apart_are_not_joined() {
    let mut effects = Effects::default();
    effects.drag(at(100, 200), 0);
    effects.drag(at(120, 200), 8);
    effects.drag(at(300, 200), 120);
    effects.drag(at(320, 200), 128);
    let shown = frame(&effects, None, 1.0, 130).expect("two trails");
    assert!(alpha(&shown, at(110, 200)) > 0 && alpha(&shown, at(310, 200)) > 0);
    assert_eq!(alpha(&shown, at(210, 200)), 0, "nothing between the drags");
}

#[test]
fn marks_that_meet_join_without_darkening() {
    let mut effects = Effects::default();
    for (step, x) in [100, 140, 180].into_iter().enumerate() {
        effects.drag(at(x, 200), step as u64 * 8);
    }
    let shown = frame(&effects, None, 1.0, 16).expect("a trail");
    assert_eq!(alpha(&shown, at(140, 200)), alpha(&shown, at(160, 200)), "where two parts meet");
}

#[test]
fn every_pixel_is_premultiplied() {
    let mut effects = Effects::default();
    effects.click(at(40, 40), 0);
    for step in 0..10 {
        effects.drag(at(40 + step * 7, 60 + step * 3), step as u64 * 8);
    }
    let shown = frame(&effects, Some(at(120, 90)), 1.5, 100).expect("a picture");
    for pixel in &shown.pixels {
        let alpha = pixel >> 24;
        for shift in [0, 8, 16] {
            assert!((pixel >> shift) & 0xff <= alpha, "{pixel:#010x}");
        }
    }
    assert_eq!(shown.pixels.len(), (shown.width * shown.height) as usize);
}

#[test]
fn the_ring_grows_with_the_screens_scale() {
    let one = frame(&Effects::default(), Some(at(0, 0)), 1.0, 0).expect("ring");
    let two = frame(&Effects::default(), Some(at(0, 0)), 2.0, 0).expect("ring");
    let grown = f64::from(two.width) / f64::from(one.width);
    assert!((1.7..=2.1).contains(&grown), "{} {}", one.width, two.width);
}
