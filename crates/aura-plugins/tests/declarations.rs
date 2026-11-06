//! A version 2 manifest: what a plugin may do with the conversations it is given, its panel, its
//! background service and its own settings, each checked with every reason.
mod support;
use aura_plugins::{PluginCapability, PluginOptionKind};
use support::*;

const PANEL_PLUGIN: &str = r#"schema_version = 2
name = "usage-panel"
version = "0.1.0"
description = "本会话用量"
capabilities = ["session.read", "session.events", "notify"]

[panel]
title = "本会话用量"
network = ["https://api.example.com"]

[service]
command = "${PLUGIN_ROOT}/service/run.cmd"
arguments = ["--quiet"]
network = ["api.sgroup.qq.com", "wss://api.sgroup.qq.com/websocket"]

[[settings]]
key = "app_id"
title = "AppID"

[[settings]]
key = "app_secret"
title = "AppSecret"
kind = "secret"
required = true
"#;

fn panel_plugin(root: &std::path::Path, manifest: &str) {
    write(root, "aura-plugin.toml", manifest);
    write(
        root,
        "panel/index.html",
        "<!doctype html><title>用量</title><script src=\"main.js\"></script>",
    );
    write(root, "panel/main.js", "auraPanel.state();\n");
    write(root, "service/run.cmd", "@echo off\r\n");
}

#[test]
fn a_version_2_manifest_declares_capabilities_a_panel_a_service_and_settings() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("usage-panel");
    panel_plugin(&root, PANEL_PLUGIN);
    let package = scan(&root).expect("a plugin with only a panel and a service");
    let declared = &package.manifest.declared;
    assert_eq!(
        declared.capabilities,
        vec![
            PluginCapability::SessionRead,
            PluginCapability::SessionEvents,
            PluginCapability::Notify
        ]
    );
    let panel = declared.panel.as_ref().expect("panel");
    assert_eq!(panel.title, "本会话用量");
    assert_eq!(panel.entry, "panel/index.html", "the default entry page");
    assert_eq!(panel.network, vec!["https://api.example.com".to_owned()]);
    let service = declared.service.as_ref().expect("service");
    assert_eq!(service.command, format!("{}\\service\\run.cmd", root.display()));
    assert_eq!(service.arguments, vec!["--quiet".to_owned()]);
    assert_eq!(service.network.len(), 2);
    assert_eq!(declared.options.len(), 2);
    assert_eq!(declared.options[0].kind, PluginOptionKind::Text);
    assert!(!declared.options[0].required);
    assert_eq!(declared.options[1].kind, PluginOptionKind::Secret);
    assert!(declared.options[1].required);
    assert!(package.panel_problems.is_empty(), "{:?}", package.panel_problems);
    let counts = package.counts();
    assert_eq!((counts.panels, counts.services), (1, 1));
}

#[test]
fn version_1_cannot_declare_the_new_parts_and_an_unknown_version_names_both() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("usage-panel");
    panel_plugin(&root, &PANEL_PLUGIN.replace("schema_version = 2", "schema_version = 1"));
    let version_1 = problems(scan(&root));
    assert!(version_1.contains("schema_version = 2"), "{version_1}");
    panel_plugin(&root, &PANEL_PLUGIN.replace("schema_version = 2", "schema_version = 3"));
    let version_3 = problems(scan(&root));
    assert!(version_3.contains("只认 1 和 2"), "{version_3}");
}

#[test]
fn every_wrong_declaration_is_named() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("usage-panel");
    let manifest = PANEL_PLUGIN
        .replace("\"notify\"", "\"notify\", \"files.write\"")
        .replace("[panel]\n", "[panel]\nentry = \"../index.html\"\n")
        .replace("https://api.example.com", "http://api.example.com")
        .replace("key = \"app_secret\"", "key = \"app_id\"");
    panel_plugin(&root, &manifest);
    let named = problems(scan(&root));
    for expected in ["files.write", "panel.entry", "http://api.example.com", "app_id"] {
        assert!(named.contains(expected), "{expected}: {named}");
    }
    assert!(named.contains("session.read"), "the known names are listed: {named}");
}

#[test]
fn a_missing_entry_page_is_a_problem_of_the_panel_only() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("usage-panel");
    panel_plugin(&root, PANEL_PLUGIN);
    std::fs::remove_file(root.join("panel\\index.html")).expect("removed");
    let package = scan(&root).expect("still a plugin");
    assert_eq!(package.panel_problems.len(), 1, "{:?}", package.panel_problems);
    assert!(package.panel_problems[0].contains("panel/index.html"));
    assert_eq!(package.counts().panels, 0, "a panel with a problem is not offered");
}

#[test]
fn a_plugin_development_fingerprint_leaves_out_what_reloads_by_itself() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("usage-panel");
    panel_plugin(&root, PANEL_PLUGIN);
    write(&root, "skills/review-guide/SKILL.md", SKILL);
    write(
        &root,
        "commands/review/command.toml",
        "name = \"review\"\ndescription = \"审查\"\nversion = 1\n",
    );
    write(&root, "commands/review/template.md", "请审查 $ARGUMENTS。\n");
    let first = scan(&root).expect("plugin");
    for (path, content) in [
        ("skills/review-guide/SKILL.md", SKILL.replace("Read the diff first.", "Read it twice.")),
        ("commands/review/template.md", "请仔细审查 $ARGUMENTS。\n".to_owned()),
        ("panel/main.js", "auraPanel.state(); // changed\n".to_owned()),
        ("theme.toml", "[colors]\naccent = \"#3366ff\"\n".to_owned()),
    ] {
        write(&root, path, &content);
        let now = scan(&root).expect("plugin");
        assert_ne!(now.fingerprint, first.fingerprint, "{path} is part of the plugin");
        assert_eq!(
            now.development_fingerprint, first.development_fingerprint,
            "{path} reloads without being trusted again"
        );
    }
    let reloaded = scan(&root).expect("plugin");
    for (path, content) in [
        ("aura-plugin.toml", PANEL_PLUGIN.replace("0.1.0", "0.1.1")),
        ("service/run.cmd", "@echo on\r\n".to_owned()),
        ("hooks/check/hook.toml", HOOK.to_owned()),
    ] {
        write(&root, path, &content);
        let now = scan(&root).expect("plugin");
        assert_ne!(
            now.development_fingerprint, reloaded.development_fingerprint,
            "{path} needs the plugin to be trusted again"
        );
    }
}
