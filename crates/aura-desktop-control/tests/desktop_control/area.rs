//! The area a screenshot of a program takes by default: around its windows, kept on the screen.
use aura_desktop_control::area::{MARGIN, around};
use aura_desktop_domain::Rect;

const fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect { x, y, width, height }
}

/// Two monitors: one 2560 pixels wide left of the primary one, which is 1920 wide.
const SCREEN: Rect = rect(-2560, 0, 4480, 1600);

#[test]
fn the_area_is_around_the_window_with_a_margin() {
    let window = rect(320, 120, 1235, 802);
    assert_eq!(
        around([window], SCREEN),
        Some(rect(320 - MARGIN, 120 - MARGIN, 1235 + 2 * MARGIN, 802 + 2 * MARGIN))
    );
}

#[test]
fn a_menu_beside_the_window_is_in_the_area() {
    let (window, menu) = (rect(100, 100, 400, 300), rect(450, 350, 160, 240));
    let area = around([window, menu], SCREEN).expect("on the screen");
    assert_eq!(area, rect(100 - MARGIN, 100 - MARGIN, 510 + 2 * MARGIN, 490 + 2 * MARGIN));
}

#[test]
fn the_area_keeps_to_the_screen_on_the_left_monitor_too() {
    let window = rect(-2570, -10, 600, 400);
    assert_eq!(around([window], SCREEN), Some(rect(-2560, 0, 590 + MARGIN, 390 + MARGIN)));
    let at_the_right = rect(1900, 1500, 100, 200);
    assert_eq!(
        around([at_the_right], SCREEN),
        Some(rect(1900 - MARGIN, 1500 - MARGIN, 20 + MARGIN, 100 + MARGIN))
    );
}

#[test]
fn windows_off_the_screen_or_empty_give_no_area() {
    assert_eq!(around([], SCREEN), None);
    assert_eq!(around([rect(-32_000, -32_000, 160, 28)], SCREEN), None);
    assert_eq!(around([rect(10, 10, 0, 50)], SCREEN), None);
    let window = rect(10, 10, 100, 50);
    assert_eq!(around([rect(-32_000, -32_000, 160, 28), window], SCREEN), around([window], SCREEN));
}
