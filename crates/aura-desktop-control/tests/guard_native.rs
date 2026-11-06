//! The kept desktop on the real one: the user takes it over with a mouse button, the wheel,
//! Escape or the stop keys, and not by moving the pointer or another key; the hold lasts between
//! commands and says when it ends and why, the pointer glides, and the banner and the pointer's
//! marks never reach a capture. These tests synthesize both Aura input and separately marked
//! stand-ins for physical input; they run one at a time only in the owner's idle, unlocked
//! desktop window.
mod support;
use aura_desktop_control::host::{Desktop, Output};
use aura_desktop_control::native::{focus, guard, input, screen};
use aura_desktop_domain::*;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::*;

fn batch(fixture: &OwnFixture) -> HostCommand {
    HostCommand::Input {
        app: fixture.app.clone(),
        steps: vec![
            InputStep::Click {
                at: center(fixture.place("edit")),
                button: MouseButton::Left,
                count: 1,
            },
            InputStep::Wait { ms: 3_000 },
            InputStep::Type { text: "guard-fixture-only".into() },
        ],
        window: None,
        clipboard: false,
        system_keys: false,
    }
}

fn held(command: HostCommand, hold: &str) -> HostCommand {
    HostCommand::Guarded {
        command: Box::new(command),
        hold: Some(hold.into()),
        stop_keys: Some(pip::PIP_STOP_KEYS.into()),
    }
}

fn ticks(fixture: &OwnFixture) {
    println!("AURA_INPUT_TICK={}", input::last_sent_tick());
    for event in fixture.events() {
        if let Some(tail) = event.strip_prefix("test-input-tick ")
            && let Some(tick) = tail.split_whitespace().next()
        {
            println!("AURA_INPUT_TICK={tick}");
        }
    }
}

/// A desktop whose lines to Core are kept for the test to read.
fn speaking() -> (aura_desktop_control::desktop::RealDesktop, Arc<Mutex<Vec<u8>>>) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let output: Output = lines.clone();
    (desktop().with_output(output), lines)
}

fn said(lines: &Mutex<Vec<u8>>) -> Vec<HostMessage> {
    String::from_utf8_lossy(&lines.lock().expect("lines"))
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn wait_until(what: &str, within: Duration, done: impl Fn() -> bool) {
    let until = Instant::now() + within;
    while !done() {
        assert!(Instant::now() < until, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn banner_shows() -> bool {
    guard::banner_status().is_some_and(|(visible, _)| visible)
}

#[test]
#[ignore = "sends mouse and keyboard input; owner idle for 10 minutes, desktop unlocked"]
fn physical_escape_takes_the_desktop_over_and_never_reaches_the_program() {
    let mut fixture = OwnFixture::new("guard-escape");
    fixture.open_with((80, 80), &["--emit", "user-esc:1500"]);
    let mut desktop = desktop();
    let error = failed(desktop.run(batch(&fixture)));
    ticks(&fixture);
    assert_eq!(error.code, HostErrorCode::TakenOver);
    assert_eq!(error.steps_done, Some(1));
    assert_eq!(error.delivery, Delivery::Uncertain);
    assert!(error.message.contains("（按了 Esc，来自另一个程序）"), "{}", error.message);
    assert!(!fixture.events().iter().any(|e| e == "key escape" || e.starts_with("edit ")));
}

#[test]
#[ignore = "sends mouse and keyboard input; owner idle for 10 minutes, desktop unlocked"]
fn aura_escape_is_forwarded_and_does_not_take_the_desktop_over() {
    let mut fixture = OwnFixture::new("guard-aura-escape");
    fixture.open_with((80, 80), &["--emit", "aura-esc:1500"]);
    let mut desktop = desktop();
    let result = done(desktop.run(batch(&fixture)));
    ticks(&fixture);
    assert_eq!(result, HostResult::Input { steps_done: 3 });
    assert!(fixture.events().iter().any(|e| e == "key escape"));
    assert!(fixture.events().iter().any(|e| e.contains("guard-fixture-only")));
}

#[test]
#[ignore = "sends mouse and keyboard input; owner idle for 10 minutes, desktop unlocked"]
fn another_key_of_the_users_is_no_takeover() {
    let mut fixture = OwnFixture::new("guard-key");
    fixture.open_with((80, 80), &["--emit", "user-key:1500"]);
    let mut desktop = desktop();
    assert_eq!(done(desktop.run(batch(&fixture))), HostResult::Input { steps_done: 3 });
    ticks(&fixture);
    assert!(fixture.events().iter().any(|e| e.contains("guard-fixture-only")));
}

#[test]
#[ignore = "sends mouse and keyboard input; owner idle for 10 minutes, desktop unlocked"]
fn moving_the_mouse_is_no_takeover_however_far() {
    let mut desktop = desktop();
    for (name, emit) in [("guard-slip", "user-mouse:1500"), ("guard-move", "user-move-far:1500")] {
        let mut fixture = OwnFixture::new(name);
        fixture.open_with((80, 80), &["--emit", emit]);
        assert_eq!(done(desktop.run(batch(&fixture))), HostResult::Input { steps_done: 3 });
        ticks(&fixture);
        assert!(fixture.events().iter().any(|e| e.contains("guard-fixture-only")), "{name}");
    }
}

#[test]
#[ignore = "sends mouse and keyboard input; owner idle for 10 minutes, desktop unlocked"]
fn a_click_or_the_wheel_of_the_users_takes_the_desktop_over_at_once() {
    let mut desktop = desktop();
    for (name, emit, cause) in [
        ("guard-click", "user-click:1500", "（点了鼠标，来自另一个程序）"),
        ("guard-wheel", "user-wheel:1500", "（滚了滚轮，来自另一个程序）"),
    ] {
        let mut fixture = OwnFixture::new(name);
        fixture.open_with((80, 80), &["--emit", emit]);
        let started = Instant::now();
        let error = failed(desktop.run(batch(&fixture)));
        ticks(&fixture);
        assert_eq!((error.code, error.steps_done), (HostErrorCode::TakenOver, Some(1)), "{name}");
        assert!(started.elapsed() < Duration::from_millis(2_800), "not waited out: {name}");
        assert!(error.message.contains(cause), "{}", error.message);
        assert!(!fixture.events().iter().any(|e| e.contains("guard-fixture-only")), "{name}");
    }
}

#[test]
#[ignore = "sends mouse and keyboard input; owner idle for 10 minutes, desktop unlocked"]
fn the_stop_keys_take_the_desktop_over_and_never_reach_the_program() {
    let mut fixture = OwnFixture::new("guard-stop-keys");
    fixture.open_with((80, 80), &["--emit", "user-stop-keys:1500"]);
    let mut desktop = desktop();
    let error = failed(desktop.run(held(batch(&fixture), "h-stop")));
    ticks(&fixture);
    assert_eq!((error.code, error.steps_done), (HostErrorCode::TakenOver, Some(1)));
    assert!(error.message.contains("（按了停止快捷键，来自另一个程序）"), "{}", error.message);
    let events = fixture.events();
    assert!(!events.iter().any(|e| e == "key q" || e.contains("guard-fixture-only")), "{events:?}");
}

#[test]
#[ignore = "takes the foreground and synthesizes Escape; owner idle for 10 minutes"]
fn escape_meant_for_another_program_reaches_it_and_still_takes_the_desktop_over() {
    let mut fixture = OwnFixture::new("guard-background");
    let window = fixture.open_with((80, 80), &["--emit", "user-esc:1500"]);
    let mut other = OwnFixture::new("guard-other");
    let other_window = other.open((650, 100));
    assert!(focus::bring_to_front(window_handle(&other_window.window).expect("handle")));
    let mut desktop = desktop();
    let elements = read_all(&mut desktop, &fixture.app);
    let edit = at_place(&elements, fixture.place("edit")).key.clone();
    let steps = (0..40)
        .map(|i| ActStep::SetValue { element: edit.clone(), value: format!("background-{i}") })
        .collect();
    let error = failed(act(&mut desktop, &fixture.app, &window.window, steps));
    ticks(&fixture);
    assert_eq!(error.code, HostErrorCode::TakenOver);
    assert!(other.events().iter().any(|e| e == "key escape"), "not kept from the other program");
}

#[test]
#[ignore = "takes the foreground and shows the banner; owner idle for 10 minutes"]
fn a_hold_keeps_the_desktop_between_its_commands_until_core_gives_it_back() {
    let mut fixture = OwnFixture::new("guard-hold");
    fixture.open((80, 80));
    let mut desktop = desktop();
    let wait = || HostCommand::Input {
        app: fixture.app.clone(),
        steps: vec![InputStep::Wait { ms: 100 }],
        window: None,
        system_keys: false,
        clipboard: false,
    };
    for _ in 0..2 {
        done(desktop.run(held(wait(), "h1")));
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(guard::banner_status(), Some((true, true)), "kept between commands");
        let marks = guard::marker_status().expect("the pointer's marks");
        assert!(marks.0 && marks.1, "the ring shows beside the pointer and no capture has it");
    }
    let busy = failed(desktop.run(held(wait(), "h2")));
    assert_eq!((busy.code, busy.delivery), (HostErrorCode::Busy, Delivery::NotDelivered));
    assert_eq!(done(desktop.run(HostCommand::Release { hold: "h1".into() })), HostResult::Released);
    wait_until("the banner stayed after the hold", Duration::from_secs(1), || !banner_shows());
    done(desktop.run(held(wait(), "h2")));
    assert_eq!(done(desktop.run(HostCommand::Release { hold: "h2".into() })), HostResult::Released);
}

#[test]
#[ignore = "sends mouse input between commands; owner idle for 10 minutes"]
fn a_takeover_between_commands_ends_the_hold_says_so_and_refuses_its_next_command() {
    let mut fixture = OwnFixture::new("guard-between");
    fixture.open_with((80, 80), &["--emit", "user-click:2000"]);
    let (mut desktop, lines) = speaking();
    let click = HostCommand::Input {
        app: fixture.app.clone(),
        steps: vec![InputStep::Click {
            at: center(fixture.place("edit")),
            button: MouseButton::Left,
            count: 1,
        }],
        window: None,
        system_keys: false,
        clipboard: false,
    };
    done(desktop.run(held(click.clone(), "h3")));
    let ended = HostMessage::HoldEnded {
        hold: "h3".into(),
        reason: HoldEnd::TakenOver,
        cause: Some("点了鼠标，来自另一个程序".into()),
    };
    wait_until("the user's click did not end the hold", Duration::from_secs(6), || {
        said(&lines).contains(&ended)
    });
    ticks(&fixture);
    wait_until("the banner stayed", Duration::from_secs(1), || !banner_shows());
    let refused = failed(desktop.run(held(click, "h3")));
    assert_eq!(
        (refused.code, refused.delivery),
        (HostErrorCode::TakenOver, Delivery::NotDelivered)
    );
}

#[test]
#[ignore = "keeps the desktop for five minutes; owner idle for 10 minutes"]
fn a_hold_is_given_back_once_no_command_came_for_its_idle_time() {
    let mut fixture = OwnFixture::new("guard-idle");
    fixture.open((80, 80));
    let (mut desktop, lines) = speaking();
    let wait = HostCommand::Input {
        app: fixture.app.clone(),
        steps: vec![InputStep::Wait { ms: 50 }],
        window: None,
        system_keys: false,
        clipboard: false,
    };
    done(desktop.run(held(wait, "h4")));
    let ended = Instant::now();
    let idle = HostMessage::HoldEnded { hold: "h4".into(), reason: HoldEnd::Idle, cause: None };
    wait_until("not given back", Duration::from_millis(HOLD_IDLE_MS + 3_000), || {
        said(&lines).contains(&idle)
    });
    let waited = ended.elapsed();
    assert!(waited >= Duration::from_millis(HOLD_IDLE_MS - 100), "{waited:?}");
    assert!(!banner_shows());
}

#[test]
#[ignore = "moves the pointer and clicks; owner idle for 10 minutes"]
fn the_pointer_glides_and_its_marks_let_clicks_through_and_stay_out_of_captures() {
    let mut fixture = OwnFixture::new("guard-glide");
    fixture.open((80, 80));
    let ok = center(fixture.place("ok"));
    let far = Point { x: ok.x + 500, y: ok.y + 300 };
    input::move_to(screen::virtual_screen(), far.x, far.y);
    let mut desktop = desktop();
    let started = Instant::now();
    let clicks = HostCommand::Input {
        app: fixture.app.clone(),
        steps: vec![
            InputStep::Click { at: ok, button: MouseButton::Left, count: 1 },
            InputStep::Click { at: ok, button: MouseButton::Left, count: 1 },
        ],
        window: None,
        system_keys: false,
        clipboard: false,
    };
    done(desktop.run(held(clicks, "h5")));
    let took = started.elapsed();
    assert!(took >= Duration::from_millis(150), "the pointer jumped: {took:?}");
    assert_eq!(input::cursor(), Some((ok.x, ok.y)));
    let events = fixture.events();
    assert!(events.iter().any(|e| e == "click ok 2"), "the ripple took a click: {events:?}");
    let marks = guard::marker_status().expect("the pointer's marks");
    assert!(marks.0 && marks.1, "{marks:?}");
    assert_eq!(
        focus::root_at(ok.x, ok.y).map(|(window, _)| window),
        fixture_window(&fixture),
        "the marks are under no point"
    );
    let shot = screen::capture(marks.2).expect("capture");
    let blue = shot.bgra.chunks_exact(4).filter(|pixel| pixel[..3] == [235, 99, 37]).count();
    assert_eq!(blue, 0, "the ring is in the capture");
    done(desktop.run(HostCommand::Release { hold: "h5".into() }));
}

fn fixture_window(fixture: &OwnFixture) -> Option<isize> {
    aura_desktop_control::desktop::windows_of(&fixture.app)
        .first()
        .and_then(|window| window_handle(&window.window))
}
