//! A plugin's theme in Core: listed with its title, given to the window only while the plugin is
//! ready, and the choice of it taken away when the plugin is uninstalled.
use super::support::{Fixture, plugin, write};
use aura_config::UiSettings;
use serde_json::json;

const NIGHT: &str = "title = \"夜间\"\nscheme = \"dark\"\n[colors]\ncanvas = \"#1e1f1c\"\nink = \"#E6E6E6\"\n[radius]\nmd = 4\n";

fn themed(f: &Fixture, name: &str, theme: &str) -> std::path::PathBuf {
    let source = plugin(&f.source(), name, "1.0.0");
    write(&source.join("theme.toml"), theme);
    source
}

fn theme(f: &Fixture, name: &str) -> anyhow::Result<serde_json::Value> {
    f.dispatch("agent.plugins.theme", json!({"name": name}))
}

fn choose(f: &Fixture, name: Option<&str>) {
    let snapshot = f.settings.ui_settings();
    let settings = UiSettings { plugin_theme: name.map(str::to_owned), ..snapshot.settings };
    f.settings.update_ui_settings(snapshot.revision.expect("revision"), &settings).expect("chosen");
}

#[test]
fn a_ready_plugins_theme_is_listed_and_given_to_the_window() {
    let f = Fixture::new();
    f.import(&themed(&f, "night", NIGHT)).expect("imported");
    let view = f.view("night");
    assert_eq!(view["theme"], "夜间", "listed with its title");
    assert_eq!(view["counts"]["themes"], 1);
    let preview = f.dispatch("agent.plugins.preview", json!({"name": "night"})).expect("preview");
    assert_eq!(preview["theme"], json!({"title": "夜间", "problems": []}));

    // Not trusted yet: nothing it brings is given.
    let refused = theme(&f, "night").expect_err("not trusted").to_string();
    assert!(refused.starts_with("plugins.theme_unavailable"), "{refused}");

    f.trust("night", false).expect("trusted");
    let given = theme(&f, "night").expect("given");
    assert_eq!(given["title"], "夜间");
    assert_eq!(given["scheme"], "dark");
    assert_eq!(given["colors"], json!({"canvas": "#1e1f1c", "ink": "#e6e6e6"}));
    assert_eq!(given["radius"], json!({"md": 4}));

    f.dispatch(
        "agent.plugins.set_enabled",
        json!({"name": "night", "enabled": false, "expected_revision": f.revision()}),
    )
    .expect("switched off");
    let refused = theme(&f, "night").expect_err("switched off").to_string();
    assert!(refused.starts_with("plugins.theme_unavailable"), "{refused}");
}

#[test]
fn a_broken_or_missing_theme_says_why() {
    let f = Fixture::new();
    f.import(&themed(&f, "night", "title = \"坏\"\n[colors]\nink = \"red\"\n")).expect("imported");
    f.trust("night", false).expect("the rest of the plugin is trusted");
    assert_eq!(f.view("night")["state"], "ready");
    assert!(f.view("night").get("theme").is_none(), "a broken theme is not listed");
    let preview = f.dispatch("agent.plugins.preview", json!({"name": "night"})).expect("preview");
    assert!(preview["theme"]["problems"][0].as_str().expect("why").contains("ink"), "{preview}");
    let refused = theme(&f, "night").expect_err("broken").to_string();
    assert!(refused.starts_with("plugins.theme_invalid") && refused.contains("ink"), "{refused}");

    f.import(&plugin(&f.source(), "plain", "1.0.0")).expect("imported");
    f.trust("plain", false).expect("trusted");
    let refused = theme(&f, "plain").expect_err("no theme").to_string();
    assert!(refused.starts_with("plugins.no_theme"), "{refused}");
    let refused = theme(&f, "missing").expect_err("no plugin").to_string();
    assert!(refused.starts_with("plugins.not_found"), "{refused}");
}

#[test]
fn uninstalling_the_plugin_whose_theme_is_chosen_takes_the_choice_away() {
    let f = Fixture::new();
    for name in ["night", "day"] {
        f.import(&themed(&f, name, NIGHT)).expect("imported");
        f.trust(name, false).expect("trusted");
    }
    choose(&f, Some("night"));
    let terminal = f.settings.ui_settings().settings.default_terminal;
    f.change("agent.plugins.uninstall", "day").expect("uninstalled");
    assert_eq!(f.settings.ui_settings().settings.plugin_theme.as_deref(), Some("night"));
    f.change("agent.plugins.uninstall", "night").expect("uninstalled");
    let settings = f.settings.ui_settings().settings;
    assert_eq!(settings.plugin_theme, None, "back to Aura's own");
    assert_eq!(settings.default_terminal, terminal, "the other choices stay");
}

#[test]
fn the_theme_of_a_plugin_in_development_changes_without_trusting_again() {
    let f = Fixture::new();
    let folder = themed(&f, "night", NIGHT);
    f.dispatch(
        "agent.plugins.add_development",
        json!({"path": folder.display().to_string(), "expected_revision": f.revision()}),
    )
    .expect("installed in development");
    f.trust("night", false).expect("trusted");
    write(&folder.join("theme.toml"), &NIGHT.replace("#1e1f1c", "#101010"));
    f.runtime.changed();
    assert_eq!(f.view("night")["state"], "ready");
    assert_eq!(theme(&f, "night").expect("given")["colors"]["canvas"], "#101010");
}
