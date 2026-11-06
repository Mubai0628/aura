//! Input steps checked before anything is sent: the rules alone, with no desktop.
use aura_desktop_control::plan::{Allowed, MAX_REPEAT, Planned, plan};
use aura_desktop_domain::*;

const AT: Point = Point { x: 10, y: 20 };

fn press(keys: &str, repeat: u8) -> InputStep {
    InputStep::Press { keys: keys.into(), repeat }
}

fn refused(steps: &[InputStep], allowed: Allowed) -> HostError {
    let error = plan(steps, allowed).expect_err("refused");
    assert_eq!(error.delivery, Delivery::NotDelivered, "{error:?}");
    assert_eq!(error.steps_done, None, "{error:?}");
    error
}

#[test]
fn steps_within_the_limits_are_planned_in_order_with_their_chords_read() {
    let steps = [
        InputStep::Click { at: AT, button: MouseButton::Left, count: 3 },
        press("Control+s", 2),
        InputStep::Drag { path: vec![AT, Point { x: 30, y: 40 }], button: MouseButton::Right },
        InputStep::Type { text: "你好".into() },
        InputStep::Wait { ms: 10_000 },
        press("Escape", 0),
        press("Tab", 200),
    ];
    let planned = plan(&steps, Allowed::default()).expect("planned");
    assert_eq!(planned.len(), steps.len());
    assert_eq!(planned[0], Planned::Step(&steps[0]));
    assert_eq!(planned[1], Planned::Chord(parse_chord("Control+s").expect("chord"), 2));
    assert_eq!(planned[2], Planned::Step(&steps[2]));
    assert_eq!(planned[4], Planned::Step(&steps[4]));
    assert!(matches!(planned[5], Planned::Chord(_, 1)), "a repeat of 0 is once: {:?}", planned[5]);
    assert!(matches!(planned[6], Planned::Chord(_, MAX_REPEAT)), "{:?}", planned[6]);
}

#[test]
fn chords_the_system_answers_are_refused_unless_the_user_allowed_them() {
    for keys in ["Win+r", "Alt+Tab", "Control+Shift+Escape", "PrintScreen"] {
        let steps =
            [InputStep::Click { at: AT, button: MouseButton::Left, count: 1 }, press(keys, 1)];
        assert_eq!(refused(&steps, Allowed::default()).code, HostErrorCode::Refused, "{keys}");
        let allowed = Allowed { system_keys: true, clipboard: false };
        assert!(plan(&steps, allowed).is_ok(), "{keys} once allowed");
    }
}

#[test]
fn steps_that_cannot_be_carried_out_are_refused_whatever_comes_before_them() {
    let fine = InputStep::Click { at: AT, button: MouseButton::Left, count: 1 };
    let wrong = [
        press("Control+NoSuchKey", 1),
        press("a+Control", 1),
        press("", 1),
        InputStep::Drag { path: vec![AT], button: MouseButton::Left },
        InputStep::Drag { path: vec![], button: MouseButton::Left },
        InputStep::Type { text: String::new() },
        InputStep::Wait { ms: 10_001 },
        InputStep::Click { at: AT, button: MouseButton::Left, count: 0 },
        InputStep::Click { at: AT, button: MouseButton::Left, count: 4 },
    ];
    for step in wrong {
        let error =
            refused(&[fine.clone(), step.clone()], Allowed { system_keys: true, clipboard: true });
        assert_eq!(error.code, HostErrorCode::InvalidRequest, "{step:?}: {error:?}");
    }
    assert_eq!(refused(&[], Allowed::default()).code, HostErrorCode::InvalidRequest);
}
