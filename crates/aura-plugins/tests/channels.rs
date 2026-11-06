//! A plugin's channels: the chat platform accounts its background service connects, declared in
//! the manifest and shown when the plugin is reviewed; and what only the plugin page changes for
//! the service and its channels, kept in `plugins.json`.
mod support;
use aura_domain_common::{CredentialId, SessionId, WorkspaceId};
use aura_plugins::{
    PluginAllowedUser, PluginChannel, PluginPlatform, PluginRemote, PluginScope, PluginSecret,
    PluginSetting, PluginValues, check_settings,
};
use std::collections::BTreeMap;
use support::*;

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

[[channels]]
id = "tg"
platform = "telegram"
title = "Telegram"
"#;

fn bot(root: &std::path::Path, manifest: &str) {
    write(root, "aura-plugin.toml", manifest);
    write(root, "service/run.cmd", "@echo off\r\n");
}

#[test]
fn channels_are_declared_with_a_platform_and_a_title() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("chat-bot");
    bot(&root, BOT);
    let package = scan(&root).expect("a plugin with a service and two channels");
    assert_eq!(
        package.manifest.declared.channels,
        vec![
            PluginChannel {
                id: "qq-main".into(),
                platform: PluginPlatform::Qq,
                title: "QQ 机器人".into()
            },
            PluginChannel {
                id: "tg".into(),
                platform: PluginPlatform::Telegram,
                title: "Telegram".into()
            },
        ]
    );
    assert_eq!(package.counts().channels, 2);
}

#[test]
fn every_platform_name_is_known_by_its_own_spelling() {
    let names: Vec<String> = PluginPlatform::ALL
        .iter()
        .map(|platform| serde_json::to_value(platform).expect("name"))
        .map(|value| value.as_str().expect("text").to_owned())
        .collect();
    assert_eq!(
        names,
        [
            "qq", "feishu", "wechat", "wecom", "telegram", "dingtalk", "slack", "discord",
            "onebot", "satori", "other"
        ]
    );
    for platform in PluginPlatform::ALL {
        assert_eq!(PluginPlatform::parse(platform.name()), Some(platform));
    }
}

#[test]
fn every_wrong_channel_is_named() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("chat-bot");
    let manifest = BOT
        .replace("id = \"qq-main\"", "id = \"QQ Main\"")
        .replace("platform = \"telegram\"", "platform = \"icq\"")
        .replace("title = \"Telegram\"", "title = \"\"")
        + "\n[[channels]]\nid = \"tg\"\nplatform = \"other\"\ntitle = \"又一个\"\n";
    bot(&root, &manifest);
    let named = problems(scan(&root));
    for expected in ["QQ Main", "icq", "qq、feishu", "tg 出现了两次", "title"] {
        assert!(named.contains(expected), "{expected}: {named}");
    }
}

#[test]
fn channels_need_a_background_service_and_version_2() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("chat-bot");
    let without_service =
        BOT.replace("[service]\ncommand = \"${PLUGIN_ROOT}/service/run.cmd\"\n", "");
    bot(&root, &without_service);
    let named = problems(scan(&root));
    assert!(named.contains("[service]"), "{named}");
    let version_1 = BOT
        .replace("schema_version = 2", "schema_version = 1")
        .replace(
            "capabilities = [\"session.read\", \"session.send\", \"notify\", \"status\"]\n",
            "",
        )
        .replace("[service]\ncommand = \"${PLUGIN_ROOT}/service/run.cmd\"\n", "");
    bot(&root, &version_1);
    let named = problems(scan(&root));
    assert!(named.contains("channels") && named.contains("schema_version = 2"), "{named}");
}

#[test]
fn at_most_ten_channels() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("chat-bot");
    let mut manifest = BOT.to_owned();
    for index in 0..9 {
        manifest.push_str(&format!(
            "\n[[channels]]\nid = \"more-{index}\"\nplatform = \"other\"\ntitle = \"第 {index} 个\"\n"
        ));
    }
    bot(&root, &manifest);
    let named = problems(scan(&root));
    assert!(named.contains("channels 最多 10 个"), "{named}");
}

#[test]
fn an_untouched_plugin_writes_no_values_or_remote_choices() {
    let setting = PluginSetting::imported("chat-bot", None);
    let written = serde_json::to_value(&setting).expect("json");
    assert!(written.get("values").is_none() && written.get("remote").is_none(), "{written}");
    let read: PluginSetting =
        serde_json::from_value(serde_json::json!({"name": "chat-bot", "enabled": true}))
            .expect("an entry from an earlier version");
    assert_eq!(read.values, PluginValues::default());
    assert_eq!(read.remote, PluginRemote::default());
    assert!(
        !read.remote.service && !read.remote.remote_approval,
        "off unless the user turns it on"
    );
    assert!(read.remote.scope.workspaces.is_empty() && read.remote.scope.sessions.is_empty());
    assert!(read.remote.allowed.is_empty());
}

fn configured() -> PluginSetting {
    let mut setting = PluginSetting::imported("chat-bot", None);
    setting.values.text.insert("app_id".into(), "1024".into());
    setting.values.secrets.insert("app_secret".into(), CredentialId::default());
    setting.remote = PluginRemote {
        service: true,
        scope: PluginScope {
            workspaces: vec![WorkspaceId::new()],
            sessions: vec![SessionId::new()],
        },
        allowed: vec![PluginAllowedUser {
            channel: "qq-main".into(),
            id: "A1B2C3".into(),
            name: "我".into(),
        }],
        remote_approval: true,
    };
    setting
}

#[test]
fn values_and_remote_choices_are_kept_as_written() {
    let setting = configured();
    check_settings(std::slice::from_ref(&setting)).expect("a configured plugin");
    let read: PluginSetting =
        serde_json::from_value(serde_json::to_value(&setting).expect("json")).expect("read");
    assert_eq!(read, setting);
}

#[test]
fn every_wrong_value_or_remote_choice_is_refused_with_its_reason() {
    type Change = Box<dyn Fn(&mut PluginSetting)>;
    let long = |count: usize| "x".repeat(count);
    let cases: Vec<(&str, Change)> = vec![
        (
            "App-ID",
            Box::new(|s| {
                s.values.text.insert("App-ID".into(), "1".into());
            }),
        ),
        (
            "一行",
            Box::new(|s| {
                s.values.text.insert("note".into(), "a\nb".into());
            }),
        ),
        (
            "2000",
            Box::new(move |s| {
                s.values.text.insert("note".into(), long(2001));
            }),
        ),
        (
            "app_id",
            Box::new(|s| {
                s.values.secrets.insert("app_id".into(), CredentialId::default());
            }),
        ),
        (
            "用了两次",
            Box::new(|s| {
                let id = *s.values.secrets.get("app_secret").expect("secret");
                s.secrets.push(PluginSecret {
                    server: "files".into(),
                    key: "TOKEN".into(),
                    secret_id: id,
                });
            }),
        ),
        ("编号", Box::new(|s| s.remote.allowed[0].id = " ".into())),
        ("128", Box::new(move |s| s.remote.allowed[0].id = long(129))),
        ("64", Box::new(move |s| s.remote.allowed[0].name = long(65))),
        ("QQ", Box::new(|s| s.remote.allowed[0].channel = "QQ".into())),
        (
            "A1B2C3",
            Box::new(|s| {
                let again = s.remote.allowed[0].clone();
                s.remote.allowed.push(again);
            }),
        ),
        (
            "200",
            Box::new(|s| {
                s.remote.allowed = (0..201)
                    .map(|index| PluginAllowedUser {
                        channel: "qq-main".into(),
                        id: format!("user-{index}"),
                        name: String::new(),
                    })
                    .collect();
            }),
        ),
        (
            "工作区",
            Box::new(|s| {
                let again = s.remote.scope.workspaces[0];
                s.remote.scope.workspaces.push(again);
            }),
        ),
        (
            "会话",
            Box::new(|s| {
                let again = s.remote.scope.sessions[0];
                s.remote.scope.sessions.push(again);
            }),
        ),
    ];
    for (hint, change) in cases {
        let mut setting = configured();
        change(&mut setting);
        let reason = check_settings(&[setting]).expect_err(hint);
        assert!(reason.contains(hint) && reason.contains("chat-bot"), "{hint}: {reason}");
    }
    let mut empty_name = configured();
    empty_name.remote.allowed[0].name = String::new();
    check_settings(&[empty_name]).expect("a name is the user's note and may be left out");
    let _: BTreeMap<String, String> = configured().values.text;
}
