//! Which pixels of a screenshot show, from the windows over them front to back, and painting the
//! rest.
use aura_desktop_control::redact::{Layer, PAINT_BGR, intersect, paint, visible, without_alpha};
use aura_desktop_domain::Rect;

fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect { x, y, width, height }
}

/// The rows as `#` (shows) and `.` (painted).
fn picture(area: Rect, seen: &[bool]) -> Vec<String> {
    seen.chunks(area.width as usize)
        .map(|row| row.iter().map(|shown| if *shown { '#' } else { '.' }).collect())
        .collect()
}

#[test]
fn the_frontmost_window_over_a_pixel_decides_whether_it_shows() {
    let area = rect(10, 20, 6, 4);
    let layers = [
        Layer { rect: rect(12, 21, 2, 2), shown: false },
        Layer { rect: rect(11, 20, 4, 3), shown: true },
        Layer { rect: rect(0, 0, 100, 100), shown: false },
    ];
    assert_eq!(picture(area, &visible(area, &layers)), [".####.", ".#..#.", ".#..#.", "......"]);
}

#[test]
fn an_allowed_window_in_front_of_another_shows_over_it_and_the_desktop_never_shows() {
    let area = rect(-3, -1, 5, 3);
    let layers = [
        Layer { rect: rect(-2, -1, 2, 2), shown: true },
        Layer { rect: rect(-3, -1, 5, 3), shown: false },
    ];
    assert_eq!(picture(area, &visible(area, &layers)), [".##..", ".##..", "....."]);
    assert_eq!(picture(area, &visible(area, &[])), [".....", ".....", "....."]);
}

#[test]
fn windows_reaching_past_the_area_are_cut_to_it() {
    let area = rect(0, 0, 4, 2);
    let layers = [
        Layer { rect: rect(2, -50, 1_000, 51), shown: true },
        Layer { rect: rect(-9, 1, 3_000_000, 9), shown: true },
    ];
    assert_eq!(picture(area, &visible(area, &layers)), ["..##", "####"]);
}

#[test]
fn painting_covers_only_the_pixels_that_do_not_show() {
    let mut pixels = vec![1, 2, 3, 4, 5, 6, 7, 8, 9];
    paint(&mut pixels, 3, &[true, false, true]);
    assert_eq!(pixels, [1, 2, 3, PAINT_BGR[0], PAINT_BGR[1], PAINT_BGR[2], 7, 8, 9]);
    let mut with_alpha = vec![1, 2, 3, 255, 5, 6, 7, 255];
    paint(&mut with_alpha, 4, &[false, true]);
    assert_eq!(with_alpha, [PAINT_BGR[0], PAINT_BGR[1], PAINT_BGR[2], 255, 5, 6, 7, 255]);
    assert_eq!(without_alpha(&[1, 2, 3, 0, 4, 5, 6, 0]), [1, 2, 3, 4, 5, 6]);
}

#[test]
fn rectangles_share_only_what_both_cover() {
    assert_eq!(intersect(rect(0, 0, 10, 10), rect(5, -5, 10, 10)), Some(rect(5, 0, 5, 5)));
    assert_eq!(
        intersect(rect(0, 0, 10, 10), rect(10, 0, 5, 5)),
        None,
        "touching edges share nothing"
    );
    assert_eq!(intersect(rect(0, 0, 10, 10), rect(0, 0, 0, 10)), None);
    assert_eq!(
        intersect(rect(-2_000_000_000, 0, 2_100_000_000, 5), rect(i32::MAX - 10, 0, 10, 5)),
        None
    );
    assert_eq!(
        intersect(rect(-2560, 0, 4480, 1600), rect(1800, 1000, 520, 420)),
        Some(rect(1800, 1000, 120, 420))
    );
}
