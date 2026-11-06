//! A message one of a plugin's channels passes in, checked field by field; the conversation it
//! goes to, chosen inside the plugin's scope; and the reply Aura sends back once the turn that
//! took it ended, kept for a bounded time and no longer once the service should not run.
use aura_agent_domain::{AgentTurnOutcome, PluginChat, PluginChatKind};
use aura_domain_common::{EventId, SessionId, WorkspaceId};
use aura_plugin_host::ReplyFormat;
use aura_plugin_session::{
    Choice, Known, PENDING_REPLIES_MAX, PENDING_REPLY_TIME, PendingReplies, choose, in_scope,
    incoming, reply_for,
};
use aura_plugins::{PluginChannel, PluginPlatform, PluginScope};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn declared() -> Vec<PluginChannel> {
    vec![PluginChannel {
        id: "qq-main".into(),
        platform: PluginPlatform::Qq,
        title: "QQ 机器人".into(),
    }]
}

fn message() -> Value {
    json!({"channel": "qq-main", "chat": {"kind": "private", "id": "A1B2"},
        "user": {"id": "A1B2", "name": "老王"}, "message_id": "msg-1", "text": "你好"})
}

fn with(change: Value) -> Value {
    let mut value = message();
    let fields = value.as_object_mut().expect("object");
    for (key, part) in change.as_object().expect("change") {
        if part.is_null() {
            fields.remove(key);
        } else {
            fields.insert(key.clone(), part.clone());
        }
    }
    value
}

#[test]
fn a_message_from_a_declared_channel_is_taken_as_it_was_written() {
    let taken = incoming(message(), &declared()).expect("taken");
    assert_eq!(
        taken.chat,
        PluginChat { channel: "qq-main".into(), kind: PluginChatKind::Private, id: "A1B2".into() }
    );
    assert_eq!((taken.user_id.as_str(), taken.user_name.as_str()), ("A1B2", "老王"));
    assert_eq!((taken.message_id.as_str(), taken.text.as_str()), ("msg-1", "你好"));
    assert_eq!(taken.reply_to, None);
    assert_eq!(taken.platform, PluginPlatform::Qq);
    let group = with(json!({"chat": {"kind": "group", "id": "G9"}, "reply_to": "msg-0",
        "user": {"id": "A1B2"}}));
    let taken = incoming(group, &declared()).expect("a group message");
    assert_eq!(
        (taken.chat.kind, taken.reply_to.as_deref()),
        (PluginChatKind::Group, Some("msg-0"))
    );
    assert_eq!(taken.user_name, "", "a name is not needed");
}

#[test]
fn every_wrong_part_of_a_message_is_named() {
    let long = |count: usize| "x".repeat(count);
    let cases = [
        (with(json!({"channel": "wx-main"})), "channel.unknown"),
        (with(json!({"chat": {"kind": "channel", "id": "A1B2"}})), "protocol.bad_params"),
        (with(json!({"chat": {"kind": "private", "id": ""}})), "channel.chat_invalid"),
        (with(json!({"chat": {"kind": "private", "id": long(129)}})), "channel.chat_invalid"),
        (with(json!({"chat": {"kind": "private", "id": "a\nb"}})), "channel.chat_invalid"),
        (with(json!({"user": {"id": " "}})), "channel.user_invalid"),
        (with(json!({"user": {"id": "A1B2", "name": long(65)}})), "channel.user_invalid"),
        (with(json!({"message_id": ""})), "channel.message_id_invalid"),
        (with(json!({"message_id": long(129)})), "channel.message_id_invalid"),
        (with(json!({"text": "  "})), "channel.text_invalid"),
        (with(json!({"text": long(16_001)})), "channel.text_invalid"),
        (with(json!({"reply_to": long(129)})), "channel.reply_to_invalid"),
        (with(json!({"text": null})), "protocol.bad_params"),
        (with(json!({"sender": "admin"})), "protocol.bad_params"),
    ];
    for (value, code) in cases {
        let refused = incoming(value.clone(), &declared()).expect_err("refused");
        assert_eq!(refused.code, code, "{value}: {}", refused.message);
        assert!(!refused.message.is_empty());
    }
}

fn scope(workspaces: &[WorkspaceId], sessions: &[SessionId]) -> PluginScope {
    PluginScope { workspaces: workspaces.to_vec(), sessions: sessions.to_vec() }
}

#[test]
fn a_conversation_is_in_scope_through_its_workspace_or_by_itself() {
    let (inside, outside, own) = (WorkspaceId::new(), WorkspaceId::new(), SessionId::new());
    let scope = scope(&[inside], &[own]);
    assert!(in_scope(&scope, SessionId::new(), inside));
    assert!(in_scope(&scope, own, outside));
    assert!(!in_scope(&scope, SessionId::new(), outside));
    assert!(!in_scope(&PluginScope::default(), own, inside));
}

#[test]
fn a_chat_stays_in_its_conversation_while_that_is_in_scope() {
    let workspace = WorkspaceId::new();
    let (first, second, newest) = (SessionId::new(), SessionId::new(), SessionId::new());
    let sessions = [(first, workspace), (second, workspace), (newest, workspace)];
    let known = Known { sessions: &sessions, workspaces: &[workspace] };
    let all = scope(&[workspace], &[]);
    assert_eq!(choose(&all, &known, Some(second), &[first]), Choice::Use(second));
    // Out of scope now, or deleted: the conversation the plugin's chats used last, then the
    // newest one in the scope's workspaces.
    let only_first = scope(&[], &[first]);
    assert_eq!(choose(&only_first, &known, Some(second), &[newest, first]), Choice::Use(first));
    let gone = SessionId::new();
    assert_eq!(choose(&all, &known, Some(gone), &[]), Choice::Use(newest));
    assert_eq!(choose(&all, &known, None, &[gone, first]), Choice::Use(first));
}

#[test]
fn a_scope_of_conversations_uses_the_first_that_exists() {
    let workspace = WorkspaceId::new();
    let (gone, kept) = (SessionId::new(), SessionId::new());
    let sessions = [(kept, workspace)];
    let known = Known { sessions: &sessions, workspaces: &[workspace] };
    assert_eq!(choose(&scope(&[], &[gone, kept]), &known, None, &[]), Choice::Use(kept));
    assert_eq!(choose(&scope(&[], &[gone]), &known, None, &[]), Choice::NoScope);
}

#[test]
fn a_workspace_without_conversations_gets_a_new_one_and_an_empty_scope_none() {
    let (empty, missing) = (WorkspaceId::new(), WorkspaceId::new());
    let known = Known { sessions: &[], workspaces: &[empty] };
    assert_eq!(choose(&scope(&[missing, empty], &[]), &known, None, &[]), Choice::Create(empty));
    assert_eq!(choose(&scope(&[missing], &[]), &known, None, &[]), Choice::NoScope);
    assert_eq!(choose(&PluginScope::default(), &known, None, &[]), Choice::NoScope);
}

#[test]
fn the_reply_is_the_final_answer_or_says_how_the_turn_ended() {
    let (text, format) = reply_for(Some(AgentTurnOutcome::Completed), Some("**好的**"));
    assert_eq!((text.as_str(), format), ("**好的**", ReplyFormat::Markdown));
    for (outcome, said) in [
        (Some(AgentTurnOutcome::Completed), "没有回复"),
        (Some(AgentTurnOutcome::Failed), "出错"),
        (Some(AgentTurnOutcome::Cancelled), "停止"),
        (Some(AgentTurnOutcome::RecoveryRequired), "恢复"),
        (None, "恢复"),
    ] {
        let (text, format) = reply_for(outcome, None);
        assert!(text.contains(said), "{outcome:?}: {text}");
        assert_eq!(format, ReplyFormat::Text);
    }
}

#[test]
fn replies_wait_a_bounded_time_and_a_plugin_keeps_only_so_many() {
    let start = Instant::now();
    let mut pending = PendingReplies::default();
    for _ in 0..=PENDING_REPLIES_MAX {
        pending.wait_for_turn(
            "chat-bot",
            message_chat(),
            None,
            SessionId::new(),
            EventId::new(),
            0,
            start,
        );
    }
    pending.wait_for_turn(
        "mail-bot",
        message_chat(),
        None,
        SessionId::new(),
        EventId::new(),
        0,
        start,
    );
    assert_eq!(pending.count("chat-bot"), PENDING_REPLIES_MAX, "the oldest one goes");
    assert_eq!(pending.count("mail-bot"), 1, "another plugin's are its own");
    pending.ready("chat-bot", message_chat(), Some("msg-1".into()), "去设置".into(), start);
    assert_eq!(pending.count("chat-bot"), PENDING_REPLIES_MAX);
    pending.expire(start + PENDING_REPLY_TIME - Duration::from_secs(1));
    assert_eq!(pending.count("mail-bot"), 1);
    pending.expire(start + PENDING_REPLY_TIME);
    assert_eq!((pending.count("chat-bot"), pending.count("mail-bot")), (0, 0));
}

#[test]
fn a_plugin_whose_service_should_no_longer_run_is_owed_nothing() {
    let now = Instant::now();
    let mut pending = PendingReplies::default();
    pending.ready("chat-bot", message_chat(), None, "去设置".into(), now);
    pending.ready("mail-bot", message_chat(), None, "去设置".into(), now);
    pending.forget("chat-bot");
    assert_eq!((pending.count("chat-bot"), pending.count("mail-bot")), (0, 1));
}

fn message_chat() -> PluginChat {
    PluginChat { channel: "qq-main".into(), kind: PluginChatKind::Private, id: "A1B2".into() }
}
