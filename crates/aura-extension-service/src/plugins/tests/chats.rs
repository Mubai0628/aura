//! What Aura keeps for a plugin's channels, written only by Aura: the conversation each chat is in
//! now, and who wrote lately without being on the allowlist. The plugin page lists the latter
//! with the channels, the scope, the allowlist and the remote approval switch.
use super::service::{bot, configure};
use super::support::{Fixture, write};
use aura_agent_domain::{PluginChat, PluginChatKind};
use aura_domain_common::{SessionId, UtcTimestamp, WorkspaceId};
use aura_plugins::{PLUGIN_UNKNOWN_USERS_MAX, PluginUnknownUser};
use serde_json::json;

fn chat(id: &str) -> PluginChat {
    PluginChat { channel: "qq-main".into(), kind: PluginChatKind::Private, id: id.into() }
}

fn stranger(id: &str, name: &str) -> PluginUnknownUser {
    PluginUnknownUser {
        channel: "qq-main".into(),
        id: id.into(),
        name: name.into(),
        chat: PluginChatKind::Group,
        at: UtcTimestamp::now(),
    }
}

#[test]
fn a_chat_keeps_its_conversation_and_the_others_used_lately_come_next() {
    let f = Fixture::new();
    bot(&f);
    let (first, second, third) = (SessionId::new(), SessionId::new(), SessionId::new());
    assert_eq!(f.runtime.chat_sessions("chat-bot", &chat("a")), (None, Vec::new()));
    f.runtime.remember_chat("chat-bot", &chat("a"), first);
    f.runtime.remember_chat("chat-bot", &chat("b"), second);
    f.runtime.remember_chat("chat-bot", &chat("c"), first);
    f.runtime.remember_chat("chat-bot", &chat("a"), third);
    assert_eq!(f.runtime.chat_sessions("chat-bot", &chat("a")), (Some(third), vec![first, second]));
    assert_eq!(f.runtime.chat_sessions("chat-bot", &chat("b")), (Some(second), vec![third, first]));
    // A conversation two chats are in comes once.
    f.runtime.remember_chat("chat-bot", &chat("d"), first);
    assert_eq!(f.runtime.chat_sessions("chat-bot", &chat("b")), (Some(second), vec![first, third]));
    let group = PluginChat { kind: PluginChatKind::Group, ..chat("a") };
    assert_eq!(f.runtime.chat_sessions("chat-bot", &group).0, None, "a group is another chat");
    assert_eq!(f.runtime.chat_sessions("mail-bot", &chat("a")), (None, Vec::new()));
}

#[test]
fn at_most_so_many_chats_are_kept_and_the_one_used_longest_ago_goes() {
    let f = Fixture::new();
    bot(&f);
    let session = SessionId::new();
    for index in 0..=crate::plugins::PLUGIN_CHATS_MAX {
        f.runtime.remember_chat("chat-bot", &chat(&format!("c{index}")), session);
    }
    assert_eq!(f.runtime.chat_sessions("chat-bot", &chat("c0")).0, None);
    assert_eq!(f.runtime.chat_sessions("chat-bot", &chat("c1")).0, Some(session));
    let last = format!("c{}", crate::plugins::PLUGIN_CHATS_MAX);
    assert_eq!(f.runtime.chat_sessions("chat-bot", &chat(&last)).0, Some(session));
}

#[test]
fn people_not_on_the_list_are_kept_newest_first_once_each_and_only_so_many() {
    let f = Fixture::new();
    bot(&f);
    for index in 0..=PLUGIN_UNKNOWN_USERS_MAX {
        assert!(f.runtime.note_unknown("chat-bot", stranger(&format!("u{index}"), "")));
    }
    assert!(!f.runtime.note_unknown("chat-bot", stranger("u5", "老王")), "u5 is listed already");
    assert!(f.runtime.note_unknown("chat-bot", stranger("u0", "")), "u0 had dropped off the list");
    assert!(!f.runtime.note_unknown("chat-bot", stranger("u5", "老王")));
    let unknown = f.view("chat-bot")["remote"]["unknown"].clone();
    let ids: Vec<&str> = unknown
        .as_array()
        .expect("unknown")
        .iter()
        .map(|user| user["id"].as_str().expect("id"))
        .collect();
    assert_eq!(ids.len(), PLUGIN_UNKNOWN_USERS_MAX);
    assert_eq!(ids[0], "u5", "the one who wrote last comes first");
    assert_eq!(unknown[0]["name"], "老王", "with the name they have now");
    assert_eq!(ids.iter().filter(|id| **id == "u5").count(), 1);
    assert_eq!(ids[1], "u0", "listed again after writing again");
    assert!(!ids.contains(&"u1"), "the one who wrote longest ago goes");
    assert_eq!(unknown[0]["chat"], "group");
}

#[test]
fn the_plugin_page_shows_the_channels_scope_allowlist_and_who_wrote_without_being_on_it() {
    let f = Fixture::new();
    bot(&f);
    assert_eq!(
        f.view("chat-bot")["remote"],
        json!({"channels": [{"id": "qq-main", "platform": "qq", "title": "QQ 机器人"}],
            "scope": {}, "remote_approval": false})
    );
    f.runtime.note_unknown("chat-bot", stranger("A1B2", "我"));
    f.runtime.note_unknown("chat-bot", stranger("C3D4", ""));
    let workspace = WorkspaceId::new();
    configure(
        &f,
        json!({"scope": {"workspaces": [workspace]},
            "allowed": [{"channel": "qq-main", "id": "A1B2", "name": "我"}],
            "remote_approval": true}),
    )
    .expect("saved");
    let remote = f.view("chat-bot")["remote"].clone();
    assert_eq!(remote["scope"], json!({"workspaces": [workspace]}));
    assert_eq!(remote["allowed"], json!([{"channel": "qq-main", "id": "A1B2", "name": "我"}]));
    assert_eq!(remote["remote_approval"], true);
    let unknown = remote["unknown"].as_array().expect("unknown");
    assert_eq!(unknown.len(), 1, "someone added to the list is no longer listed: {remote}");
    assert_eq!(unknown[0]["id"], "C3D4");
}

#[test]
fn a_state_file_that_cannot_be_read_counts_as_empty_and_is_replaced() {
    let f = Fixture::new();
    bot(&f);
    let file = f.runtime.chat_state_file("chat-bot");
    write(&file, "{ not json");
    assert_eq!(f.runtime.chat_sessions("chat-bot", &chat("a")), (None, Vec::new()));
    assert!(f.view("chat-bot")["remote"].get("unknown").is_none());
    let session = SessionId::new();
    f.runtime.remember_chat("chat-bot", &chat("a"), session);
    assert_eq!(f.runtime.chat_sessions("chat-bot", &chat("a")).0, Some(session));
    let text = std::fs::read_to_string(&file).expect("state file");
    assert!(serde_json::from_str::<serde_json::Value>(&text).is_ok(), "{text}");
}

#[test]
fn a_plugin_without_a_background_service_has_nothing_to_reach() {
    let f = Fixture::new();
    let source = super::support::plugin(&f.source(), "plain", "0.1.0");
    f.import(&source).expect("imported");
    assert!(f.view("plain").get("remote").is_none());
}
