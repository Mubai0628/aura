//! Simulated input for Aura's test window on the real desktop. The refusals send nothing and run
//! with the other tests; the tests that click and type bring the window to the front and move
//! the mouse, so they are run by hand (`--ignored`) when no one is using the computer.
mod support;

use aura_desktop_control::host::Desktop;
use aura_desktop_control::native::{focus, input as native_input, screen};
use aura_desktop_domain::*;
use std::time::Duration;
use support::*;

const SETTLE: Duration = Duration::from_secs(5);

fn input(app: &AppEntry, steps: Vec<InputStep>) -> HostCommand {
    HostCommand::Input {
        app: app.clone(),
        steps,
        window: None,
        system_keys: false,
        clipboard: false,
    }
}

fn click(at: Point) -> InputStep {
    InputStep::Click { at, button: MouseButton::Left, count: 1 }
}

fn typed(text: &str) -> InputStep {
    InputStep::Type { text: text.into() }
}

fn press(keys: &str) -> InputStep {
    InputStep::Press { keys: keys.into(), repeat: 1 }
}

fn refused_before_sending(error: &HostError, code: HostErrorCode) {
    assert_eq!(error.code, code, "{error:?}");
    assert_eq!(error.delivery, Delivery::NotDelivered, "{error:?}");
}

#[test]
fn steps_that_cannot_be_carried_out_are_refused_before_anything_is_sent() {
    let mut desktop = desktop();
    let mut fixture = OwnFixture::new("input-plan");
    let window = fixture.open((140, 140));
    let at = center(fixture.place("ok"));
    let cases = [
        (vec![], HostErrorCode::InvalidRequest),
        (vec![press("Win+r")], HostErrorCode::Refused),
        (vec![click(at), press("Alt+Tab")], HostErrorCode::Refused),
        (vec![press("Control+Shift+Escape")], HostErrorCode::Refused),
        (vec![press("PrintScreen")], HostErrorCode::Refused),
        (vec![press("Control+NoSuchKey")], HostErrorCode::InvalidRequest),
        (vec![press("a+Control")], HostErrorCode::InvalidRequest),
        (
            vec![InputStep::Drag { path: vec![at], button: MouseButton::Left }],
            HostErrorCode::InvalidRequest,
        ),
        (vec![typed("")], HostErrorCode::InvalidRequest),
        (vec![InputStep::Wait { ms: 60_000 }], HostErrorCode::InvalidRequest),
        (
            vec![InputStep::Click { at, button: MouseButton::Left, count: 4 }],
            HostErrorCode::InvalidRequest,
        ),
    ];
    for (steps, code) in cases {
        let error = failed(desktop.run(input(&fixture.app, steps.clone())));
        refused_before_sending(&error, code);
        assert_eq!(error.steps_done, None, "{steps:?}");
    }
    assert!(
        fixture.since_ready().is_empty(),
        "nothing reached the window: {:?}",
        fixture.since_ready()
    );
    assert!(!window_is_in_front(&window), "the window was not brought to the front");
}

fn window_is_in_front(window: &WindowInfo) -> bool {
    focus::foreground().is_some_and(|(front, _)| Some(front) == window_handle(&window.window))
}

#[test]
fn a_window_that_moved_or_is_not_the_programs_is_refused() {
    let mut desktop = desktop();
    let mut fixture = OwnFixture::new("input-moved");
    let mut stranger = OwnFixture::new("input-stranger");
    let window = fixture.open((180, 160));
    let theirs = stranger.open((700, 200));
    let at = center(fixture.place("ok"));
    let moved = Rect { x: window.rect.x + 10, ..window.rect };
    let checks = [
        WindowCheck { window: window.window.clone(), expected: moved },
        WindowCheck { window: theirs.window.clone(), expected: theirs.rect },
        WindowCheck { window: "w0".into(), expected: window.rect },
    ];
    for check in checks {
        let command = HostCommand::Input {
            app: fixture.app.clone(),
            steps: vec![click(at)],
            window: Some(check),
            system_keys: false,
            clipboard: false,
        };
        refused_before_sending(&failed(desktop.run(command)), HostErrorCode::Moved);
    }
    assert!(fixture.since_ready().is_empty() && stranger.since_ready().is_empty());
}

#[test]
fn a_program_with_no_window_open_gets_no_input() {
    let mut desktop = desktop();
    let fixture = OwnFixture::new("input-closed");
    let error = failed(desktop.run(input(&fixture.app, vec![typed("你好")])));
    refused_before_sending(&error, HostErrorCode::WindowNotFound);
}

#[test]
fn a_process_with_more_rights_or_unreadable_counts_as_above_us() {
    assert!(!focus::above_us(std::process::id()), "this test runs at its own level");
    assert!(focus::above_us(4), "the System process");
    assert!(focus::above_us(u32::MAX - 3), "no such process");
}

#[test]
#[ignore = "brings the test window to the front and moves the mouse: run by hand when no one is using the computer"]
fn clicks_typing_and_chords_reach_the_program_in_front() {
    let mut desktop = desktop();
    let mut fixture = OwnFixture::new("input-keys");
    let window = fixture.open((200, 180));
    let (ok, edit) = (center(fixture.place("ok")), center(fixture.place("edit")));
    let check = WindowCheck { window: window.window.clone(), expected: window.rect };
    let steps = vec![
        click(ok),
        click(edit),
        typed("你好 Aura"),
        press("Shift+Home"),
        typed("x"),
        InputStep::Press { keys: "Backspace".into(), repeat: 1 },
        typed("第二行"),
    ];
    let command = HostCommand::Input {
        app: fixture.app.clone(),
        steps,
        window: Some(check),
        system_keys: false,
        clipboard: false,
    };
    let result = done(desktop.run(command));
    assert_eq!(result, HostResult::Input { steps_done: 7 });
    assert!(
        fixture.wait_for(|events| events.iter().any(|event| event == "edit 第二行"), SETTLE),
        "{:?}",
        fixture.since_ready()
    );
    let events = fixture.since_ready();
    assert_eq!(
        events.iter().filter(|event| event.starts_with("click ok")).collect::<Vec<_>>(),
        ["click ok 1"]
    );
    assert!(events.contains(&"edit 你好 Aura".to_owned()), "{events:?}");
    assert!(
        events.contains(&"edit x".to_owned()),
        "Shift+Home selected the text that x replaced: {events:?}"
    );
    assert!(window_is_in_front(&window));
}

#[test]
#[ignore = "brings the test window to the front and moves the mouse: run by hand when no one is using the computer"]
fn the_mouse_presses_drags_and_scrolls_where_it_is_told() {
    let mut desktop = desktop();
    let mut fixture = OwnFixture::new("input-mouse");
    fixture.open((220, 200));
    let client = fixture.place("client");
    // Below the hidden button and between the two lists. A control under the pointer takes the
    // press and the wheel, and only the window itself logs them, so no point may be on one.
    let from = Point { x: client.x + 240, y: client.y + 310 };
    let to = Point { x: client.x + 340, y: client.y + 330 };
    let middle = Point { x: (from.x + to.x) / 2, y: (from.y + to.y) / 2 };
    for point in [from, middle, to] {
        assert!(client.contains(point.x, point.y), "{point:?} is outside {client:?}");
        let under = fixture.controls_at(point);
        assert!(under.is_empty(), "{point:?} is on {under:?}");
    }
    let steps = vec![
        InputStep::Drag { path: vec![from, middle, to], button: MouseButton::Left },
        InputStep::Click { at: from, button: MouseButton::Left, count: 2 },
        InputStep::Click { at: to, button: MouseButton::Right, count: 1 },
        InputStep::Scroll { at: middle, down: 2, right: 0 },
        InputStep::Scroll { at: middle, down: 0, right: -1 },
    ];
    assert_eq!(done(desktop.run(input(&fixture.app, steps))), HostResult::Input { steps_done: 5 });
    assert!(
        fixture.wait_for(|events| events.iter().any(|event| event.starts_with("hwheel")), SETTLE),
        "{:?}",
        fixture.since_ready()
    );
    let events = fixture.since_ready();
    let expected = [
        format!("down {} {}", from.x, from.y),
        format!("up {} {}", to.x, to.y),
        format!("down {} {}", from.x, from.y),
        format!("up {} {}", from.x, from.y),
        format!("down {} {}", from.x, from.y),
        format!("up {} {}", from.x, from.y),
        format!("right {} {}", to.x, to.y),
        // A notch at a time, as a hand turns the wheel.
        "wheel -120".to_owned(),
        "wheel -120".to_owned(),
        "hwheel -120".to_owned(),
    ];
    assert_eq!(events, expected);
}

#[test]
#[ignore = "moves the mouse: run by hand when no one is using the computer"]
fn the_pointer_lands_on_the_exact_pixel_of_every_monitor() {
    let _desktop = desktop();
    let screen = screen::virtual_screen();
    let mut missed = Vec::new();
    for monitor in screen::monitors() {
        let rect = monitor.rect;
        let (right, bottom) = (rect.x + rect.width - 1, rect.y + rect.height - 1);
        for x in [
            rect.x,
            rect.x + 1,
            rect.x + rect.width / 3,
            rect.x + rect.width / 2 + 1,
            right - 1,
            right,
        ] {
            for y in [rect.y, rect.y + 1, rect.y + rect.height / 2 + 1, bottom - 1, bottom] {
                assert!(native_input::move_to(screen, x, y));
                std::thread::sleep(Duration::from_millis(20));
                let landed = native_input::cursor().expect("the pointer");
                if landed != (x, y) {
                    missed.push(((x, y), landed, monitor.dpi));
                }
            }
        }
    }
    assert!(missed.is_empty(), "asked, landed, dpi: {missed:?}");
}

#[test]
#[ignore = "brings two test windows to the front and moves the mouse: run by hand when no one is using the computer"]
fn a_point_on_another_programs_window_is_not_clicked() {
    let mut desktop = desktop();
    let mut fixture = OwnFixture::new("input-target");
    let mut stranger = OwnFixture::new("input-cover");
    fixture.open((240, 220));
    stranger.open((900, 220));
    let theirs = center(stranger.place("ok"));
    let ok = center(fixture.place("ok"));
    let error = failed(desktop.run(input(&fixture.app, vec![click(ok), click(theirs)])));
    assert_eq!(error.code, HostErrorCode::Moved, "{error:?}");
    assert_eq!(error.delivery, Delivery::Uncertain, "the first click went through");
    assert_eq!(error.steps_done, Some(1));
    assert!(fixture.wait_for(|events| events.iter().any(|event| event == "click ok 1"), SETTLE));
    assert!(stranger.since_ready().is_empty(), "{:?}", stranger.since_ready());
}
