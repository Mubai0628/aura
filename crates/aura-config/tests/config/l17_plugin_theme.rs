//! The theme chosen in 设置 → 环境: a plugin's name, or none for Aura's own. Saving without it
//! takes it away; a name no plugin could have is refused before it is published.
use aura_config::{AuraPaths, ConfigModule, SettingsFacade, UiSettings};
use serde_json::json;

fn paths(temp: &tempfile::TempDir) -> AuraPaths {
    AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"))
}

#[test]
fn a_chosen_plugin_theme_is_saved_read_back_and_taken_away() {
    let temp = tempfile::tempdir().expect("TEMP");
    let facade = SettingsFacade::open(paths(&temp)).expect("settings");
    assert_eq!(facade.ui_settings().settings.plugin_theme, None, "Aura's own by default");
    let chosen = UiSettings { plugin_theme: Some("night".into()), ..UiSettings::default() };
    facade.update_ui_settings(1, &chosen).expect("save");
    drop(facade);

    let reopened = SettingsFacade::open(paths(&temp)).expect("reopen");
    assert_eq!(reopened.ui_settings().settings, chosen);
    reopened.update_ui_settings(2, &UiSettings::default()).expect("back to Aura's own");
    assert_eq!(reopened.ui_settings().settings.plugin_theme, None);
    let written =
        std::fs::read_to_string(paths(&temp).config_path(ConfigModule::Ui)).expect("file");
    assert!(!written.contains("plugin_theme"), "{written}");
}

#[test]
fn a_name_no_plugin_could_have_is_refused() {
    let temp = tempfile::tempdir().expect("TEMP");
    let facade = SettingsFacade::open(paths(&temp)).expect("settings");
    for value in [json!("Night"), json!("../night"), json!(""), json!(3), json!(null)] {
        assert!(
            facade
                .update(ConfigModule::Ui, 1, |values| {
                    values.insert("plugin_theme".into(), value.clone());
                })
                .is_err(),
            "{value} must be refused"
        );
    }
    let bad = UiSettings { plugin_theme: Some("Night Sky".into()), ..UiSettings::default() };
    assert!(facade.update_ui_settings(1, &bad).is_err());
    assert_eq!(facade.snapshot(ConfigModule::Ui).expect("unchanged").document.revision, 1);
    assert!(facade.ui_settings().issue.is_none());
}
