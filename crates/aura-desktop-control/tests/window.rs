//! A program's window minimized, restored and closed by the host on this computer's real
//! desktop, and the window of the Aura that started the host left alone. None of these takes the
//! focus; maximizing, bringing to the front and opening a program do, and are run by hand
//! (`--ignored`) when no one is using the computer.
mod support;

use aura_desktop_control::desktop::RealDesktop;
use aura_desktop_control::host::Desktop;
use aura_desktop_control::native::focus;
use aura_desktop_domain::*;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::*;

fn arrange(
    desktop: &mut RealDesktop,
    app: &AppEntry,
    window: &str,
    action: WindowAction,
) -> HostOutcome {
    desktop.run(HostCommand::Window { app: app.clone(), window: Some(window.into()), action })
}

fn arranged(outcome: HostOutcome) -> (bool, Vec<WindowInfo>) {
    match done(outcome) {
        HostResult::Window { changed, windows, .. } => (changed, windows),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_window_is_minimized_from_behind_restored_where_it_was_and_closed() {
    let mut desktop = desktop();
    let mut fixture = OwnFixture::new("arrange");
    let window = fixture.open((60, 60));
    let (changed, windows) =
        arranged(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Minimize));
    assert!(changed);
    assert!(windows[0].minimized && !windows[0].foreground, "{windows:?}");
    let (again, _) =
        arranged(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Minimize));
    assert!(!again, "it was minimized already");
    let error = failed(read(&mut desktop, &fixture.app, None, 100, 8, 5_000));
    assert_eq!(error.code, HostErrorCode::Unsupported, "nothing of it is on the screen: {error:?}");

    let (changed, windows) =
        arranged(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Restore));
    assert!(changed);
    assert!(!windows[0].minimized && !windows[0].foreground, "{windows:?}");
    assert_eq!(windows[0].rect, window.rect, "back where it was");

    let (changed, windows) =
        arranged(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Close));
    assert!(changed);
    assert!(windows.is_empty(), "{windows:?}");
}

#[test]
fn a_window_that_closes_slowly_is_waited_for_and_one_that_stays_open_is_listed() {
    let mut desktop = desktop();
    let mut slow = OwnFixture::new("close-slow");
    let window = slow.open_with((60, 60), &["--close-ms", "800"]);
    let started = Instant::now();
    let (changed, windows) =
        arranged(arrange(&mut desktop, &slow.app, &window.window, WindowAction::Close));
    assert!(changed);
    assert!(windows.is_empty(), "the reply waited for the window to go: {windows:?}");
    assert!(started.elapsed() >= Duration::from_millis(700), "{:?}", started.elapsed());

    // As a program that asks whether to save: the window is still there when the wait ends.
    let mut stays = OwnFixture::new("close-stays");
    let window = stays.open_with((90, 90), &["--close-ms", "30000"]);
    let started = Instant::now();
    let (changed, windows) =
        arranged(arrange(&mut desktop, &stays.app, &window.window, WindowAction::Close));
    let took = started.elapsed();
    assert!(changed, "it was asked to close");
    let listed: Vec<&str> = windows.iter().map(|one| one.window.as_str()).collect();
    assert_eq!(listed, [window.window.as_str()], "a window that stays open is listed");
    assert!(
        took >= Duration::from_millis(2_900) && took < Duration::from_millis(4_500),
        "{took:?}"
    );
    assert_eq!(stays.since_ready(), ["close asked"]);
}

#[test]
fn a_window_without_a_maximize_button_is_not_maximized() {
    let mut desktop = desktop();
    let mut fixture = OwnFixture::new("fixed");
    let window = fixture.open_with((80, 80), &["--no-maximize"]);
    let error = failed(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Maximize));
    assert_eq!(error.code, HostErrorCode::Unsupported, "{error:?}");
    assert_eq!(error.delivery, Delivery::NotDelivered, "{error:?}");
    let (changed, _) =
        arranged(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Minimize));
    assert!(changed, "it can still be minimized");
}

#[test]
#[ignore = "maximizes the test window and brings it to the front: run by hand when no one is using the computer"]
fn a_window_is_maximized_brought_to_the_front_and_restored() {
    let mut desktop = desktop();
    let mut fixture = OwnFixture::new("front");
    let window = fixture.open((100, 100));
    let (changed, windows) =
        arranged(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Maximize));
    assert!(changed);
    assert!(!windows[0].minimized && windows[0].rect.width > window.rect.width, "{windows:?}");
    assert!(windows[0].foreground, "maximizing brings it to the front: {windows:?}");
    let mut other = OwnFixture::new("front-other");
    let other_window = other.open((300, 300));
    assert!(focus::bring_to_front(window_handle(&other_window.window).expect("handle")));
    let (changed, windows) =
        arranged(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Maximize));
    assert!(changed && windows[0].foreground, "maximized behind another: {windows:?}");
    let (changed, windows) =
        arranged(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Restore));
    assert!(changed);
    assert_eq!(windows[0].rect, window.rect, "{windows:?}");

    arranged(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Minimize));
    let (changed, windows) =
        arranged(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Front));
    assert!(changed);
    assert!(!windows[0].minimized && windows[0].foreground, "{windows:?}");
    let (again, _) =
        arranged(arrange(&mut desktop, &fixture.app, &window.window, WindowAction::Front));
    assert!(!again, "it was in front already");
}

fn opened(outcome: HostOutcome) -> (Vec<WindowInfo>, bool) {
    match done(outcome) {
        HostResult::Launched { windows, already_running } => (windows, already_running),
        other => panic!("{other:?}"),
    }
}

#[test]
#[ignore = "opens the test window and brings it to the front: run by hand when no one is using the computer"]
fn opening_a_program_brings_its_window_to_the_front_whether_it_ran_or_not() {
    let mut desktop = desktop();
    let mut fixture = OwnFixture::new("open-front");
    let launch = |app: &AppEntry| HostCommand::Launch { app: app.clone(), timeout_ms: 15_000 };
    let (windows, already) = opened(desktop.run(launch(&fixture.app)));
    fixture.processes.extend(windows.iter().map(|window| window.process_id));
    assert!(!already && windows[0].foreground, "a new window: {windows:?}");
    let mut other = OwnFixture::new("open-other");
    let other_window = other.open((300, 300));
    assert!(focus::bring_to_front(window_handle(&other_window.window).expect("handle")));
    let (windows, already) = opened(desktop.run(launch(&fixture.app)));
    assert!(already && windows[0].foreground, "a window it had: {windows:?}");
}

/// One request to the host and its reply. A request on a program's window also gets control lines
/// before its reply, which say whether the host is paused and how many steps are done.
fn ask(input: &mut impl Write, output: &mut impl BufRead, id: u64, command: Value) -> Value {
    writeln!(input, "{}", json!({"id": id, "command": command})).expect("sent");
    loop {
        let mut line = String::new();
        output.read_line(&mut line).expect("reply");
        let message: Value = serde_json::from_str(&line).expect("json");
        assert_eq!(message["id"], id, "{message}");
        if message["type"] == "reply" {
            return message["outcome"].clone();
        }
        assert_eq!(message["type"], "control", "{message}");
    }
}

#[test]
fn the_window_of_the_aura_that_started_the_host_is_no_programs() {
    // The test window starts the host as Core does: to the host it is Aura.
    let mut fixture = OwnFixture::new("aura");
    let mut started = Command::new(&fixture.app.id)
        .arg("--log")
        .arg(&fixture.log)
        .args(["--host", HOST])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the test window");
    fixture.processes.push(started.id());
    let mut input = started.stdin.take().expect("stdin");
    let mut output = BufReader::new(started.stdout.take().expect("stdout"));
    let mut line = String::new();
    output.read_line(&mut line).expect("ready");
    assert_eq!(serde_json::from_str::<Value>(&line).expect("json")["type"], "ready", "{line}");
    assert!(
        fixture.wait_for(|events| events.iter().any(|e| e == "ready"), Duration::from_secs(20))
    );

    let app = serde_json::to_value(&fixture.app).expect("app");
    let listed = ask(&mut input, &mut output, 1, json!({"op": "windows", "apps": [app]}));
    let windows = listed["done"]["windows"].as_array().expect("windows").clone();
    let own: Vec<&Value> =
        windows.iter().filter(|window| window["process_id"] == started.id()).collect();
    assert_eq!(own.len(), 1, "the window is on the desktop: {windows:?}");
    assert!(own[0].get("app_id").is_none(), "and no program's: {own:?}");

    let read = json!({"op": "elements", "app": app, "max_elements": 10, "max_depth": 2, "timeout_ms": 5_000});
    let minimize = json!({"op": "window", "app": app, "action": "minimize"});
    for (id, command) in [(2, read), (3, minimize)] {
        let refused = ask(&mut input, &mut output, id, command);
        assert_eq!(refused["failed"]["code"], "window_not_found", "{refused}");
    }
    drop(input);
    let _ = started.kill();
    let _ = started.wait();
}
