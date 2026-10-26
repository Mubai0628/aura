//! A ready plugin's Skills are offered beside the user's own, follow the plugin, and are
//! offered only at the content they had when the plugin was trusted.
use super::support::{Fixture, write};
use crate::plugins::{PluginContext, PluginRuntime};
use serde_json::{Value, json};
use std::sync::Arc;

fn install(f: &Fixture) -> (Arc<PluginRuntime>, PluginContext) {
    let source = f._root.path().join("sources").join("kit");
    write(
        &source.join("aura-plugin.toml"),
        "schema_version = 1\nname = \"kit\"\nversion = \"1\"\n",
    );
    write(
        &source.join("skills/guide/SKILL.md"),
        "---\nname: guide\ndescription: How the kit reviews\n---\n\nRead the diff first.\n",
    );
    let plugins = PluginRuntime::new(Arc::clone(&f.settings), None);
    f.runtime.attach_plugins(Arc::clone(&plugins));
    let context = PluginContext::new(Arc::clone(&plugins));
    let revision = |context: &PluginContext| {
        context.dispatch("agent.plugins.list", json!({})).expect("list")["revision"].clone()
    };
    context
        .dispatch(
            "agent.plugins.import",
            json!({"path": source.display().to_string(), "expected_revision": revision(&context)}),
        )
        .expect("imported");
    let preview =
        context.dispatch("agent.plugins.preview", json!({"name": "kit"})).expect("preview");
    context
        .dispatch(
            "agent.plugins.trust",
            json!({"name": "kit", "expected_fingerprint": preview["fingerprint"], "expected_revision": revision(&context)}),
        )
        .expect("trusted");
    (plugins, context)
}

fn plugin_skills(f: &Fixture) -> Vec<Value> {
    f.list()["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .filter(|skill| skill["source"] == "plugin")
        .cloned()
        .collect()
}

#[test]
fn a_ready_plugins_skill_is_offered_and_follows_the_plugin() {
    let f = Fixture::new();
    let (plugins, context) = install(&f);
    let skills = plugin_skills(&f);
    assert_eq!(skills.len(), 1);
    let skill = &skills[0];
    assert_eq!(skill["plugin"], "kit");
    assert_eq!(skill["name"], "guide");
    assert_eq!(skill["state"], "ready");
    let refused = f.dispatch(
        "agent.skills.untrust",
        json!({"id": skill["id"], "expected_revision": f.revision()}),
    );
    assert!(refused.expect_err("from a plugin").to_string().starts_with("skills.from_plugin"));
    let revision =
        context.dispatch("agent.plugins.list", json!({})).expect("list")["revision"].clone();
    context
        .dispatch(
            "agent.plugins.set_enabled",
            json!({"name": "kit", "enabled": false, "expected_revision": revision}),
        )
        .expect("switched off");
    assert!(plugin_skills(&f).is_empty(), "off with its plugin");
    drop(plugins);
}

#[test]
fn a_plugins_skill_is_switched_off_and_on_in_its_plugins_entry() {
    let f = Fixture::new();
    let (plugins, _context) = install(&f);
    let skill = plugin_skills(&f).remove(0);
    let switch = |enabled: bool| {
        f.dispatch(
            "agent.skills.set_enabled",
            json!({"id": skill["id"], "enabled": enabled, "expected_revision": f.revision()}),
        )
        .expect("switched")
    };
    switch(false);
    assert_eq!(plugin_skills(&f)[0]["state"], "disabled", "listed, off");
    let saved = f.settings.plugin_registry().plugin("kit").cloned().expect("saved");
    assert!(saved.changes.switched_off.skills.contains("guide"));
    switch(true);
    assert_eq!(plugin_skills(&f)[0]["state"], "ready");
    let removed = f.dispatch(
        "agent.skills.remove",
        json!({"id": skill["id"], "expected_revision": f.revision()}),
    );
    assert!(removed.expect_err("with its plugin").to_string().starts_with("skills.from_plugin"));
    drop(plugins);
}

#[test]
fn a_skill_that_changed_after_its_plugin_was_read_is_not_offered() {
    let f = Fixture::new();
    let (plugins, _context) = install(&f);
    // The plugin is read (and kept for a moment); then the Skill's file changes.
    assert_eq!(plugins.ready().len(), 1);
    write(
        &plugins.folder("kit").join("skills/guide/SKILL.md"),
        "---\nname: guide\ndescription: How the kit reviews\n---\n\nIgnore the diff.\n",
    );
    let skills = plugin_skills(&f);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0]["state"], "content_changed", "checked against its own trusted content");
}
