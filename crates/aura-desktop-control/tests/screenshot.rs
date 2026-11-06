//! Screenshots of the real desktop with two of Aura's test windows on it, one of a program the
//! user allowed and one of a program they did not. Whether a point shows is checked against the
//! window Windows itself finds under the point. Nothing here takes the focus or sends input.
//! The tests take turns: a window another test opens or closes between a screenshot and the
//! checks of its pixels would change what is under a point.
mod support;

use aura_desktop_control::desktop::{Shot, shot};
use aura_desktop_control::host::Desktop;
use aura_desktop_control::native::focus;
use aura_desktop_control::native::screen::virtual_screen;
use aura_desktop_control::redact::PAINT_BGR;
use aura_desktop_domain::*;
use base64::Engine as _;
use std::sync::{Mutex, MutexGuard, PoisonError};
use support::*;

static SCREEN: Mutex<()> = Mutex::new(());

fn turn() -> MutexGuard<'static, ()> {
    SCREEN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Points of each window's client area away from its controls, one where the two windows
/// overlap, and one beside both.
fn points(allowed: &OwnFixture, other: &OwnFixture) -> Vec<(i32, i32)> {
    let a = allowed.place("client");
    let b = other.place("client");
    vec![
        (a.x + 20, a.y + a.height - 20),
        (a.x + a.width - 40, a.y + 20),
        (b.x + b.width - 20, b.y + b.height - 20),
        (b.x + 20, b.y + b.height - 20),
        (b.x + 60, a.y + a.height - 40),
        (a.x - 30, a.y + 60),
    ]
}

/// Whether Windows finds a window of the process under the point.
fn on_process(process: u32, (x, y): (i32, i32)) -> bool {
    focus::root_at(x, y).is_some_and(|(_, found)| found == process)
}

fn pixel(shot: &Shot, (x, y): (i32, i32)) -> [u8; 3] {
    shot.pixel(x, y).unwrap_or_else(|| panic!("({x}, {y}) is not in {:?}", shot.area))
}

#[test]
fn only_the_allowed_programs_windows_can_be_seen() {
    let _turn = turn();
    let _desktop = desktop();
    let mut allowed = OwnFixture::new("shot-allowed");
    let mut other = OwnFixture::new("shot-other");
    let a = allowed.open((120, 120));
    let b = other.open((420, 300));
    let points = points(&allowed, &other);
    let screen = Some(virtual_screen());

    let painted = shot(std::slice::from_ref(&allowed.app), true, screen).expect("screenshot");
    let mut shown = 0;
    for point in &points {
        if on_process(a.process_id, *point) {
            shown += 1;
            assert_ne!(pixel(&painted, *point), PAINT_BGR, "{point:?} is the allowed program's");
        } else {
            assert_eq!(
                pixel(&painted, *point),
                PAINT_BGR,
                "{point:?} is not the allowed program's"
            );
        }
    }
    assert!(shown >= 2, "the allowed window shows at its own points");
    assert!(
        points.iter().any(|point| on_process(b.process_id, *point)),
        "the other window is on screen"
    );
    assert_eq!(
        painted.windows.iter().map(|window| window_handle(&window.window)).collect::<Vec<_>>(),
        [window_handle(&a.window)]
    );

    let plain = shot(std::slice::from_ref(&allowed.app), false, screen).expect("screenshot");
    for point in points.iter().filter(|point| on_process(b.process_id, **point)) {
        assert_ne!(pixel(&plain, *point), PAINT_BGR, "nothing is painted when not asked");
    }
}

#[test]
fn a_screenshot_takes_the_area_around_the_programs_windows() {
    let _turn = turn();
    let _desktop = desktop();
    let mut allowed = OwnFixture::new("shot-area");
    let mut other = OwnFixture::new("shot-area-other");
    let a = allowed.open((140, 130));
    other.open((900, 520));
    let shot = shot(std::slice::from_ref(&allowed.app), true, None).expect("screenshot");
    let (window, area) = (a.rect, shot.area);
    // Each side is the window's, moved out by at most 16 pixels.
    let out = [
        window.x - area.x,
        window.y - area.y,
        (area.x + area.width) - (window.x + window.width),
        (area.y + area.height) - (window.y + window.height),
    ];
    assert!(
        out.iter().all(|side| (0..=16).contains(side)),
        "{area:?} is not the area around {window:?}"
    );
    assert_eq!(
        shot.windows.iter().map(|window| window_handle(&window.window)).collect::<Vec<_>>(),
        [window_handle(&a.window)]
    );
}

fn png_of(desktop: &mut impl Desktop, app: &AppEntry, region: Rect, marks: Vec<Mark>) -> String {
    let result = done(desktop.run(HostCommand::Screenshot {
        show: vec![app.clone()],
        paint: true,
        max_long_edge: 4_096,
        region: Some(region),
        marks,
    }));
    let HostResult::Screenshot { png, .. } = result else { panic!("{result:?}") };
    png
}

#[test]
fn marks_on_the_area_are_drawn_and_marks_off_it_change_nothing() {
    let _turn = turn();
    let mut desktop = desktop();
    let mut allowed = OwnFixture::new("shot-marks");
    let window = allowed.open((200, 160));
    let region = window.rect;
    // The window may still be fading in: wait until two screenshots in a row are the same.
    let mut plain = png_of(&mut desktop, &allowed.app, region, vec![]);
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let again = png_of(&mut desktop, &allowed.app, region, vec![]);
        if again == plain {
            break;
        }
        plain = again;
    }
    assert_eq!(
        png_of(&mut desktop, &allowed.app, region, vec![]),
        plain,
        "nothing moves by itself"
    );
    let inside = Rect { x: region.x + 20, y: region.y + 40, width: 80, height: 30 };
    let marked = png_of(&mut desktop, &allowed.app, region, vec![Mark { number: 5, rect: inside }]);
    assert_ne!(marked, plain, "the box is drawn");
    let off = Rect { x: region.x - 400, y: region.y, width: 80, height: 30 };
    let elsewhere = png_of(&mut desktop, &allowed.app, region, vec![Mark { number: 6, rect: off }]);
    assert_eq!(elsewhere, plain, "a box off the area draws nothing");
}

#[test]
fn a_screenshot_is_no_longer_than_asked_and_its_points_map_back_to_the_screen() {
    let _turn = turn();
    let mut desktop = desktop();
    let mut allowed = OwnFixture::new("shot-size");
    let window = allowed.open((160, 140));
    let region = window.rect;
    let result = done(desktop.run(HostCommand::Screenshot {
        show: vec![allowed.app.clone()],
        paint: true,
        max_long_edge: (region.width.max(region.height) / 2) as u32,
        region: Some(region),
        marks: vec![],
    }));
    let HostResult::Screenshot { png, map, monitors, windows } = result else {
        panic!("{result:?}")
    };
    let bytes = base64::engine::general_purpose::STANDARD.decode(png).expect("base64");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let side =
        |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    assert_eq!((side(16), side(20)), (map.width, map.height), "the image is the size the map says");
    assert_eq!(map.width.max(map.height), (region.width.max(region.height) / 2) as u32);
    let (across, down) = map.scale();
    assert!((across - 2.0).abs() < 0.01 && (down - 2.0).abs() < 0.01, "{map:?}");
    assert_eq!(map.screen_rect(), region);
    assert!(!monitors.is_empty() && monitors.iter().any(|monitor| monitor.primary));
    assert_eq!(windows.len(), 1, "{windows:?}");

    let (right, bottom) = (map.width - 1, map.height - 1);
    let corner = map.to_screen(f64::from(right), f64::from(bottom)).expect("in the image");
    assert!(region.contains(corner.0, corner.1), "{corner:?} in {region:?}");
    assert!(region.contains(region.x + region.width - 1, region.y + region.height - 1));
    assert_eq!(map.to_screen(f64::from(map.width), 0.0), None);
    assert_eq!(map.to_image(region.x, region.y), Some((0, 0)));
    assert_eq!(map.to_image(region.x + region.width, region.y), None);
}

#[test]
fn a_region_that_is_not_on_the_screen_is_refused() {
    let _turn = turn();
    let _desktop = desktop();
    let region = Rect { x: -1_000_000, y: -1_000_000, width: 50, height: 50 };
    let Err(error) = shot(&[], true, Some(region)) else { panic!("a screenshot of nothing") };
    assert_eq!(error.code, HostErrorCode::InvalidRequest, "{error:?}");
    assert_eq!(error.delivery, Delivery::NotDelivered);
}

#[test]
fn with_no_program_allowed_everything_is_painted() {
    let _turn = turn();
    let _desktop = desktop();
    let mut other = OwnFixture::new("shot-none");
    let window = other.open((200, 160));
    let painted = shot(&[], true, Some(window.rect)).expect("screenshot");
    assert!(painted.bgr.chunks_exact(3).all(|pixel| pixel == PAINT_BGR));
    assert!(painted.windows.is_empty());
}
