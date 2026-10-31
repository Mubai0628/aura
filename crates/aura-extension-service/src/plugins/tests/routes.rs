//! What an installed plugin is, what trust covers, and the switch.
use super::support::{Fixture, plugin, write};
use serde_json::json;

#[test]
fn an_imported_plugin_is_switched_on_and_not_trusted() {
    let f = Fixture::new();
    let source = plugin(&f.source(), "review-kit", "1.0.0");
    let listed = f.import(&source).expect("imported");
    let view = &listed["plugins"][0];
    assert_eq!(view["name"], "review-kit");
    assert_eq!(view["version"], "1.0.0");
    assert_eq!(view["layout"], "aura");
    assert_eq!(view["state"], "not_trusted");
    assert_eq!(view["enabled"], true);
    assert_eq!(view["counts"], json!({"skills": 1, "commands": 1, "servers": 0, "hooks": 1}));
    assert_eq!(view["limits"], json!({"memory_mb": 2048, "processes": 64}));
    assert!(f.library().join("review-kit/aura-plugin.toml").is_file(), "copied into Aura's own");
    assert!(source.join("aura-plugin.toml").is_file(), "the source is unchanged");
    let saved = f.saved("review-kit").expect("saved");
    assert_eq!(saved.imported_from.as_deref(), Some(source.display().to_string().as_str()));
    assert!(saved.trusted_fingerprint.is_none());
}

#[test]
fn trust_is_given_only_at_the_shown_content_and_records_each_part() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    let preview =
        f.dispatch("agent.plugins.preview", json!({"name": "review-kit"})).expect("preview");
    assert_eq!(preview["skills"][0]["name"], "guide");
    assert_eq!(preview["commands"][0]["name"], "review");
    assert_eq!(preview["hooks"][0]["definition"]["name"], "check");
    assert!(preview["files"].as_array().expect("files").len() >= 6);
    // The plugin changes after the user looked at it.
    write(&f.library().join("review-kit/skills/guide/notes.md"), "added later");
    let refused = f.dispatch(
        "agent.plugins.trust",
        json!({"name": "review-kit", "expected_fingerprint": preview["fingerprint"], "expected_revision": f.revision()}),
    );
    assert!(refused.expect_err("stale").to_string().starts_with("plugins.content_changed"));
    let trusted = f.trust("review-kit", false).expect("trusted");
    assert_eq!(trusted["plugins"][0]["state"], "ready");
    let saved = f.saved("review-kit").expect("saved");
    let parts = &saved.trusted_parts;
    assert_eq!(parts.skills.keys().collect::<Vec<_>>(), ["guide"]);
    assert_eq!(parts.commands.keys().collect::<Vec<_>>(), ["review"]);
    assert_eq!(parts.hooks.keys().collect::<Vec<_>>(), ["check"]);
    assert!(parts.servers.is_empty());
}

#[test]
fn a_change_after_trust_withdraws_the_whole_plugin_until_it_is_trusted_again() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    f.trust("review-kit", false).expect("trusted");
    write(&f.library().join("review-kit/commands/review/template.md"), "做别的事");
    assert_eq!(f.view("review-kit")["state"], "content_changed");
    f.trust("review-kit", false).expect("trusted again");
    assert_eq!(f.view("review-kit")["state"], "ready");
}

#[test]
fn untrust_and_the_switch_change_only_the_choices() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    f.trust("review-kit", false).expect("trusted");
    let off = f
        .dispatch(
            "agent.plugins.set_enabled",
            json!({"name": "review-kit", "enabled": false, "expected_revision": f.revision()}),
        )
        .expect("switched off");
    assert_eq!(off["plugins"][0]["state"], "disabled");
    let untrusted = f.change("agent.plugins.untrust", "review-kit").expect("untrusted");
    assert_eq!(untrusted["plugins"][0]["state"], "not_trusted");
    let saved = f.saved("review-kit").expect("saved");
    assert!(saved.trusted_fingerprint.is_none() && saved.trusted_parts.skills.is_empty());
    assert!(f.library().join("review-kit").is_dir(), "the files stay");
}

#[test]
fn a_change_against_an_old_revision_is_refused() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    let stale = f.revision();
    f.dispatch(
        "agent.plugins.set_enabled",
        json!({"name": "review-kit", "enabled": false, "expected_revision": stale}),
    )
    .expect("first change");
    let refused = f.dispatch(
        "agent.plugins.set_enabled",
        json!({"name": "review-kit", "enabled": true, "expected_revision": stale}),
    );
    assert!(refused.expect_err("stale").to_string().starts_with("plugins.revision_changed"));
}

#[test]
fn a_folder_that_is_not_a_plugin_is_listed_with_why() {
    let f = Fixture::new();
    write(&f.library().join("stray/readme.txt"), "not a plugin");
    let view = f.view("stray");
    assert_eq!(view["state"], "invalid");
    assert!(view["problems"].to_string().contains("不是插件"), "{view}");
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    std::fs::rename(f.library().join("review-kit"), f.library().join("renamed")).expect("rename");
    let renamed = f.view("renamed");
    assert!(renamed["problems"].to_string().contains("文件夹名 renamed 与插件名 review-kit 不同"));
    assert_eq!(f.view("review-kit")["state"], "unavailable", "saved, folder gone");
}
