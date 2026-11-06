//! Who has the desktop while Aura keeps it: a mouse button, the wheel, Escape or the stop keys of
//! the user's take it over; moving the pointer, another key or letting go does not.
use aura_desktop_control::guard::{Cause, Gate, Input, State, Takeover, is_chord};
use aura_desktop_domain::pip::{PIP_STOP_KEYS, stop_chord};

#[test]
fn aura_input_never_takes_the_desktop_over() {
    let mut state = State::new();
    assert!(!state.observe(Input::Aura, true));
    assert_eq!(state.gate(), Gate::Ready);
    assert_eq!(state.takeover(), None);
}

#[test]
fn moving_the_pointer_another_key_or_letting_go_is_no_takeover() {
    let mut state = State::new();
    for _ in 0..1_000 {
        assert!(!state.observe(Input::Ignored, true));
    }
    assert_eq!(state.gate(), Gate::Ready);
}

#[test]
fn a_mouse_button_or_the_wheel_takes_the_desktop_over_and_still_reaches_the_program() {
    for (input, cause) in [
        (Input::Button { injected: false }, Cause::Button),
        (Input::Wheel { injected: false }, Cause::Wheel),
    ] {
        let mut state = State::new();
        assert!(!state.observe(input, true), "{input:?} is never kept from the program");
        assert_eq!(state.gate(), Gate::TakenOver);
        assert_eq!(state.takeover(), Some(Takeover { cause, injected: false }));
    }
}

#[test]
fn a_click_or_the_wheel_on_auras_own_window_is_no_takeover() {
    use aura_desktop_control::guard::{Pointer, pointer};
    assert_eq!(pointer(Pointer::Pressed, false, true), Input::Ignored, "writing to Aura");
    assert_eq!(pointer(Pointer::Wheel, true, true), Input::Ignored, "scrolling its conversation");
    assert_eq!(pointer(Pointer::Pressed, true, false), Input::Button { injected: true });
    assert_eq!(pointer(Pointer::Wheel, false, false), Input::Wheel { injected: false });
    assert_eq!(pointer(Pointer::Other, false, false), Input::Ignored);
}

#[test]
fn escape_takes_over_and_is_kept_from_the_program_only_when_its_window_is_in_front() {
    let mut front = State::new();
    assert!(front.observe(Input::Escape { injected: false }, true), "meant for Aura");
    assert_eq!(front.gate(), Gate::TakenOver);
    let mut elsewhere = State::new();
    assert!(!elsewhere.observe(Input::Escape { injected: false }, false), "another program's own");
    assert_eq!(elsewhere.takeover(), Some(Takeover { cause: Cause::Escape, injected: false }));
}

#[test]
fn the_stop_keys_take_over_and_are_always_kept_from_the_program() {
    for target_in_front in [true, false] {
        let mut state = State::new();
        assert!(state.observe(Input::StopKeys { injected: false }, target_in_front));
        assert_eq!(state.takeover(), Some(Takeover { cause: Cause::StopKeys, injected: false }));
    }
}

#[test]
fn the_first_takeover_is_the_one_remembered_and_nothing_gives_the_desktop_back() {
    let mut state = State::new();
    state.observe(Input::Wheel { injected: true }, false);
    state.observe(Input::Button { injected: false }, false);
    state.take_over();
    state.observe(Input::Aura, true);
    assert_eq!(state.gate(), Gate::TakenOver);
    assert_eq!(state.takeover(), Some(Takeover { cause: Cause::Wheel, injected: true }));
}

#[test]
fn the_watch_ending_stops_the_desktop_without_an_input() {
    let mut state = State::new();
    state.take_over();
    assert_eq!(state.takeover(), Some(Takeover { cause: Cause::Stopped, injected: false }));
}

#[test]
fn a_takeover_says_what_did_it_and_whether_a_program_sent_it() {
    let words = |cause, injected| Takeover { cause, injected }.words();
    assert_eq!(words(Cause::Button, false), "点了鼠标");
    assert_eq!(words(Cause::Wheel, false), "滚了滚轮");
    assert_eq!(words(Cause::Escape, false), "按了 Esc");
    assert_eq!(words(Cause::StopKeys, false), "按了停止快捷键");
    assert_eq!(words(Cause::Stopped, false), "看护输入的线程已停止");
    assert_eq!(words(Cause::Button, true), "点了鼠标，来自另一个程序");
}

#[test]
fn the_stop_keys_are_their_key_with_exactly_their_modifiers() {
    let chord = stop_chord(PIP_STOP_KEYS).expect("the default stop keys");
    let held = |codes: Vec<u16>| move |code: u16| codes.contains(&code);
    let (control, alt, shift, windows, q) = (0x11, 0x12, 0x10, 0x5B, 0x51);
    assert!(is_chord(&chord, q, held(vec![control, alt])));
    assert!(!is_chord(&chord, q, held(vec![control])), "Alt is missing");
    assert!(!is_chord(&chord, q, held(vec![control, alt, shift])), "Shift is one too many");
    assert!(!is_chord(&chord, q, held(vec![control, alt, windows])), "so is Windows");
    assert!(!is_chord(&chord, 0x57, held(vec![control, alt])), "another key");
    assert!(!is_chord(&chord, u32::from(control), held(vec![control, alt])), "a modifier alone");
}
