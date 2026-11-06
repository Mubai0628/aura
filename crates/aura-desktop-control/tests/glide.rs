//! The pointer glides to where Aura acts instead of jumping there: 150 to 400 ms by distance, one
//! point every few milliseconds, slow at both ends, in a slight arc unless a button is held.
use aura_desktop_control::glide::{
    DRAG_STEP_PX, LONGEST_MS, SHORTEST_MS, STEP_MS, drag_path, duration_ms, path,
};
use aura_desktop_domain::Point;

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

fn length(a: Point, b: Point) -> f64 {
    f64::from(b.x - a.x).hypot(f64::from(b.y - a.y))
}

#[test]
fn a_glide_takes_150_to_400_ms_by_its_distance() {
    assert_eq!(duration_ms(3.0), SHORTEST_MS);
    assert_eq!(duration_ms(400.0), 250);
    assert_eq!(duration_ms(1_000.0), LONGEST_MS);
    assert_eq!(duration_ms(5_000.0), LONGEST_MS);
    let mut last = 0;
    for distance in (0..3_000).step_by(7) {
        let ms = duration_ms(f64::from(distance));
        assert!(ms >= last && (SHORTEST_MS..=LONGEST_MS).contains(&ms), "{distance}: {ms}");
        last = ms;
    }
}

#[test]
fn a_glide_has_a_point_for_each_step_and_ends_exactly_at_the_target() {
    for curved in [true, false] {
        let points = path(at(0, 0), at(400, 0), curved);
        assert_eq!(points.len() as u64, 250_u64.div_ceil(STEP_MS), "{points:?}");
        assert_eq!(points.last(), Some(&at(400, 0)));
        let odd = path(at(-37, 512), at(1_203, -77), curved);
        assert_eq!(odd.last(), Some(&at(1_203, -77)));
    }
}

#[test]
fn a_curved_glide_bows_a_little_to_one_side_and_never_goes_back_or_past_the_target() {
    let points = path(at(0, 0), at(1_000, 0), true);
    let bow = points.iter().map(|point| point.y.abs()).max().expect("points");
    assert!((10..=48).contains(&bow), "bow {bow}: {points:?}");
    assert!(
        points.iter().all(|point| point.y <= 0) || points.iter().all(|point| point.y >= 0),
        "one side: {points:?}"
    );
    assert!(points.windows(2).all(|pair| pair[1].x >= pair[0].x), "{points:?}");
    assert!(points.iter().all(|point| (0..=1_000).contains(&point.x)), "{points:?}");
    let short = path(at(0, 0), at(0, 100), true);
    let bow = short.iter().map(|point| point.x.abs()).max().expect("points");
    assert!(bow <= 6, "a short glide bows little: {short:?}");
}

#[test]
fn a_straight_glide_stays_on_the_line() {
    let points = path(at(10, 20), at(310, 20), false);
    assert!(points.iter().all(|point| point.y == 20), "{points:?}");
    assert!(points.windows(2).all(|pair| pair[1].x >= pair[0].x), "{points:?}");
}

#[test]
fn a_glide_starts_slowly_speeds_up_and_slows_down_before_the_target() {
    for curved in [true, false] {
        let start = at(0, 0);
        let points = path(start, at(900, 300), curved);
        let steps: Vec<f64> = std::iter::once(start)
            .chain(points.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
            .map(|pair| length(pair[0], pair[1]))
            .collect();
        let middle = steps[steps.len() / 2];
        assert!(steps[0] < middle / 2.0, "{steps:?}");
        assert!(steps[steps.len() - 1] < middle / 2.0, "{steps:?}");
    }
}

#[test]
fn a_target_under_the_pointer_or_next_to_it_is_reached_in_one_move() {
    assert_eq!(path(at(5, 5), at(5, 5), true), [at(5, 5)]);
    assert_eq!(path(at(0, 0), at(1, 1), true), [at(1, 1)]);
}

#[test]
fn a_held_button_goes_straight_at_an_even_pace_through_each_point_it_was_given() {
    let points = drag_path(at(0, 0), at(120, 0));
    assert_eq!(points.len(), (120.0 / DRAG_STEP_PX) as usize);
    assert!(points.iter().enumerate().all(|(n, point)| point.x == 12 * (n as i32 + 1)));
    assert!(points.iter().all(|point| point.y == 0));
    let slanted = drag_path(at(10, 10), at(-290, 410));
    assert_eq!(slanted.last(), Some(&at(-290, 410)));
    let steps: Vec<f64> = slanted.windows(2).map(|pair| length(pair[0], pair[1])).collect();
    assert!(steps.iter().all(|step| (DRAG_STEP_PX - 1.0..=DRAG_STEP_PX + 1.0).contains(step)));
    assert_eq!(drag_path(at(3, 3), at(3, 3)), [at(3, 3)]);
    assert_eq!(drag_path(at(0, 0), at(3, 0)), [at(3, 0)], "a close point in one step");
}
