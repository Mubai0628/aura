//! The interface settings in 设置 → 环境 — whether the session shows its
//! debugging information (default off) and which terminal a new terminal tab opens (default
//! PowerShell 7).
use aura_config::{
    AuraPaths, BackgroundMotion, ConfigModule, DefaultTerminal, SettingsFacade, UiSettings,
};
use serde_json::json;
use std::sync::Arc;

/// An edit outside Aura that names a terminal Aura does not offer.
const DAMAGED: &[u8] = b"schema_version = 1\nrevision = 3\ndefault_terminal = 'zsh'\n";

fn facade() -> (tempfile::TempDir, AuraPaths, Arc<SettingsFacade>) {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let facade = SettingsFacade::open(paths.clone()).expect("settings");
    (temp, paths, facade)
}

#[test]
fn defaults_hide_debugging_information_and_open_powershell() {
    let (_temp, _paths, facade) = facade();
    let snapshot = facade.ui_settings();
    assert_eq!(
        snapshot.settings,
        UiSettings {
            show_debug_information: false,
            default_terminal: DefaultTerminal::PowerShell,
            plugin_theme: None,
            background_motion: BackgroundMotion::System,
        }
    );
    assert_eq!(snapshot.revision, Some(1));
    assert!(snapshot.issue.is_none());
}

#[test]
fn a_saved_choice_is_read_back_at_its_revision() {
    let (_temp, paths, facade) = facade();
    let chosen = UiSettings {
        show_debug_information: true,
        default_terminal: DefaultTerminal::Cmd,
        ..UiSettings::default()
    };
    facade.update_ui_settings(1, &chosen).expect("save");
    assert_eq!(facade.ui_settings().settings, chosen);
    assert_eq!(facade.ui_settings().revision, Some(2));
    drop(facade);
    let reopened = SettingsFacade::open(paths).expect("reopen");
    assert_eq!(reopened.ui_settings().settings, chosen);
    // A stale revision is refused and changes nothing.
    assert!(reopened.update_ui_settings(1, &UiSettings::default()).is_err());
    assert_eq!(reopened.ui_settings().settings, chosen);
}

#[test]
fn bad_values_are_refused_before_they_are_published() {
    let (_temp, _paths, facade) = facade();
    for (field, value) in [
        ("show_debug_information", json!("yes")),
        ("default_terminal", json!("bash")),
        ("background_motion", json!("fast")),
    ] {
        assert!(
            facade
                .update(ConfigModule::Ui, 1, |values| {
                    values.insert(field.into(), value);
                })
                .is_err(),
            "{field} must be refused"
        );
        assert_eq!(facade.snapshot(ConfigModule::Ui).expect("unchanged").document.revision, 1);
    }
}

/// The sea behind an empty conversation follows Windows' animation effects unless 开启 or 关闭 is
/// chosen; following the system is the default and is not written to the file.
#[test]
fn the_background_motion_is_saved_read_back_and_left_out_while_it_follows_the_system() {
    fn motion(facade: &SettingsFacade) -> serde_json::Value {
        serde_json::to_value(facade.ui_settings().settings).expect("JSON")["background_motion"]
            .clone()
    }
    fn chosen(motion: &str) -> UiSettings {
        serde_json::from_value(json!({
            "show_debug_information": false,
            "default_terminal": "power_shell",
            "background_motion": motion,
        }))
        .expect("a choice the page offers")
    }
    let (_temp, paths, facade) = facade();
    assert_eq!(motion(&facade), serde_json::Value::Null);
    facade.update_ui_settings(1, &chosen("on")).expect("save");
    assert_eq!(motion(&facade), json!("on"));
    facade.update_ui_settings(2, &chosen("off")).expect("save");
    drop(facade);
    let reopened = SettingsFacade::open(paths.clone()).expect("reopen");
    assert_eq!(motion(&reopened), json!("off"));
    reopened.update_ui_settings(3, &chosen("system")).expect("save");
    assert_eq!(motion(&reopened), serde_json::Value::Null);
    let written = std::fs::read_to_string(paths.config_path(ConfigModule::Ui)).expect("file");
    assert!(!written.contains("background_motion"), "{written}");
}

#[test]
fn a_damaged_file_at_startup_gives_back_the_last_valid_choice() {
    // Like other settings that cannot widen what runs, a damaged file is backed up into the
    // history and the last valid choice is served and written back.
    let (_temp, paths, facade) = facade();
    let chosen = UiSettings {
        show_debug_information: true,
        default_terminal: DefaultTerminal::Cmd,
        ..UiSettings::default()
    };
    facade.update_ui_settings(1, &chosen).expect("save");
    drop(facade);
    std::fs::write(paths.config_path(ConfigModule::Ui), DAMAGED).expect("external edit");
    let reopened = SettingsFacade::open(paths).expect("reopen");
    let snapshot = reopened.ui_settings();
    assert_eq!(snapshot.settings, chosen);
    assert!(snapshot.issue.is_none());
}

#[test]
fn a_damaged_file_with_no_valid_choice_before_it_shows_the_defaults_and_is_kept() {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let damaged = DAMAGED;
    std::fs::create_dir_all(paths.config_path(ConfigModule::Ui).parent().expect("folder"))
        .expect("folder");
    std::fs::write(paths.config_path(ConfigModule::Ui), damaged).expect("external edit");
    let facade = SettingsFacade::open(paths.clone()).expect("other settings stay available");
    let snapshot = facade.ui_settings();
    assert_eq!(snapshot.settings, UiSettings::default());
    assert_eq!(snapshot.revision, None);
    assert!(snapshot.issue.is_some());
    assert_eq!(std::fs::read(paths.config_path(ConfigModule::Ui)).expect("kept"), damaged);
}

#[test]
fn fields_this_version_does_not_use_are_kept_and_do_not_disable_the_page() {
    // A theme carried over by the V1 import, or a value another version wrote.
    let (_temp, paths, facade) = facade();
    facade
        .update(ConfigModule::Ui, 1, |values| {
            values.insert("theme".into(), json!("dark"));
            values.insert("ui_scale".into(), json!(1.0));
        })
        .expect("other fields are accepted");
    assert!(facade.ui_settings().issue.is_none());
    facade
        .update_ui_settings(
            2,
            &UiSettings { show_debug_information: true, ..UiSettings::default() },
        )
        .expect("save");
    let written = std::fs::read_to_string(paths.config_path(ConfigModule::Ui)).expect("file");
    assert!(written.contains("theme = \"dark\"") && written.contains("ui_scale"), "{written}");
    assert!(facade.ui_settings().settings.show_debug_information);
}
