//! The host on this computer's real desktop: the Start menu's programs and their icons, opening
//! Aura's own test window and finding it again, and the host program itself over its standard
//! input and output.
//! Opening a program brings its window to the front, so the tests that open one with a window are
//! run by hand (`--ignored`) when no one is using the computer; the others never take the focus.
mod support;

use aura_desktop_control::host::Desktop;
use aura_desktop_domain::*;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use support::*;

#[test]
fn all_apps_has_notepad_as_a_store_app_and_programs_with_their_files() {
    let mut desktop = desktop();
    let apps = catalog(&mut desktop, Vec::new());
    assert!(apps.len() > 20, "{} programs", apps.len());
    let notepad =
        apps.iter().find(|app| app.id.starts_with("Microsoft.WindowsNotepad_")).expect("Notepad");
    assert_eq!(notepad.kind, AppKind::Packaged);
    assert!(notepad.id.ends_with("!App"), "{notepad:?}");
    let with_files: Vec<&AppEntry> = apps.iter().filter(|app| app.path.is_some()).collect();
    assert!(!with_files.is_empty());
    for app in with_files {
        let path = app.path.as_deref().unwrap_or_default();
        assert!(path.to_ascii_lowercase().ends_with(".exe"), "{app:?}");
        assert!(!path.starts_with('{'), "a known folder is filled in: {app:?}");
    }
    let mut ids: Vec<String> = apps.iter().map(|app| app.id.to_lowercase()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), apps.len(), "each program once");
}

#[test]
fn the_icons_of_a_store_app_a_program_and_an_added_program_are_pngs_of_the_size_asked() {
    use base64::Engine as _;
    let mut desktop = desktop();
    let fixture = OwnFixture::new("icon");
    let manual = vec![ManualApp { name: fixture.app.name.clone(), path: fixture.app.id.clone() }];
    let apps = catalog(&mut desktop, manual);
    let pick = |wanted: &dyn Fn(&AppEntry) -> bool| apps.iter().find(|app| wanted(app)).cloned();
    let notepad = pick(&|app| app.id.starts_with("Microsoft.WindowsNotepad_")).expect("Notepad");
    let program =
        pick(&|app| app.kind == AppKind::Desktop && app.path.is_some()).expect("a program");
    let added = pick(&|app| app.id == fixture.app.id).expect("the added program");
    let asked = vec![notepad.clone(), program.clone(), added.clone()];
    let HostResult::Icons { icons } =
        done(desktop.run(HostCommand::Icons { apps: asked, size: 32 }))
    else {
        panic!("not icons")
    };
    let ids: Vec<&str> = icons.iter().map(|icon| icon.app_id.as_str()).collect();
    assert_eq!(ids, [notepad.id.as_str(), program.id.as_str(), added.id.as_str()]);
    for icon in &icons {
        let png = base64::engine::general_purpose::STANDARD.decode(&icon.png).expect("base64");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "{}", icon.app_id);
        let side = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().expect("4 bytes"));
        assert_eq!((side(16), side(20)), (32, 32), "{}", icon.app_id);
        assert_eq!(png[25], 6, "with transparency: {}", icon.app_id);
    }
    let refused = failed(desktop.run(HostCommand::Icons { apps: vec![notepad], size: 0 }));
    assert_eq!(refused.code, HostErrorCode::InvalidRequest);
}

#[test]
#[ignore = "opens the test window and brings it to the front: run by hand when no one is using the computer"]
fn opening_a_program_waits_for_its_new_window_and_does_not_open_it_twice() {
    let mut desktop = desktop();
    let mut fixture = OwnFixture::new("open");
    let manual = vec![ManualApp { name: fixture.app.name.clone(), path: fixture.app.id.clone() }];
    let apps = catalog(&mut desktop, manual);
    let app = apps.iter().find(|app| app.id == fixture.app.id).expect("the added program").clone();
    assert_eq!(app.kind, AppKind::Manual);

    let opened = done(desktop.run(HostCommand::Launch { app: app.clone(), timeout_ms: 20_000 }));
    let HostResult::Launched { windows, already_running } = opened else { panic!("{opened:?}") };
    fixture.processes.extend(windows.iter().map(|window| window.process_id));
    assert!(!already_running);
    assert_eq!(windows.len(), 1, "{windows:?}");
    let window = &windows[0];
    assert_eq!(window.app_id.as_deref(), Some(app.id.as_str()));
    assert!(window.title.starts_with("Aura 测试窗口"), "{window:?}");
    assert_eq!(window.process_name, "aura-fixture-open.exe");
    assert!(!window.minimized && !window.rect.is_empty(), "{window:?}");
    assert!(window.foreground, "opening a program brings its window to the front");

    let again = done(desktop.run(HostCommand::Launch { app: app.clone(), timeout_ms: 20_000 }));
    let HostResult::Launched { windows: same, already_running } = again else {
        panic!("{again:?}")
    };
    assert!(already_running);
    assert_eq!(same.iter().map(|w| &w.window).collect::<Vec<_>>(), [&window.window]);

    let listed = done(desktop.run(HostCommand::Windows { apps: apps.clone() }));
    let HostResult::Windows { windows: all } = listed else { panic!("{listed:?}") };
    let mine: Vec<&WindowInfo> =
        all.iter().filter(|w| w.app_id.as_deref() == Some(app.id.as_str())).collect();
    assert_eq!(mine.len(), 1, "{all:?}");
    assert_eq!(window_handle(&mine[0].window), window_handle(&window.window));
}

#[test]
fn a_program_that_shows_no_window_in_time_is_reported_as_maybe_opened() {
    let mut desktop = desktop();
    let fixture = OwnFixture::new("none");
    // The program opens, but the entry names another program file: no window is the entry's.
    let app =
        AppEntry { path: Some(format!("{}.other.exe", fixture.app.id)), ..fixture.app.clone() };
    let outcome = desktop.run(HostCommand::Launch { app, timeout_ms: 1_500 });
    let _ = Command::new("taskkill").args(["/IM", "aura-fixture-none.exe", "/F"]).output();
    let HostOutcome::Failed(error) = outcome else { panic!("{outcome:?}") };
    assert_eq!(error.code, HostErrorCode::LaunchFailed, "{error:?}");
    assert_eq!(error.delivery, Delivery::Uncertain, "{error:?}");
}

#[test]
fn a_program_file_that_is_not_there_is_reported_as_not_opened() {
    let mut desktop = desktop();
    let fixture = OwnFixture::new("gone");
    let missing = format!("{}.gone.exe", fixture.app.id);
    let app = AppEntry { id: missing.clone(), path: Some(missing), ..fixture.app.clone() };
    let outcome = desktop.run(HostCommand::Launch { app, timeout_ms: 5_000 });
    let HostOutcome::Failed(error) = outcome else { panic!("{outcome:?}") };
    assert_eq!(error.code, HostErrorCode::LaunchFailed, "{error:?}");
    assert_eq!(error.delivery, Delivery::NotDelivered, "{error:?}");
}

#[test]
#[ignore = "opens Calculator on the desktop: run by hand when no one is using the computer"]
fn a_store_apps_frame_window_belongs_to_the_app_drawing_inside_it() {
    let mut desktop = desktop();
    let apps = catalog(&mut desktop, Vec::new());
    let calculator = apps
        .iter()
        .find(|app| app.id.starts_with("Microsoft.WindowsCalculator_"))
        .expect("Calculator")
        .clone();
    let opened =
        done(desktop.run(HostCommand::Launch { app: calculator.clone(), timeout_ms: 20_000 }));
    let HostResult::Launched { windows, .. } = opened else { panic!("{opened:?}") };
    assert!(!windows.is_empty());
    for window in &windows {
        assert_eq!(window.app_id.as_deref(), Some(calculator.id.as_str()), "{window:?}");
        assert_ne!(
            window.process_name.to_ascii_lowercase(),
            "applicationframehost.exe",
            "{window:?}"
        );
    }
    let _ = Command::new("taskkill").args(["/IM", "CalculatorApp.exe", "/F"]).output();
}

#[test]
fn the_host_program_speaks_the_protocol_and_ends_when_its_input_closes() {
    let mut host = Command::new(HOST)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("host");
    let mut input = host.stdin.take().expect("stdin");
    let mut output = BufReader::new(host.stdout.take().expect("stdout"));
    let mut line = String::new();
    output.read_line(&mut line).expect("ready");
    assert_eq!(
        serde_json::from_str::<Value>(&line).expect("json"),
        json!({"type": "ready", "protocol": DESKTOP_PROTOCOL_VERSION})
    );

    writeln!(input, "{}", json!({"id": 1, "command": {"op": "catalog"}})).expect("sent");
    writeln!(input, "oops").expect("sent");
    line.clear();
    output.read_line(&mut line).expect("reply");
    let reply: Value = serde_json::from_str(&line).expect("json");
    assert_eq!(reply["id"], 1);
    assert!(
        reply["outcome"]["done"]["apps"].as_array().is_some_and(|apps| !apps.is_empty()),
        "{reply}"
    );
    line.clear();
    output.read_line(&mut line).expect("reply");
    let refused: Value = serde_json::from_str(&line).expect("json");
    assert_eq!(refused["outcome"]["failed"]["code"], "invalid_request", "{refused}");

    drop(input);
    let status = host.wait().expect("ended");
    assert!(status.success(), "{status:?}");
}
