//! How screenshot points become screen points, how small a screenshot is made, and how the
//! tools' key names are read.
use aura_desktop_domain::*;

#[test]
fn a_screenshot_point_is_the_middle_of_the_screen_pixels_it_shows() {
    let same = ScreenMap {
        left: -2560,
        top: 0,
        width: 2560,
        height: 1600,
        screen_width: 2560,
        screen_height: 1600,
    };
    assert_eq!(same.scale(), (1.0, 1.0));
    assert_eq!(same.to_screen(0.0, 0.0), Some((-2560, 0)));
    assert_eq!(same.to_screen(2559.0, 1599.0), Some((-1, 1599)));
    assert_eq!(same.to_screen(2560.0, 0.0), None);
    assert_eq!(same.to_screen(-0.5, 3.0), None);
    assert_eq!(same.to_screen(f64::NAN, 3.0), None);
    assert_eq!(same.to_image(-2560, 0), Some((0, 0)));
    assert_eq!(same.to_image(-1, 1599), Some((2559, 1599)));
    assert_eq!(same.to_image(0, 0), None);

    let half = ScreenMap {
        left: 100,
        top: 50,
        width: 400,
        height: 300,
        screen_width: 800,
        screen_height: 600,
    };
    assert_eq!(half.to_screen(0.0, 0.0), Some((101, 51)), "the middle of the 2 × 2 pixels");
    assert_eq!(half.to_screen(399.0, 299.0), Some((899, 649)));
    assert_eq!(half.to_screen(10.5, 0.0), Some((122, 51)));
    assert_eq!(half.to_image(101, 51), Some((0, 0)));
    assert_eq!(half.to_image(899, 649), Some((399, 299)));
    assert_eq!(half.to_image(900, 649), None);
    assert_eq!(half.to_image(99, 649), None);
    assert_eq!(half.screen_rect(), Rect { x: 100, y: 50, width: 800, height: 600 });
    for (x, y) in [(0.0, 0.0), (123.0, 45.0), (399.0, 299.0)] {
        let (screen_x, screen_y) = half.to_screen(x, y).expect("on the image");
        assert_eq!(
            half.to_image(screen_x, screen_y),
            Some((x as u32, y as u32)),
            "({x}, {y}) maps back"
        );
    }
}

#[test]
fn a_side_rounded_to_whole_pixels_still_maps_inside_the_area() {
    // 506 × 413 made half as long: 253 × 207, so the rows are 413 / 207 screen pixels each.
    let (width, height) = fitted(506, 413, 253);
    assert_eq!((width, height), (253, 207));
    let map =
        ScreenMap { left: 167, top: 140, width, height, screen_width: 506, screen_height: 413 };
    let area = map.screen_rect();
    assert_eq!(area, Rect { x: 167, y: 140, width: 506, height: 413 });
    let corner = map.to_screen(f64::from(width - 1), f64::from(height - 1)).expect("in the image");
    assert!(area.contains(corner.0, corner.1), "{corner:?} in {area:?}");
    assert_eq!(
        map.to_image(area.x + area.width - 1, area.y + area.height - 1),
        Some((width - 1, height - 1))
    );
    for y in 0..height {
        let (_, screen_y) = map.to_screen(0.0, f64::from(y)).expect("in the image");
        assert!(area.contains(area.x, screen_y), "row {y} is row {screen_y}");
        assert_eq!(
            map.to_image(area.x, screen_y).map(|(_, row)| row),
            Some(y),
            "row {y} maps back"
        );
    }
}

#[test]
fn a_screenshot_is_made_smaller_only_when_it_is_longer_than_asked() {
    assert_eq!(fitted(1920, 1080, 1568), (1568, 882));
    assert_eq!(fitted(1080, 1920, 960), (540, 960));
    assert_eq!(fitted(800, 600, 1568), (800, 600));
    assert_eq!(fitted(1568, 10, 1568), (1568, 10));
    assert_eq!(fitted(4480, 1, 448), (448, 1), "never thinner than a pixel");
    assert_eq!(fitted(640, 480, 0), (640, 480), "no limit");
}

fn chord(keys: &str) -> Chord {
    parse_chord(keys).unwrap_or_else(|error| panic!("{keys}: {error}"))
}

#[test]
fn chords_are_read_modifiers_first_with_the_extended_keys_marked() {
    let save = chord("Control+Shift+s");
    assert_eq!(save.modifiers.iter().map(|key| key.code).collect::<Vec<_>>(), [0x11, 0x10]);
    assert_eq!(save.key, Key { code: 0x53, extended: false });
    assert_eq!(
        chord("ctrl + S"),
        Chord { modifiers: vec![Key { code: 0x11, extended: false }], key: save.key }
    );
    assert_eq!(chord("Enter").key.code, 0x0D);
    assert_eq!(chord("Alt+F4").key, Key { code: 0x73, extended: false });
    assert_eq!(chord("F24").key.code, 0x87);
    assert_eq!(chord("Shift+Home").key, Key { code: 0x24, extended: true });
    assert_eq!(chord("ArrowDown").key, Key { code: 0x28, extended: true });
    assert_eq!(chord("Delete").key, Key { code: 0x2E, extended: true });
    assert_eq!(chord("Control+/").key.code, 0xBF);
    assert_eq!(chord("7").key.code, 0x37);
    assert!(chord("Win").key.extended);

    for wrong in ["", "Control+", "+a", "a+Control", "Control+NoSuchKey", "F25", "F0", "Control++"]
    {
        assert!(parse_chord(wrong).is_err(), "{wrong:?} is refused");
    }
}

#[test]
fn chords_the_system_answers_are_told_apart_from_the_programs() {
    for system in [
        "Win",
        "Win+r",
        "Win+Shift+s",
        "Alt+Tab",
        "Alt+Shift+Tab",
        "Alt+Escape",
        "Control+Escape",
        "Control+Shift+Escape",
        "Control+Alt+Delete",
        "PrintScreen",
        "Alt+PrintScreen",
    ] {
        assert!(chord(system).is_system(), "{system} is the system's");
    }
    for program in [
        "Control+s",
        "Alt+F4",
        "Escape",
        "Tab",
        "Control+Tab",
        "Shift+Tab",
        "Delete",
        "Control+Delete",
        "Alt+Space",
        "Control+Shift+n",
    ] {
        assert!(!chord(program).is_system(), "{program} is the program's");
    }
}

/// Three monitors at different scales, in physical pixels as a per-monitor aware process sees
/// them: a 150 % primary, a 100 % one to its left and lower, and a 125 % one above it.
const MONITORS: [(Rect, u32); 3] = [
    (Rect { x: 0, y: 0, width: 3840, height: 2160 }, 144),
    (Rect { x: -1920, y: 600, width: 1920, height: 1080 }, 96),
    (Rect { x: 640, y: -1440, width: 2560, height: 1440 }, 120),
];

#[test]
fn every_monitor_of_a_mixed_scale_desktop_maps_to_and_from_a_smaller_screenshot() {
    // The virtual screen holds them all: from (-1920, -1440) to (3840, 2160).
    let screen = Rect { x: -1920, y: -1440, width: 5760, height: 3600 };
    for (monitor, _) in MONITORS {
        assert!(screen.contains(monitor.x, monitor.y), "{monitor:?}");
        assert!(screen.contains(monitor.x + monitor.width - 1, monitor.y + monitor.height - 1));
    }
    let (width, height) = fitted(5760, 3600, 1568);
    assert_eq!((width, height), (1568, 980));
    let map = ScreenMap {
        left: screen.x,
        top: screen.y,
        width,
        height,
        screen_width: 5760,
        screen_height: 3600,
    };
    for (monitor, dpi) in MONITORS {
        // A screenshot pixel on a monitor's edge also shows some of its neighbour.
        let inside = [
            (monitor.x + 8, monitor.y + 8),
            (monitor.x + monitor.width - 9, monitor.y + monitor.height - 9),
            (monitor.x + monitor.width / 2, monitor.y + monitor.height / 2),
        ];
        for (x, y) in inside {
            let (image_x, image_y) = map.to_image(x, y).expect("on the screenshot");
            let (back_x, back_y) =
                map.to_screen(f64::from(image_x), f64::from(image_y)).expect("on the screen");
            assert!(
                monitor.contains(back_x, back_y),
                "{dpi} dpi: ({x}, {y}) → ({image_x}, {image_y}) → ({back_x}, {back_y})"
            );
            assert!(
                (back_x - x).abs() <= 4 && (back_y - y).abs() <= 4,
                "within a screenshot pixel"
            );
        }
    }
    // The gap below the monitor above, left of the primary, is on the screenshot but on no
    // monitor: a point there is on no window either.
    let (gap_x, gap_y) = map.to_screen(10.0, 10.0).expect("on the screenshot");
    assert!(MONITORS.iter().all(|(monitor, _)| !monitor.contains(gap_x, gap_y)));
}

#[test]
fn a_part_of_one_monitor_maps_at_that_monitors_own_pixels() {
    // A region of the 100 % monitor, taken at full detail: one screenshot pixel a screen pixel.
    let map = ScreenMap {
        left: -1500,
        top: 900,
        width: 600,
        height: 400,
        screen_width: 600,
        screen_height: 400,
    };
    assert_eq!(map.to_screen(0.0, 0.0), Some((-1500, 900)));
    assert_eq!(map.to_screen(599.0, 399.0), Some((-901, 1299)));
    assert_eq!(map.to_image(-901, 1299), Some((599, 399)));
    assert!(MONITORS[1].0.contains(-901, 1299));
}
