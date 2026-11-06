//! Acting on Aura's test window element by element, from behind: the steps work while another
//! window is in front and the pointer does not move. Filling, selecting, expanding and scrolling
//! leave the window behind; a click on a button or a check box activates it as a user's click
//! would, and `focus` brings it to the front (that test is left out of ordinary runs).
mod support;

use aura_desktop_control::desktop::RealDesktop;
use aura_desktop_control::host::Desktop;
use aura_desktop_control::native::{focus, input};
use aura_desktop_domain::*;
use std::time::{Duration, Instant};
use support::*;

fn key(elements: &[Element], role: Role, name: &str) -> ElementKey {
    named(elements, role, name).key.clone()
}

/// A button's click and a check box's toggle reach the button as a user's click does, and the
/// window activates as it would then; Windows lets that bring it to the front only when no one
/// has used the computer for a while. So this test does not check the front; the next one does,
/// for the steps that never activate the window.
#[test]
#[ignore = "UIA Invoke can activate the test window; owner must be idle and desktop unlocked"]
fn steps_on_elements_change_the_window_from_behind_without_the_pointer() {
    let mut own = OwnFixture::new("act");
    let window = own.open((60, 60));
    let mut desktop = desktop();
    let pointer = input::cursor();
    let elements = read_all(&mut desktop, &own.app);
    let edit = at_place(&elements, own.place("edit")).key.clone();
    let combo = at_place(&elements, own.place("combo")).key.clone();
    let check = key(&elements, Role::CheckBox, "选项");
    let after = acted(act(
        &mut desktop,
        &own.app,
        &window.window,
        vec![
            ActStep::Click { element: key(&elements, Role::Button, "确定") },
            ActStep::SetValue { element: edit, value: "你好 Aura".into() },
            ActStep::Check { element: check.clone() },
            ActStep::Check { element: check.clone() },
            ActStep::Select { element: key(&elements, Role::ListItem, "香蕉") },
            ActStep::Expand { element: combo.clone() },
            ActStep::Collapse { element: combo },
            ActStep::Uncheck { element: check },
        ],
    ));
    let after: Vec<Element> =
        after.into_iter().map(|element| element.expect("still there")).collect();
    assert_eq!(after[1].value.as_deref(), Some("你好 Aura"));
    assert_eq!(after[2].checked, Some(Checked::On));
    assert_eq!(after[3].checked, Some(Checked::On), "checking a checked box leaves it checked");
    assert_eq!(after[4].selected, Some(true));
    assert_eq!(after[5].expanded, Some(true));
    assert_eq!(after[6].expanded, Some(false));
    assert_eq!(after[7].checked, Some(Checked::Off));
    // A list box tells its window of a selection only when the user makes it.
    let events: Vec<String> =
        own.since_ready().into_iter().filter(|event| !event.starts_with("list ")).collect();
    assert_eq!(
        events,
        ["click ok 1", "edit 你好 Aura", "check 1", "check 0"],
        "each step reached the window once, in order"
    );
    named(&read_all(&mut desktop, &own.app), Role::Text, "点了 1 次");
    assert_eq!(input::cursor(), pointer, "the pointer did not move");
}

#[test]
fn filling_selecting_expanding_and_scrolling_leave_the_window_behind() {
    let mut own = OwnFixture::new("act-behind");
    let window = own.open((70, 70));
    let handle = window_handle(&window.window).expect("handle");
    let mut desktop = desktop();
    let in_front = || focus::foreground().map(|(front, _)| front) == Some(handle);
    assert!(!in_front(), "it opened behind");
    let elements = read_all(&mut desktop, &own.app);
    let combo = at_place(&elements, own.place("combo")).key.clone();
    let after = acted(act(
        &mut desktop,
        &own.app,
        &window.window,
        vec![
            ActStep::SetValue {
                element: at_place(&elements, own.place("edit")).key.clone(),
                value: "后台".into(),
            },
            ActStep::Select { element: key(&elements, Role::ListItem, "橙子") },
            ActStep::Expand { element: combo.clone() },
            ActStep::Collapse { element: combo },
            ActStep::Scroll {
                element: at_place(&elements, own.place("long")).key.clone(),
                down: 2,
                right: 0,
            },
        ],
    ));
    assert_eq!(after[0].as_ref().and_then(|edit| edit.value.as_deref()), Some("后台"));
    assert!(!in_front(), "still behind");
}

#[test]
fn a_list_scrolls_and_what_comes_into_view_can_be_read() {
    let mut own = OwnFixture::new("act-scroll");
    let window = own.open((80, 80));
    let mut desktop = desktop();
    let elements = read_all(&mut desktop, &own.app);
    let long = at_place(&elements, own.place("long")).clone();
    assert!(long.abilities.contains(&Ability::Scroll), "{long:?}");
    assert!(elements.iter().all(|element| element.key.name != "第 12 项"));
    acted(act(
        &mut desktop,
        &own.app,
        &window.window,
        vec![ActStep::Scroll { element: long.key, down: 10, right: 0 }],
    ));
    let elements = read_all(&mut desktop, &own.app);
    named(&elements, Role::ListItem, "第 12 项");
    assert!(elements.iter().all(|element| element.key.name != "第 1 项"), "{}", outline(&elements));
}

#[test]
#[ignore = "UIA Invoke can activate the test window; owner must be idle and desktop unlocked"]
fn a_step_on_an_element_that_changed_or_is_gone_is_refused_as_stale() {
    let mut own = OwnFixture::new("act-stale");
    let window = own.open((100, 100));
    let mut desktop = desktop();
    let elements = read_all(&mut desktop, &own.app);
    let apple = key(&elements, Role::ListItem, "苹果");
    acted(act(
        &mut desktop,
        &own.app,
        &window.window,
        vec![ActStep::Click { element: key(&elements, Role::Button, "换水果") }],
    ));
    assert_eq!(own.since_ready(), ["fruits replaced"]);
    let error = failed(act(
        &mut desktop,
        &own.app,
        &window.window,
        vec![ActStep::Select { element: apple.clone() }],
    ));
    assert_eq!(error.code, HostErrorCode::Stale, "{error:?}");
    assert_eq!((error.delivery, error.steps_done), (Delivery::NotDelivered, Some(0)));
    let made_up = ElementKey { runtime_id: vec![42, 4242, 4242], ..apple };
    let error = failed(act(
        &mut desktop,
        &own.app,
        &window.window,
        vec![ActStep::Click { element: made_up }],
    ));
    assert_eq!(error.code, HostErrorCode::Stale);
    assert_eq!(own.since_ready(), ["fruits replaced"], "nothing else reached the window");
}

#[test]
#[ignore = "the partial-delivery case invokes a real button and can activate its window"]
fn steps_that_cannot_be_done_are_refused_before_they_reach_the_window() {
    let mut own = OwnFixture::new("act-refused");
    let window = own.open((120, 120));
    let mut desktop = desktop();
    let elements = read_all(&mut desktop, &own.app);
    let ok = key(&elements, Role::Button, "确定");
    let refused = |desktop: &mut RealDesktop, steps: Vec<ActStep>| {
        failed(act(desktop, &own.app, &window.window, steps))
    };
    for step in [
        ActStep::Click { element: key(&elements, Role::Button, "不能点") },
        ActStep::Click { element: key(&elements, Role::Text, "还没点") },
        ActStep::SetValue { element: ok.clone(), value: "x".into() },
        ActStep::Expand { element: ok.clone() },
    ] {
        let error = refused(&mut desktop, vec![step.clone()]);
        assert_eq!(error.code, HostErrorCode::Unsupported, "{step:?}: {error:?}");
        assert_eq!((error.delivery, error.steps_done), (Delivery::NotDelivered, Some(0)));
    }
    let error = refused(&mut desktop, Vec::new());
    assert_eq!(error.code, HostErrorCode::InvalidRequest);
    let error =
        refused(&mut desktop, vec![ActStep::Scroll { element: ok.clone(), down: 0, right: 0 }]);
    assert_eq!(error.code, HostErrorCode::InvalidRequest);
    assert!(own.since_ready().is_empty(), "{:?}", own.since_ready());

    let error = refused(
        &mut desktop,
        vec![
            ActStep::Click { element: ok },
            ActStep::Click { element: key(&elements, Role::Button, "不能点") },
        ],
    );
    assert_eq!(error.code, HostErrorCode::Unsupported);
    assert_eq!((error.delivery, error.steps_done), (Delivery::Uncertain, Some(1)));
    assert_eq!(own.since_ready(), ["click ok 1"]);
}

#[test]
fn a_step_in_a_window_that_stopped_answering_ends_in_time() {
    let mut own = OwnFixture::new("act-hung");
    let window = own.open_with((140, 140), &["--hang-at", "2000", "--hang-ms", "20000"]);
    let mut desktop = desktop();
    let ok = key(&read_all(&mut desktop, &own.app), Role::Button, "确定");
    assert!(
        own.wait_for(|events| events.iter().any(|event| event == "hang"), Duration::from_secs(10))
    );
    let started = Instant::now();
    let error = failed(desktop.run(HostCommand::Act {
        app: own.app.clone(),
        window: window.window.clone(),
        steps: vec![ActStep::Click { element: ok }],
        timeout_ms: 2_000,
    }));
    let took = started.elapsed();
    assert_eq!(error.code, HostErrorCode::Timeout, "{error:?}");
    // The window did not answer before the first step began.
    assert_eq!((error.delivery, error.steps_done), (Delivery::NotDelivered, None));
    assert!(took < Duration::from_millis(3_500), "{took:?}");
}

#[test]
#[ignore = "brings the test window to the front: run while no one is using the computer"]
fn focus_brings_the_window_to_the_front() {
    let mut own = OwnFixture::new("act-focus");
    let window = own.open((160, 160));
    let mut desktop = desktop();
    let elements = read_all(&mut desktop, &own.app);
    let edit = at_place(&elements, own.place("edit")).key.clone();
    let after =
        acted(act(&mut desktop, &own.app, &window.window, vec![ActStep::Focus { element: edit }]));
    assert!(after[0].as_ref().is_some_and(|element| element.focused));
    assert_eq!(focus::foreground().map(|(front, _)| front), window_handle(&window.window));
}
