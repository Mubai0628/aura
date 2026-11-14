//! The banner Aura shows while it keeps the desktop: what it says, where each part of it is drawn,
//! where on the screen it stands and how it moves out of the way of Aura's own pointer, and how its
//! pixels are finished. The rules alone, with no desktop.
use aura_desktop_control::banner::*;
use aura_desktop_domain::{Point, Rect};

fn right(rect: Rect) -> i32 {
    rect.x + rect.width
}

fn inside(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && right(inner) <= right(outer)
        && inner.y + inner.height <= outer.y + outer.height
}

#[test]
fn the_banner_names_the_program_and_how_to_take_the_desktop_over() {
    let said = words("画图", Some("Ctrl+Alt+Q"));
    assert_eq!(said.lead, "Aura 正在操作");
    assert_eq!(said.name, "画图");
    assert_eq!(said.hint, "点鼠标、按 Esc 或 Ctrl+Alt+Q 即可接管");
    assert_eq!(said.stop, "停止");
    assert_eq!(words("记事本", None).hint, "点鼠标、按 Esc 即可接管");
    let long = words(&"很长的程序名".repeat(10), None).name;
    assert_eq!(long.chars().count(), NAME_CHARS);
    assert!(long.ends_with('…'), "{long}");
}

#[test]
fn its_parts_stand_left_to_right_in_one_line_with_the_program_as_a_chip() {
    let measured = Measured { lead: 90, name: 40, hint: 160, stop: 26, text: 17 };
    let at = layout(measured, 1.0);
    assert_eq!((at.height, at.border, at.shadow), (40, 1, 3));
    let parts = [at.mark, at.lead, at.chip, at.hint, at.button];
    for pair in parts.windows(2) {
        assert!(right(pair[0]) < pair[1].x, "{pair:?}");
    }
    assert_eq!(at.mark.x, 12);
    assert_eq!(right(at.button) + 12, at.width);
    for part in parts {
        assert!((part.y * 2 + part.height - at.height).abs() <= 1, "centered: {part:?}");
    }
    assert!(inside(at.chip, at.icon) && inside(at.chip, at.name), "{at:?}");
    assert!(right(at.icon) < at.name.x);
    assert_eq!((at.icon.width, at.icon.height), (16, 16));
    assert!(inside(at.button, at.stop));
    assert_eq!((at.lead.width, at.name.width, at.hint.width), (90, 40, 160));
}

#[test]
fn its_parts_grow_with_the_screens_scale() {
    let at = layout(Measured { lead: 135, name: 60, hint: 240, stop: 39, text: 26 }, 1.5);
    assert_eq!((at.height, at.border, at.shadow), (60, 2, 5));
    assert_eq!((at.icon.width, at.icon.height), (24, 24));
    assert_eq!(at.mark.x, 18);
    assert_eq!(right(at.button) + 18, at.width);
}

#[test]
fn it_stands_at_the_top_middle_of_the_screen_or_at_the_bottom() {
    let area = Rect { x: 0, y: 0, width: 1920, height: 1040 };
    assert_eq!(
        place(area, (523, 43), Spot::Top, 1.0),
        Rect { x: 698, y: 8, width: 523, height: 43 }
    );
    assert_eq!(
        place(area, (523, 43), Spot::Bottom, 1.0),
        Rect { x: 698, y: 1040 - 8 - 43, width: 523, height: 43 }
    );
    let second = Rect { x: -2560, y: 100, width: 2560, height: 1400 };
    assert_eq!(
        place(second, (785, 65), Spot::Top, 1.5),
        Rect { x: -2560 + 887, y: 112, width: 785, height: 65 }
    );
}

#[test]
fn it_moves_out_of_the_way_of_auras_own_pointer_and_hides_when_both_places_are_in_it() {
    let top = Rect { x: 700, y: 8, width: 520, height: 43 };
    let bottom = Rect { x: 700, y: 990, width: 520, height: 43 };
    let at = |x, y| Point { x, y };
    assert_eq!(spot_for(&[at(100, 100)], Spot::Top, top, bottom, 1.0), Spot::Top);
    assert_eq!(spot_for(&[at(800, 20)], Spot::Top, top, bottom, 1.0), Spot::Bottom);
    assert_eq!(spot_for(&[at(695, 20)], Spot::Top, top, bottom, 1.0), Spot::Bottom, "beside it");
    assert_eq!(spot_for(&[at(800, 20)], Spot::Bottom, top, bottom, 1.0), Spot::Bottom);
    assert_eq!(spot_for(&[at(800, 1000)], Spot::Bottom, top, bottom, 1.0), Spot::Top);
    assert_eq!(spot_for(&[at(800, 20), at(800, 1000)], Spot::Top, top, bottom, 1.0), Spot::Hidden);
    assert_eq!(spot_for(&[], Spot::Top, top, bottom, 1.0), Spot::Top);
}

#[test]
fn its_pixels_are_opaque_on_the_banner_and_its_shadow_and_clear_elsewhere() {
    let mut pixels = vec![0x00ff_ffff_u32; 12];
    finish(&mut pixels, 4, &[Rect { x: 1, y: 0, width: 2, height: 2 }]);
    assert_eq!(pixels[1], 0xffff_ffff);
    assert_eq!(pixels[6], 0xffff_ffff);
    for clear in [0, 3, 4, 7, 8, 9, 10, 11] {
        assert_eq!(pixels[clear], 0, "{clear}");
    }
}

#[test]
fn the_programs_icon_is_drawn_over_the_chip_by_its_own_transparency() {
    let mut pixels = vec![0xffef_efef_u32; 9];
    let icon = [0x0000_0000, 0x8000_0000, 0xff12_3456, 0x8080_0000];
    blend(&mut pixels, 3, (1, 1), &icon, 2);
    assert_eq!(pixels[4], 0xffef_efef, "a clear pixel leaves the chip");
    assert_eq!(pixels[5], 0xff77_7777, "half black");
    assert_eq!(pixels[7], 0xff12_3456, "an opaque pixel covers it");
    assert_eq!(pixels[8], 0xfff7_7777, "half red, premultiplied");
    assert_eq!(pixels[0], 0xffef_efef, "outside the icon");
}

#[test]
fn the_banner_goes_back_on_top_at_once_but_not_in_a_race_with_a_window_that_does_the_same() {
    assert!(may_raise(&[], 5_000), "the first time");
    let nine: Vec<u64> = (0..9).map(|n| 4_100 + n * 10).collect();
    assert!(may_raise(&nine, 4_200), "fewer than RAISES_AT_ONCE in the last second");
    let ten: Vec<u64> = (0..10).map(|n| 4_100 + n * 10).collect();
    assert!(!may_raise(&ten, 4_200), "the eleventh time within a second waits");
    assert!(may_raise(&ten, 4_190 + RAISE_AGAIN_MS), "then once every RAISE_AGAIN_MS");
    assert!(may_raise(&ten, 5_100), "a second after the first of them, at once again");
}
