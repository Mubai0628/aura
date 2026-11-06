//! Real held-button gestures, using only the unpublished fixture and its synthetic input.
mod support;
use aura_desktop_control::host::Desktop;
use aura_desktop_control::native::input;
use aura_desktop_domain::*;
use support::*;

fn run(event: &str, finishes: bool) {
    let mut fixture = OwnFixture::new(event);
    fixture.open_with((80, 80), &["--emit", &format!("{event}:100")]);
    let client = fixture.place("client");
    let start = Point { x: client.x + 20, y: client.y + client.height - 8 };
    let end = Point { x: client.x + 300, y: start.y };
    let path =
        (0..=100).map(|n| Point { x: start.x + (end.x - start.x) * n / 100, y: start.y }).collect();
    let mut desktop = desktop();
    let outcome = desktop.run(HostCommand::Input {
        app: fixture.app.clone(),
        steps: vec![
            InputStep::Wait { ms: 1 },
            InputStep::Drag { path, button: MouseButton::Left },
            InputStep::Click {
                at: center(fixture.place("ok")),
                button: MouseButton::Left,
                count: 1,
            },
        ],
        window: None,
        clipboard: false,
        system_keys: false,
    });
    println!("AURA_INPUT_TICK={}", input::last_sent_tick());
    let events = fixture.events();
    println!(
        "DRAG_EVENTS={}",
        serde_json::json!({
            "start":start,"end":end,
            "pointer":events.iter().filter(|event| event.starts_with("down ") || event.starts_with("up ")).collect::<Vec<_>>()
        })
    );
    let driver_tick = events
        .iter()
        .find_map(|event| {
            event.strip_prefix("test-input-tick ")?.split_whitespace().next()?.parse::<u32>().ok()
        })
        .expect("synthetic input happened during the gesture");
    println!("AURA_INPUT_TICK={driver_tick}");
    assert_eq!(events.iter().filter(|e| e.starts_with("down ")).count(), 1, "{events:?}");
    if finishes {
        assert_eq!(done(outcome), HostResult::Input { steps_done: 3 });
        assert_eq!(
            events.iter().filter(|e| e.starts_with("up ")).collect::<Vec<_>>(),
            [&format!("up {} {}", end.x, end.y)],
            "{events:?}"
        );
        assert!(input::last_sent_tick().wrapping_sub(driver_tick) < 2_000, "no pause any more");
    } else {
        let error = failed(outcome);
        assert_eq!(error.code, HostErrorCode::TakenOver);
        assert_eq!(error.delivery, Delivery::Uncertain);
        assert_eq!(error.steps_done, Some(1));
        assert!(!events.iter().any(|e| e == &format!("up {} {}", end.x, end.y)), "{events:?}");
        assert!(events.iter().any(|e| e.starts_with("up ")), "Aura must release: {events:?}");
        assert!(!events.iter().any(|e| e.starts_with("click ok")), "{events:?}");
    }
}

#[test]
#[ignore = "drags the pointer; owner idle for 10 minutes, unlocked desktop"]
fn a_move_of_the_mouse_during_a_drag_lets_it_finish_at_its_endpoint() {
    run("user-drag-move", true);
}

#[test]
#[ignore = "drags the pointer and presses a button; owner idle for 10 minutes"]
fn a_user_mouse_button_during_drag_takes_over_and_lets_the_button_go() {
    run("user-drag-button", false);
}

#[test]
#[ignore = "drags the pointer and presses a key; owner idle for 10 minutes"]
fn a_user_key_during_a_drag_lets_it_finish_at_its_endpoint() {
    run("user-drag-key", true);
}

#[test]
#[ignore = "drags the pointer and scrolls; owner idle for 10 minutes"]
fn a_user_wheel_during_drag_takes_over_and_lets_the_button_go() {
    run("user-drag-wheel", false);
}
