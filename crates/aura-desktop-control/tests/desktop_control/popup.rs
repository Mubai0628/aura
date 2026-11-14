//! After a click or a key, a window the program opens is waited for until it stops changing; a
//! step that opens nothing waits only a moment.
use aura_desktop_control::plan::{Allowed, plan};
use aura_desktop_control::popup::{
    APPEAR_AFTER_MENU_MS, APPEAR_MS, LIMIT_MS, Next, POLL_MS, appear_ms, next, settle,
};
use aura_desktop_domain::{InputStep, MouseButton, Point, Rect};
use std::collections::BTreeSet;

const MAIN: (isize, Rect) = (11, Rect { x: 0, y: 0, width: 800, height: 600 });
const MENU: (isize, Rect) = (22, Rect { x: 300, y: 200, width: 160, height: 240 });

fn before() -> BTreeSet<isize> {
    BTreeSet::from([MAIN.0])
}

#[test]
fn a_menu_is_waited_for_until_it_is_the_same_twice() {
    let growing = (MENU.0, Rect { height: 120, ..MENU.1 });
    assert_eq!(next(&before(), &[MAIN], &[MAIN, growing], 30, APPEAR_MS), Next::Wait);
    assert_eq!(next(&before(), &[MAIN, growing], &[MAIN, MENU], 60, APPEAR_MS), Next::Wait);
    assert_eq!(next(&before(), &[MAIN, MENU], &[MAIN, MENU], 90, APPEAR_MS), Next::Go);
}

#[test]
fn without_a_new_window_the_step_waits_only_until_one_could_have_appeared() {
    assert_eq!(next(&before(), &[MAIN], &[MAIN], 0, APPEAR_MS), Next::Wait);
    assert_eq!(next(&before(), &[MAIN], &[MAIN], APPEAR_MS - 1, APPEAR_MS), Next::Wait);
    assert_eq!(next(&before(), &[MAIN], &[MAIN], APPEAR_MS, APPEAR_MS), Next::Go);
    // A click that closed the menu: nothing new, so nothing more to wait for.
    let open = BTreeSet::from([MAIN.0, MENU.0]);
    assert_eq!(next(&open, &[MAIN], &[MAIN], APPEAR_MS, APPEAR_MS), Next::Go);
}

#[test]
fn a_window_that_keeps_changing_is_waited_for_no_longer_than_the_limit() {
    let moved = (MENU.0, Rect { x: 301, ..MENU.1 });
    assert_eq!(next(&before(), &[MAIN, MENU], &[MAIN, moved], LIMIT_MS - 1, APPEAR_MS), Next::Wait);
    assert_eq!(next(&before(), &[MAIN, MENU], &[MAIN, moved], LIMIT_MS, APPEAR_MS), Next::Go);
}

#[test]
fn clicks_and_keys_wait_for_what_they_open_and_menus_longer() {
    let at = Point { x: 10, y: 10 };
    let steps = [
        InputStep::Click { at, button: MouseButton::Right, count: 1 },
        InputStep::Click { at, button: MouseButton::Left, count: 2 },
        InputStep::Press { keys: "Shift+F10".into(), repeat: 1 },
        InputStep::Press { keys: "Apps".into(), repeat: 1 },
        InputStep::Press { keys: "F10".into(), repeat: 1 },
        InputStep::Press { keys: "Up".into(), repeat: 2 },
        InputStep::Move { at },
        InputStep::Scroll { at, down: 1, right: 0 },
        InputStep::Type { text: "hi".into() },
        InputStep::Wait { ms: 10 },
    ];
    let planned = plan(&steps, Allowed::default()).expect("planned");
    let waits: Vec<_> = planned.iter().map(appear_ms).collect();
    assert_eq!(
        waits,
        [
            Some(APPEAR_AFTER_MENU_MS),
            Some(APPEAR_MS),
            Some(APPEAR_AFTER_MENU_MS),
            Some(APPEAR_AFTER_MENU_MS),
            Some(APPEAR_MS),
            Some(APPEAR_MS),
            None,
            None,
            None,
            None,
        ]
    );
}

/// How many looks `settle` takes, with a clock that moves [`POLL_MS`] per wait and `look`
/// given the number of the look.
fn looks(appear_ms: u64, mut look: impl FnMut(usize) -> Vec<(isize, Rect)>) -> usize {
    let (mut clock, mut count) = (0, 0);
    settle(
        &before(),
        appear_ms,
        || {
            clock += POLL_MS;
            clock
        },
        || {
            count += 1;
            look(count)
        },
    );
    count
}

#[test]
fn settling_looks_until_the_menu_is_the_same_twice() {
    let growing = (MENU.0, Rect { height: 120, ..MENU.1 });
    let count = looks(APPEAR_MS, |look| match look {
        1 => vec![MAIN],
        2 => vec![MAIN, growing],
        _ => vec![MAIN, MENU],
    });
    assert_eq!(count, 4);
}

#[test]
fn settling_without_a_new_window_lasts_until_one_could_have_appeared() {
    let polls = |ms: u64| (ms / POLL_MS) as usize;
    assert_eq!(looks(APPEAR_MS, |_| vec![MAIN]), polls(APPEAR_MS));
    assert_eq!(looks(APPEAR_AFTER_MENU_MS, |_| vec![MAIN]), polls(APPEAR_AFTER_MENU_MS));
}

#[test]
fn settling_stops_at_the_limit_for_a_window_that_keeps_moving() {
    let count = looks(APPEAR_MS, |look| vec![MAIN, (MENU.0, Rect { x: look as i32, ..MENU.1 })]);
    assert_eq!(count, (LIMIT_MS / POLL_MS) as usize);
}
