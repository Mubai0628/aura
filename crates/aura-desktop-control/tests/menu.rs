//! A program's menus on the real desktop. Finding the windows of the program's processes sends
//! nothing and runs with the other tests. The menu test right-clicks Aura's test window and
//! presses keys, so it is run by hand (`--ignored`) when no one is using the computer: the menu,
//! a window of its own that takes the front and closes when it loses it, stays open while the
//! screenshot takes it in and the program is brought to the front, and the next keys reach it.
mod support;

use aura_desktop_control::desktop::shot;
use aura_desktop_control::host::Desktop;
use aura_desktop_control::native::{focus, window_list};
use aura_desktop_domain::*;
use std::collections::BTreeSet;
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

fn press(keys: &str) -> InputStep {
    InputStep::Press { keys: keys.into(), repeat: 1 }
}

fn inside(area: Rect, rect: Rect) -> bool {
    area.contains(rect.x, rect.y)
        && area.contains(rect.x + rect.width - 1, rect.y + rect.height - 1)
}

#[test]
fn the_windows_of_a_programs_processes_are_found_with_their_bounds() {
    let _desktop = desktop();
    let mut fixture = OwnFixture::new("by-process");
    let window = fixture.open((180, 150));
    let handle = window_handle(&window.window).expect("a window handle");
    let found = window_list::of_processes(&BTreeSet::from([window.process_id]));
    assert!(found.contains(&(handle, window.rect)), "{found:?}");
    assert!(window_list::of_processes(&BTreeSet::new()).is_empty());
}

#[test]
#[ignore = "right-clicks the test window and presses keys: run by hand when no one is using the computer"]
fn a_menu_in_front_stays_open_for_the_screenshot_and_front_and_takes_the_next_keys() {
    let mut desktop = desktop();
    let mut fixture = OwnFixture::new("menu");
    let window = fixture.open_with((200, 160), &["--menu"]);
    let handle = window_handle(&window.window).expect("a window handle");
    let client = fixture.place("client");
    let at = Point { x: client.x + 20, y: client.y + client.height - 20 };
    let right = InputStep::Click { at, button: MouseButton::Right, count: 1 };
    assert_eq!(
        done(desktop.run(input(&fixture.app, vec![right]))),
        HostResult::Input { steps_done: 1 }
    );
    // The input waited for the menu to open.
    let events = fixture.since_ready();
    assert!(events.contains(&"menu open".to_owned()), "{events:?}");
    let (menu, place) = window_list::of_processes(&BTreeSet::from([window.process_id]))
        .into_iter()
        .find(|(other, _)| *other != handle)
        .expect("the menu is a window of the program");
    assert_eq!(focus::foreground().map(|(front, _)| front), Some(menu), "the menu took the front");

    let shot = shot(std::slice::from_ref(&fixture.app), true, None).expect("screenshot");
    assert!(inside(shot.area, place), "{:?} leaves out the menu at {place:?}", shot.area);
    let front = HostCommand::Window {
        app: fixture.app.clone(),
        window: Some(window.window.clone()),
        action: WindowAction::Front,
    };
    match done(desktop.run(front)) {
        HostResult::Window { changed, .. } => assert!(!changed, "the program was in front"),
        other => panic!("{other:?}"),
    }
    let events = fixture.since_ready();
    assert!(!events.contains(&"menu closed".to_owned()), "{events:?}");

    let keys = input(&fixture.app, vec![press("Down"), press("Enter")]);
    assert_eq!(done(desktop.run(keys)), HostResult::Input { steps_done: 2 });
    assert!(
        fixture.wait_for(|events| events.iter().any(|event| event == "menu 1"), SETTLE),
        "{:?}",
        fixture.since_ready()
    );
}
