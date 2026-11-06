//! Numbered boxes drawn on a screenshot's pixels: the rules alone, on a blank canvas.
use aura_desktop_control::marks::{BOX, Canvas, TEXT, draw};
use aura_desktop_domain::{Mark, Rect};

/// Screen pixels left of and below the primary monitor's origin.
const AREA: Rect = Rect { x: -100, y: 50, width: 200, height: 120 };
const CLEAR: [u8; 3] = [0; 3];
const ELEMENT: Rect = Rect { x: -60, y: 80, width: 100, height: 60 };

fn drawn(marks: &[Mark], scale: f64) -> Vec<u8> {
    let mut bgr = vec![0; (AREA.width * AREA.height * 3) as usize];
    draw(&mut Canvas { bgr: &mut bgr, area: AREA }, marks, scale);
    bgr
}

fn pixel(bgr: &[u8], x: i32, y: i32) -> [u8; 3] {
    let at = ((y - AREA.y) as usize * AREA.width as usize + (x - AREA.x) as usize) * 3;
    [bgr[at], bgr[at + 1], bgr[at + 2]]
}

#[test]
fn a_box_outlines_its_element_and_leaves_the_inside_as_it_was() {
    let bgr = drawn(&[Mark { number: 7, rect: ELEMENT }], 1.0);
    assert_eq!(pixel(&bgr, 39, 139), BOX, "bottom right corner");
    assert_eq!(pixel(&bgr, 38, 110), BOX, "right edge, two pixels wide");
    assert_eq!(pixel(&bgr, 37, 110), CLEAR, "inside");
    assert_eq!(pixel(&bgr, 40, 110), CLEAR, "outside");
    assert_eq!(pixel(&bgr, -10, 110), CLEAR, "the middle");
}

#[test]
fn the_number_is_written_in_the_top_left_corner() {
    // A 7 is lit across its top row and down its right column, two pixels a dot.
    let bgr = drawn(&[Mark { number: 7, rect: ELEMENT }], 1.0);
    assert_eq!(pixel(&bgr, -58, 82), TEXT);
    assert_eq!(pixel(&bgr, -53, 83), TEXT);
    assert_eq!(pixel(&bgr, -58, 85), BOX, "the label's ground");
    assert_eq!(pixel(&bgr, -53, 85), TEXT);
    assert_eq!(pixel(&bgr, -51, 92), BOX, "the label is one digit wide");
    assert_eq!(pixel(&bgr, -50, 92), CLEAR);
    assert_eq!(pixel(&bgr, -55, 93), BOX, "and seven dots high");
    assert_eq!(pixel(&bgr, -55, 94), CLEAR);

    let twelve = drawn(&[Mark { number: 12, rect: ELEMENT }], 1.0);
    assert_eq!(pixel(&twelve, -50, 82), TEXT, "the 2's top row");
    assert_eq!(pixel(&twelve, -43, 93), BOX, "two digits wide");
    assert_eq!(pixel(&twelve, -42, 93), CLEAR);
}

#[test]
fn a_smaller_screenshot_gets_thicker_lines_and_what_is_off_the_area_is_cut() {
    // 2.5 screen pixels a screenshot pixel: lines six wide. The box runs off the right and the
    // bottom; another is wholly off the area, and an empty one draws nothing.
    let marks = [
        Mark { number: 3, rect: Rect { x: 20, y: 60, width: 100, height: 140 } },
        Mark { number: 9, rect: Rect { x: 500, y: 500, width: 10, height: 10 } },
        Mark { number: 1, rect: Rect { x: 0, y: 120, width: 0, height: 30 } },
    ];
    let bgr = drawn(&marks, 2.5);
    assert_eq!(pixel(&bgr, 80, 65), BOX, "the top line's sixth row");
    assert_eq!(pixel(&bgr, 80, 66), CLEAR);
    assert_eq!(pixel(&bgr, 25, 169), BOX, "the left line, to the area's bottom");
    assert_eq!(pixel(&bgr, 99, 100), CLEAR, "the right line is off the area");
    assert_eq!(pixel(&bgr, 0, 130), CLEAR, "an empty box");
}
