//! A plugin in development: used from the folder it is written in, trusted once, its Skills,
//! commands and panel reloading by themselves while anything that runs needs trust again.
use super::support::{Fixture, plugin, write};
use serde_json::json;

/// A folder as Aura records it: its full path, in its ordinary spelling.
fn shown(folder: &std::path::Path) -> String {
    aura_domain_common::display_path(&std::fs::canonicalize(folder).expect("folder"))
}

fn install(f: &Fixture, folder: &std::path::Path) -> anyhow::Result<serde_json::Value> {
    f.dispatch(
        "agent.plugins.add_development",
        json!({"path": folder.display().to_string(), "expected_revision": f.revision()}),
    )
}

#[test]
fn a_folder_is_installed_in_development_where_it_is_and_not_trusted() {
    let f = Fixture::new();
    let folder = plugin(&f.source(), "review-kit", "1.0.0");
    let view = f.runtime.install_development(&folder).expect("installed");
    assert_eq!(view.name, "review-kit");
    assert_eq!(view.state, aura_plugins::PluginState::NotTrusted);
    assert_eq!(view.development.as_deref(), Some(shown(&folder).as_str()));
    assert_eq!(view.path, shown(&folder), "used where it is");
    assert!(!f.library().join("review-kit").exists(), "never copied");
    let saved = f.saved("review-kit").expect("saved");
    assert_eq!(saved.development.expect("in development").folder, shown(&folder));
    assert!(saved.trusted_fingerprint.is_none() && saved.enabled);
    assert!(f.runtime.ready().is_empty(), "nothing is offered before the user trusts it");
}

#[test]
fn once_trusted_its_skills_and_commands_reload_without_trusting_again() {
    let f = Fixture::new();
    let folder = plugin(&f.source(), "review-kit", "1.0.0");
    install(&f, &folder).expect("installed from the plugin page");
    f.trust("review-kit", false).expect("trusted");
    assert_eq!(f.view("review-kit")["state"], "ready");
    write(
        &folder.join("skills/guide/SKILL.md"),
        "---\nname: guide\ndescription: Guide, rewritten\n---\n\nRead the diff twice.\n",
    );
    write(&folder.join("commands/review/template.md"), "请仔细审查 $ARGUMENTS 的改动。\n");
    write(&folder.join("panel/index.html"), "<!doctype html><title>x</title>");
    f.runtime.changed();
    assert_eq!(f.view("review-kit")["state"], "ready", "text and pages reload by themselves");
    let ready = f.runtime.ready();
    let plugin = ready.iter().find(|p| p.package.manifest.name == "review-kit").expect("ready");
    let skill = plugin.package.skills[0].scanned.as_ref().expect("a Skill");
    assert_eq!(skill.frontmatter.description, "Guide, rewritten");
    assert_eq!(
        plugin.setting.trusted_parts.skills.get("guide"),
        Some(&skill.fingerprint),
        "offered at what it is now"
    );
    let command = &plugin.package.commands[0].command;
    assert_eq!(plugin.setting.trusted_parts.commands.get("review"), command.revision.as_ref());
}

#[test]
fn a_changed_hook_or_manifest_needs_trust_again() {
    let f = Fixture::new();
    let folder = plugin(&f.source(), "review-kit", "1.0.0");
    f.runtime.install_development(&folder).expect("installed");
    f.trust("review-kit", false).expect("trusted");
    write(&folder.join("hooks/check/check.ps1"), "'{\"decision\":\"block\"}'\n");
    f.runtime.changed();
    assert_eq!(f.view("review-kit")["state"], "content_changed");
    assert!(f.runtime.ready().is_empty(), "nothing runs until it is trusted again");
    f.trust("review-kit", false).expect("trusted again");
    assert_eq!(f.view("review-kit")["state"], "ready");
    let manifest = std::fs::read_to_string(folder.join("aura-plugin.toml")).expect("manifest");
    write(&folder.join("aura-plugin.toml"), &manifest.replace("1.0.0", "1.0.1"));
    f.runtime.changed();
    assert_eq!(f.view("review-kit")["state"], "content_changed");
}

#[test]
fn the_same_folder_again_changes_nothing_and_a_taken_name_is_refused() {
    let f = Fixture::new();
    let folder = plugin(&f.source(), "review-kit", "1.0.0");
    f.runtime.install_development(&folder).expect("installed");
    let revision = f.revision();
    f.runtime.install_development(&folder).expect("installed again");
    assert_eq!(f.revision(), revision, "nothing was written");
    let other = plugin(&f.root.path().join("elsewhere"), "review-kit", "1.0.0");
    let refused = f.runtime.install_development(&other).expect_err("taken").to_string();
    assert!(refused.starts_with("plugins.name_taken"), "{refused}");
    assert!(refused.contains(&shown(&folder)), "names the folder: {refused}");
    let refused = f.import(&other).expect_err("an import of the same name").to_string();
    assert!(refused.starts_with("plugins.name_taken"), "{refused}");
    let imported = plugin(&f.root.path().join("imported"), "other-kit", "1.0.0");
    f.import(&imported).expect("imported");
    let copy = plugin(&f.root.path().join("copy"), "other-kit", "1.0.0");
    let refused = f.runtime.install_development(&copy).expect_err("installed").to_string();
    assert!(refused.starts_with("plugins.name_taken"), "{refused}");
}

#[test]
fn uninstalling_a_plugin_in_development_leaves_its_folder_alone() {
    let f = Fixture::new();
    let folder = plugin(&f.source(), "review-kit", "1.0.0");
    f.runtime.install_development(&folder).expect("installed");
    f.trust("review-kit", false).expect("trusted");
    f.change("agent.plugins.uninstall", "review-kit").expect("uninstalled");
    assert!(f.saved("review-kit").is_none() && !f.listed("review-kit"));
    assert!(folder.join("aura-plugin.toml").is_file(), "the user's folder stays");
    assert!(folder.join("skills/guide/SKILL.md").is_file());
}

#[test]
fn a_renamed_plugin_and_a_folder_that_is_not_one_say_why() {
    let f = Fixture::new();
    let folder = plugin(&f.source(), "review-kit", "1.0.0");
    f.runtime.install_development(&folder).expect("installed");
    let manifest = std::fs::read_to_string(folder.join("aura-plugin.toml")).expect("manifest");
    write(&folder.join("aura-plugin.toml"), &manifest.replace("review-kit", "renamed-kit"));
    f.runtime.changed();
    let view = f.view("review-kit");
    assert_eq!(view["state"], "invalid");
    let problems = view["problems"].to_string();
    assert!(problems.contains("renamed-kit") && problems.contains("review-kit"), "{problems}");
    let empty = f.root.path().join("empty");
    std::fs::create_dir_all(&empty).expect("folder");
    let refused = f.runtime.install_development(&empty).expect_err("not a plugin").to_string();
    assert!(
        refused.starts_with("plugins.invalid") && refused.contains("aura-plugin.toml"),
        "{refused}"
    );
}
