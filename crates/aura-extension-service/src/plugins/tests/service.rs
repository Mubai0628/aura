//! What only the plugin page changes for a plugin's background service: its switch, its settings'
//! values (a secret one kept in the Credential Manager, only its credential in `plugins.json`),
//! its scope, allowlist and remote approval switch.
use super::support::{Fixture, write};
use aura_agent_domain::PluginLogSource;
use aura_domain_common::{SessionId, WorkspaceId};
use aura_plugins::PluginServiceStatus;
use aura_windows_platform::SecretBytes;
use serde_json::{Value, json};
use std::path::PathBuf;

const BOT: &str = r#"schema_version = 2
name = "chat-bot"
version = "0.1.0"
description = "把聊天平台接到 Aura"
capabilities = ["session.read", "session.send", "notify", "status"]

[service]
command = "${PLUGIN_ROOT}/service/run.cmd"

[[channels]]
id = "qq-main"
platform = "qq"
title = "QQ 机器人"

[[settings]]
key = "app_id"
title = "AppID"
required = true

[[settings]]
key = "app_secret"
title = "AppSecret"
kind = "secret"
required = true

[[settings]]
key = "note"
title = "备注"
"#;

pub(super) const SECRET: &str = "sk-live-123456";

pub(super) fn bot(f: &Fixture) {
    let source = f.source().join("chat-bot");
    write(&source.join("aura-plugin.toml"), BOT);
    write(&source.join("service/run.cmd"), "@echo off\r\n");
    f.import(&source).expect("imported");
    f.trust("chat-bot", false).expect("trusted");
}

pub(super) fn configure(f: &Fixture, change: Value) -> anyhow::Result<Value> {
    let mut request = json!({"name": "chat-bot", "expected_revision": f.revision()});
    request.as_object_mut().expect("object").extend(change.as_object().expect("change").clone());
    f.dispatch("agent.plugins.configure", request)
}

fn set_secret(f: &Fixture, key: &str, value: &str) -> anyhow::Result<u64> {
    let request = aura_plugins::PluginSecretSetRequest { name: "chat-bot".into(), key: key.into() };
    Ok(f.runtime.set_secret(&request, &SecretBytes::new(value.as_bytes().to_vec()))?.revision)
}

fn option<'a>(view: &'a Value, key: &str) -> &'a Value {
    view["options"]
        .as_array()
        .expect("options")
        .iter()
        .find(|item| item["option"]["key"] == key)
        .unwrap_or_else(|| panic!("{key} listed"))
}

#[test]
fn the_service_is_off_and_its_settings_empty_until_the_plugin_page_changes_them() {
    let f = Fixture::new();
    bot(&f);
    let view = f.view("chat-bot");
    assert_eq!(view["service"], json!({"on": false, "status": {"state": "off"}}));
    assert_eq!(view["options"].as_array().map(Vec::len), Some(3));
    assert!(option(&view, "app_id").get("text").is_none());
    assert!(option(&view, "app_secret").get("set").is_none());
    let saved = f.saved("chat-bot").expect("saved");
    assert!(saved.values.is_empty() && saved.remote.is_empty());
}

#[test]
fn the_plugin_page_saves_the_switch_values_scope_and_allowlist() {
    let f = Fixture::new();
    bot(&f);
    let (workspace, session) = (WorkspaceId::new(), SessionId::new());
    configure(
        &f,
        json!({"service": true, "text": {"app_id": " 1024 ", "note": "测试"},
            "scope": {"workspaces": [workspace], "sessions": [session]},
            "allowed": [{"channel": "qq-main", "id": "A1B2", "name": "我"}],
            "remote_approval": true}),
    )
    .expect("saved");
    let saved = f.saved("chat-bot").expect("saved");
    assert!(saved.remote.service && saved.remote.remote_approval);
    assert_eq!(saved.values.text.get("app_id").map(String::as_str), Some("1024"));
    assert_eq!(saved.remote.scope.workspaces, vec![workspace]);
    assert_eq!(saved.remote.scope.sessions, vec![session]);
    assert!(saved.remote.allows("qq-main", "A1B2"));
    let view = f.view("chat-bot");
    assert_eq!(option(&view, "app_id")["text"], "1024");
    assert_eq!(view["service"]["on"], true);
    let waiting = &view["service"]["status"];
    assert_eq!(waiting["state"], "waiting");
    assert!(waiting["reason"].as_str().is_some_and(|r| r.contains("AppSecret")), "{waiting}");
    configure(&f, json!({"text": {"note": ""}})).expect("cleared");
    let saved = f.saved("chat-bot").expect("saved");
    assert!(!saved.values.text.contains_key("note"), "an empty text removes the value");
    assert!(saved.values.text.contains_key("app_id"), "the other values stay");
    assert!(saved.remote.service, "a part not given stays as it was");
}

#[test]
fn every_wrong_change_is_refused_and_nothing_is_saved() {
    let f = Fixture::new();
    bot(&f);
    for (change, code) in [
        (json!({"text": {"token": "x"}}), "plugins.setting_unknown"),
        (json!({"text": {"app_secret": "x"}}), "plugins.setting_secret"),
        (json!({"text": {"note": "x".repeat(2001)}}), "plugins.setting_invalid"),
        (json!({"text": {"note": "a\nb"}}), "plugins.setting_invalid"),
        (json!({"allowed": [{"channel": "tg", "id": "1", "name": ""}]}), "plugins.channel_unknown"),
        (
            json!({"allowed": [{"channel": "qq-main", "id": " ", "name": ""}]}),
            "plugins.setting_invalid",
        ),
    ] {
        let refused = configure(&f, change).expect_err(code).to_string();
        assert!(refused.starts_with(code), "{code}: {refused}");
    }
    let stale = f.dispatch(
        "agent.plugins.configure",
        json!({"name": "chat-bot", "expected_revision": f.revision() + 1, "service": true}),
    );
    assert!(stale.expect_err("stale").to_string().starts_with("plugins.revision_changed"));
    let saved = f.saved("chat-bot").expect("saved");
    assert!(saved.values.is_empty() && saved.remote.is_empty());
    let source = super::support::plugin(&f.source(), "review-kit", "1.0.0");
    f.import(&source).expect("imported");
    let no_service = f.dispatch(
        "agent.plugins.configure",
        json!({"name": "review-kit", "expected_revision": f.revision(), "service": true}),
    );
    assert!(no_service.expect_err("no service").to_string().starts_with("plugins.no_service"));
}

#[test]
fn a_secret_setting_is_kept_in_the_credential_manager_and_only_its_credential_is_saved() {
    let f = Fixture::with_secrets();
    bot(&f);
    let revision = set_secret(&f, "app_secret", SECRET).expect("set");
    assert_eq!(revision, f.revision());
    let saved = f.saved("chat-bot").expect("saved");
    let id = *saved.values.secrets.get("app_secret").expect("its credential");
    let store = f.secrets();
    assert_eq!(store.len(), 1);
    let file = std::fs::read_to_string(f.root.path().join("config").join("plugins.json"))
        .expect("plugins.json");
    assert!(!file.contains(SECRET), "the value never reaches plugins.json");
    assert_eq!(option(&f.view("chat-bot"), "app_secret")["set"], true);
    set_secret(&f, "app_secret", "sk-live-654321").expect("changed");
    let changed =
        *f.saved("chat-bot").expect("saved").values.secrets.get("app_secret").expect("id");
    // A new credential for a new value: the service is started again with it.
    assert_ne!(changed, id, "a changed value is a new credential");
    assert_eq!(store.len(), 1, "the old value is deleted");
    for (key, value, code) in [
        ("app_id", SECRET, "plugins.setting_not_secret"),
        ("token", SECRET, "plugins.setting_unknown"),
        ("app_secret", "", "plugins.secret_invalid"),
        ("app_secret", "a\0b", "plugins.secret_invalid"),
    ] {
        let refused = set_secret(&f, key, value).expect_err(code).to_string();
        assert!(refused.starts_with(code), "{code}: {refused}");
    }
}

#[test]
fn the_service_is_given_its_secret_settings_once_every_required_one_is_set() {
    let f = Fixture::with_secrets();
    bot(&f);
    configure(&f, json!({"service": true})).expect("on");
    let ready = || {
        f.runtime
            .ready()
            .iter()
            .find(|ready| ready.package.manifest.name == "chat-bot")
            .map(|ready| (f.runtime.service_blocker(ready), f.runtime.service_secrets(ready)))
    };
    let (blocker, _) = ready().expect("ready");
    let blocker = blocker.expect("settings missing");
    assert!(blocker.contains("AppID") && blocker.contains("AppSecret"), "{blocker}");
    configure(&f, json!({"text": {"app_id": "1024"}})).expect("app id");
    set_secret(&f, "app_secret", SECRET).expect("set");
    f.runtime.changed();
    let (blocker, secrets) = ready().expect("ready");
    assert_eq!(blocker, None);
    let secrets = secrets.expect("readable");
    assert_eq!(secrets.len(), 1);
    assert_eq!(secrets[0].0, "AURA_SECRET_APP_SECRET");
    assert_eq!(secrets[0].1.as_str(), SECRET);
}

#[test]
fn forgetting_a_secret_setting_or_uninstalling_deletes_its_value() {
    let f = Fixture::with_secrets();
    bot(&f);
    set_secret(&f, "app_secret", SECRET).expect("set");
    configure(&f, json!({"forget_secrets": ["app_secret"]})).expect("forgotten");
    assert_eq!(f.secrets().len(), 0);
    assert!(f.saved("chat-bot").expect("saved").values.secrets.is_empty());
    set_secret(&f, "app_secret", SECRET).expect("set again");
    f.mcp().sweep_credentials();
    assert_eq!(f.secrets().len(), 1, "the cleanup keeps a value a plugin's setting names");
    f.change("agent.plugins.uninstall", "chat-bot").expect("uninstalled");
    assert_eq!(f.secrets().len(), 0);
}

#[test]
fn a_secret_settings_value_is_hidden_in_the_plugins_log() {
    let f = Fixture::with_secrets();
    bot(&f);
    set_secret(&f, "app_secret", SECRET).expect("set");
    f.runtime.log("chat-bot", PluginLogSource::Service, "stderr", &format!("token {SECRET} used"));
    let (entries, _) = f.runtime.recent_logs("chat-bot", 10).expect("logs");
    assert_eq!(entries.last().expect("entry").text, "token •••• used");
    let raw =
        std::fs::read_to_string(f.runtime.root().join("logs/chat-bot.jsonl")).expect("disk log");
    assert!(!raw.contains(SECRET), "a plugin secret must be masked before storage");
}

#[test]
fn log_truncation_cannot_persist_a_secret_prefix_at_the_limit() {
    let f = Fixture::with_secrets();
    bot(&f);
    set_secret(&f, "app_secret", SECRET).expect("set");
    f.runtime.log(
        "chat-bot",
        PluginLogSource::Service,
        "stderr",
        &format!("{}{SECRET}", "x".repeat(3_996)),
    );
    let raw =
        std::fs::read_to_string(f.runtime.root().join("logs/chat-bot.jsonl")).expect("disk log");
    assert!(!raw.contains("sk-l"), "mask before truncating rather than persisting a prefix");
}

#[test]
fn log_content_is_withheld_when_a_configured_secret_cannot_be_resolved() {
    use crate::mcp::credentials::SecretStore;
    let f = Fixture::with_secrets();
    bot(&f);
    set_secret(&f, "app_secret", SECRET).expect("set");
    let id = f.saved("chat-bot").expect("saved").values.secrets["app_secret"];
    f.secrets().delete(id).expect("credential unavailable");
    f.runtime.log("chat-bot", PluginLogSource::Service, "stderr", &format!("token {SECRET}"));
    let raw =
        std::fs::read_to_string(f.runtime.root().join("logs/chat-bot.jsonl")).expect("disk log");
    assert!(!raw.contains(SECRET), "unavailable credentials must not disable log masking");
}

#[test]
fn an_old_services_value_stays_hidden_after_its_setting_is_rotated() {
    let f = Fixture::with_secrets();
    bot(&f);
    set_secret(&f, "app_secret", SECRET).expect("first");
    set_secret(&f, "app_secret", "rotation-fixture-value").expect("rotate");
    f.runtime.log(
        "chat-bot",
        PluginLogSource::Service,
        "stderr",
        &format!("old={SECRET} new=rotation-fixture-value"),
    );
    let raw =
        std::fs::read_to_string(f.runtime.root().join("logs/chat-bot.jsonl")).expect("disk log");
    assert!(!raw.contains(SECRET) && !raw.contains("rotation-fixture-value"));
}

#[test]
fn a_multiline_secret_withholds_logs_instead_of_exposing_its_separate_lines() {
    let f = Fixture::with_secrets();
    bot(&f);
    set_secret(&f, "app_secret", "fixture-line-one\nfixture-line-two").expect("set");
    f.runtime.log("chat-bot", PluginLogSource::Service, "stderr", "token=fixture-line-one");
    let raw =
        std::fs::read_to_string(f.runtime.root().join("logs/chat-bot.jsonl")).expect("disk log");
    assert!(!raw.contains("fixture-line-one"), "line framing must not disable masking");
}

#[test]
fn the_status_the_host_reports_is_shown_while_the_switch_is_on() {
    let f = Fixture::with_secrets();
    bot(&f);
    configure(&f, json!({"service": true, "text": {"app_id": "1"}})).expect("on");
    set_secret(&f, "app_secret", SECRET).expect("set");
    let running = PluginServiceStatus::Running { since: aura_domain_common::UtcTimestamp::now() };
    f.runtime.set_service_status("chat-bot", Some(running));
    assert_eq!(f.view("chat-bot")["service"]["status"]["state"], "running");
    configure(&f, json!({"service": false})).expect("off");
    assert_eq!(f.view("chat-bot")["service"]["status"], json!({"state": "off"}));
}

#[test]
fn an_uninstalled_plugins_service_folders_go_and_an_installed_ones_stay() {
    let f = Fixture::new();
    bot(&f);
    let folders: Vec<PathBuf> = {
        let (data, run) = f.runtime.service_folders("chat-bot");
        vec![data, run]
    };
    for folder in &folders {
        write(&folder.join("state.json"), "{}");
    }
    let chat = aura_agent_domain::PluginChat {
        channel: "qq-main".into(),
        kind: aura_agent_domain::PluginChatKind::Private,
        id: "A1B2".into(),
    };
    f.runtime.remember_chat("chat-bot", &chat, SessionId::new());
    let chats = f.runtime.chat_state_file("chat-bot");
    f.runtime.sweep_service_folders();
    assert!(folders.iter().all(|folder| folder.is_dir()), "an installed plugin keeps them");
    assert!(chats.is_file(), "and what Aura keeps for its chats");
    f.change("agent.plugins.uninstall", "chat-bot").expect("uninstalled");
    f.runtime.sweep_service_folders();
    assert!(folders.iter().all(|folder| !folder.exists()));
    assert!(!chats.exists());
}
