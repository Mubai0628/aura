//! 设置 → 电脑操控. Off by default, no program always allowed or added, unauthorized programs
//! painted out of screenshots, no clipboard and no system chords. The lists are saved without
//! repeats and read back after a restart; a bad value is refused and changes nothing; a file Aura
//! cannot trust turns computer control off instead of reading as something else.
use aura_config::{
    AuraPaths, ComputerAddedApp, ComputerApp, ComputerControlSettings, ConfigModule,
    DEFAULT_SCREENSHOT_EDGE, SettingsFacade,
};
use aura_domain_common::ComputerMode;
use serde_json::json;
use std::sync::Arc;

fn facade() -> (tempfile::TempDir, AuraPaths, Arc<SettingsFacade>) {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let facade = SettingsFacade::open(paths.clone()).expect("settings");
    (temp, paths, facade)
}

fn app(id: &str, name: &str) -> ComputerApp {
    ComputerApp { app_id: id.into(), name: name.into() }
}

#[test]
fn pip_stop_keys_persist_and_invalid_keys_leave_the_previous_revision() {
    let (_temp, paths, facade) = facade();
    let mut settings = facade.computer_settings().settings;
    assert_eq!(settings.pip_stop_keys, "Ctrl+Alt+Q");
    settings.pip_stop_keys = "Ctrl+Shift+Q".into();
    facade.update_computer_settings(1, &settings).expect("save shortcut");
    settings.pip_stop_keys = "Q".into();
    assert!(facade.update_computer_settings(2, &settings).is_err());
    drop(facade);
    let reopened = SettingsFacade::open(paths).expect("reopen");
    assert_eq!(reopened.computer_settings().settings.pip_stop_keys, "Ctrl+Shift+Q");
    assert_eq!(reopened.computer_settings().revision, Some(2));
}

#[test]
fn the_pause_switch_of_an_older_aura_is_read_past_and_dropped_by_the_next_save() {
    let (_temp, paths, facade) = facade();
    facade
        .update(ConfigModule::Computer, 1, |values| {
            values.insert("enabled".into(), json!(true));
            values.insert("pause_on_input".into(), json!(false));
        })
        .expect("an older Aura's settings");
    let snapshot = facade.computer_settings();
    assert!(snapshot.issue.is_none(), "{:?}", snapshot.issue);
    assert!(snapshot.settings.enabled);
    let typed = serde_json::to_value(&snapshot.settings).expect("settings");
    assert!(typed.get("pause_on_input").is_none(), "{typed}");
    facade.update_computer_settings(2, &snapshot.settings).expect("save");
    let written = std::fs::read_to_string(paths.config_path(ConfigModule::Computer)).expect("file");
    assert!(written.contains("enabled = true") && !written.contains("pause_on_input"), "{written}");
}

#[test]
fn computer_control_is_off_by_default_and_screenshots_paint_out_other_programs() {
    let (_temp, _paths, facade) = facade();
    let snapshot = facade.computer_settings();
    assert_eq!(
        snapshot.settings,
        ComputerControlSettings {
            pip_stop_keys: "Ctrl+Alt+Q".to_owned(),
            enabled: false,
            always_allowed: vec![],
            added: vec![],
            paint_others: true,
            clipboard: false,
            system_keys: false,
            screenshot_long_edge: DEFAULT_SCREENSHOT_EDGE,
            models: vec![],
            mode: ComputerMode::Foreground,
        }
    );
    assert_eq!(snapshot.revision, Some(1));
    assert!(snapshot.issue.is_none());
}

#[test]
fn the_lists_are_saved_without_repeats_and_read_back_after_a_restart() {
    let (_temp, paths, facade) = facade();
    let chosen = ComputerControlSettings {
        pip_stop_keys: "Ctrl+Alt+Q".to_owned(),
        enabled: true,
        always_allowed: vec![
            app("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "记事本"),
            app("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "记事本"),
            app("path:c:\\tools\\viewer.exe", "Viewer"),
        ],
        added: vec![
            ComputerAddedApp { path: "C:\\Tools\\viewer.exe".into(), name: String::new() },
            ComputerAddedApp { path: "c:\\tools\\VIEWER.exe".into(), name: "又一个".into() },
        ],
        paint_others: false,
        clipboard: true,
        system_keys: true,
        screenshot_long_edge: 1_280,
        models: vec!["gpt-6.1-sol".into()],
        mode: ComputerMode::ChildSession,
    };
    facade.update_computer_settings(1, &chosen).expect("save");
    let saved = ComputerControlSettings {
        always_allowed: vec![chosen.always_allowed[0].clone(), chosen.always_allowed[2].clone()],
        added: vec![chosen.added[0].clone()],
        ..chosen.clone()
    };
    assert_eq!(facade.computer_settings().settings, saved);
    assert_eq!(facade.computer_settings().revision, Some(2));
    drop(facade);
    let reopened = SettingsFacade::open(paths).expect("reopen");
    assert_eq!(reopened.computer_settings().settings, saved);
    // A stale revision is refused and changes nothing.
    assert!(reopened.update_computer_settings(1, &ComputerControlSettings::default()).is_err());
    assert_eq!(reopened.computer_settings().settings, saved);
}

#[test]
fn a_bad_value_is_refused_and_nothing_changes() {
    let (_temp, _paths, facade) = facade();
    let defaults = ComputerControlSettings::default;
    let cases = [
        ComputerControlSettings { screenshot_long_edge: 639, ..defaults() },
        ComputerControlSettings { screenshot_long_edge: 3_841, ..defaults() },
        ComputerControlSettings { always_allowed: vec![app("", "记事本")], ..defaults() },
        ComputerControlSettings { always_allowed: vec![app("a", " ")], ..defaults() },
        ComputerControlSettings {
            added: vec![ComputerAddedApp { path: "viewer.exe".into(), name: String::new() }],
            ..defaults()
        },
        ComputerControlSettings {
            always_allowed: (0..201).map(|n| app(&format!("app-{n}"), "程序")).collect(),
            ..defaults()
        },
    ];
    for settings in cases {
        assert!(facade.update_computer_settings(1, &settings).is_err(), "{settings:?}");
        assert_eq!(facade.computer_settings().revision, Some(1));
    }
    for (field, value) in [
        ("enabled", json!("yes")),
        ("clipboard", json!(1)),
        ("always_allowed", json!("记事本")),
        ("added", json!([{"path": "C:\\a.exe", "extra": true}])),
        ("models", json!("gpt")),
        ("mode", json!("fallback_to_foreground")),
    ] {
        let written = facade.update(ConfigModule::Computer, 1, |values| {
            values.insert(field.into(), value);
        });
        assert!(written.is_err(), "{field} must be refused");
        assert_eq!(facade.computer_settings().revision, Some(1));
    }
}

#[test]
fn a_file_changed_into_something_invalid_turns_computer_control_off() {
    let (_temp, paths, facade) = facade();
    let chosen = ComputerControlSettings {
        enabled: true,
        always_allowed: vec![app("Microsoft.Paint_8wekyb3d8bbwe!App", "画图")],
        ..ComputerControlSettings::default()
    };
    facade.update_computer_settings(1, &chosen).expect("save");
    std::fs::write(
        paths.config_path(ConfigModule::Computer),
        b"schema_version = 1\nrevision = 3\nenabled = true\nalways_allowed = 'Paint'\n",
    )
    .expect("external edit");
    let snapshot = facade.computer_settings();
    assert!(!snapshot.settings.enabled, "computer control must not run on settings it cannot read");
    assert!(snapshot.settings.always_allowed.is_empty());
    assert!(snapshot.issue.as_deref().is_some_and(|issue| issue.contains("恢复上一版有效配置")));
}
