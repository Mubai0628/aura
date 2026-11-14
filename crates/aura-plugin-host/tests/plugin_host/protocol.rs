//! The protocol's lines: what a service may write, every way a line can be wrong, and what Aura
//! writes back.
use aura_agent_domain::PluginChatKind;
use aura_plugin_host::{
    ApprovalOffer, ChannelAbilities, ChatAddress, FromService, LogLevel, MenuCommand, RequestId,
    ServiceError, ToService, parse_line,
};
use aura_plugins::{PluginChannel, PluginPlatform};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn parse(value: Value) -> Result<FromService, aura_plugin_host::LineProblem> {
    parse_line(value.to_string().as_bytes())
}

#[test]
fn a_service_may_say_it_is_ready_ask_and_log() {
    let ready = parse(json!({"type": "ready", "protocol": 1, "channels": [
        {"id": "main", "max_text_chars": 2000, "markdown": true, "buttons": true,
         "command_menu": true, "groups": true}]}))
    .expect("ready");
    assert_eq!(
        ready,
        FromService::Ready {
            protocol: 1,
            channels: vec![ChannelAbilities {
                id: "main".into(),
                max_text_chars: 2000,
                markdown: true,
                buttons: true,
                command_menu: true,
                groups: true,
            }],
        }
    );
    let numbered = parse(json!({"type": "request", "id": 3, "method": "session.state",
        "params": {"session_id": "x"}}))
    .expect("request");
    assert!(matches!(numbered, FromService::Request { id: RequestId::Number(3), .. }));
    let named =
        parse(json!({"type": "request", "id": "r-1", "method": "notify"})).expect("request");
    let FromService::Request { id, method, params } = named else { panic!("a request") };
    assert_eq!((id, method.as_str(), params), (RequestId::Text("r-1".into()), "notify", json!({})));
    let log = parse(json!({"type": "log", "text": "connected"})).expect("log");
    assert_eq!(log, FromService::Log { level: LogLevel::Info, text: "connected".into() });
}

#[test]
fn every_wrong_line_names_what_is_wrong() {
    let not_json = parse_line(b"{oops").expect_err("not JSON");
    assert_eq!(not_json.code, "protocol.not_json");
    assert_eq!(parse(json!([1, 2])).expect_err("an array").code, "protocol.not_json");
    assert_eq!(parse(json!({"protocol": 1})).expect_err("no type").code, "protocol.bad_message");
    let unknown = parse(json!({"type": "hello"})).expect_err("Aura's own type");
    assert_eq!(unknown.code, "protocol.unknown_type");
    assert!(unknown.message.contains("ready、request、log"), "{}", unknown.message);
    let extra = parse(json!({"type": "ready", "protocol": 1, "colour": "red"})).expect_err("extra");
    assert!(extra.message.contains("colour"), "{}", extra.message);
    let no_id = parse(json!({"type": "request", "method": "notify"})).expect_err("no id");
    assert_eq!((no_id.code, no_id.id), ("protocol.bad_message", None));
    let long_id = parse(json!({"type": "request", "id": "x".repeat(65), "method": "notify"}));
    assert_eq!(long_id.expect_err("a long id").id, None);
    let fraction = parse(json!({"type": "request", "id": 1.5, "method": "notify"}));
    assert_eq!(fraction.expect_err("a fraction").id, None);
    for (line, why) in [
        (json!({"type": "request", "id": 9, "method": "Notify"}), "method"),
        (json!({"type": "request", "id": 9, "method": "notify", "params": [1]}), "params"),
        (json!({"type": "request", "id": 9, "method": "notify", "extra": true}), "extra"),
        (json!({"type": "request", "id": 9}), "method"),
    ] {
        let problem = parse(line).expect_err(why);
        assert_eq!(problem.id, Some(RequestId::Number(9)), "{why}: answered to its request");
        assert!(problem.message.contains(why), "{why}: {}", problem.message);
    }
    let level = parse(json!({"type": "log", "level": "debug", "text": "x"})).expect_err("level");
    assert_eq!(level.code, "protocol.bad_message");
}

#[test]
fn aura_writes_one_json_object_per_line() {
    let hello = ToService::Hello {
        protocol: 1,
        plugin: "chat-bot".into(),
        channels: vec![PluginChannel {
            id: "main".into(),
            platform: PluginPlatform::Telegram,
            title: "Telegram".into(),
        }],
        settings: BTreeMap::from([("app_id".to_owned(), "1".to_owned())]),
    };
    let line = hello.encode();
    assert_eq!(line.last(), Some(&b'\n'));
    assert_eq!(line.iter().filter(|byte| **byte == b'\n').count(), 1);
    let value: Value = serde_json::from_slice(&line).expect("json");
    assert_eq!(
        value,
        json!({"type": "hello", "protocol": 1, "plugin": "chat-bot",
            "channels": [{"id": "main", "platform": "telegram", "title": "Telegram"}],
            "settings": {"app_id": "1"}})
    );
    let ok = ToService::answer(RequestId::Number(1), Ok(json!({"a": 1})));
    assert_eq!(
        serde_json::from_slice::<Value>(&ok.encode()).expect("json"),
        json!({"type": "response", "id": 1, "result": {"a": 1}})
    );
    let failed = ToService::answer(
        RequestId::Text("x".into()),
        Err(ServiceError::new("protocol.busy", "等一等")),
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&failed.encode()).expect("json"),
        json!({"type": "response", "id": "x", "error": {"code": "protocol.busy", "message": "等一等"}})
    );
    assert_eq!(ToService::Shutdown.encode(), b"{\"type\":\"shutdown\"}\n");
    // A channel's command menu: a command without an argument names no hint.
    let menu = ToService::Commands {
        channel: "main".into(),
        commands: vec![
            MenuCommand {
                name: "plan".into(),
                title: "Plan 模式".into(),
                description: "先梳理方案".into(),
                argument_hint: Some("要做的事".into()),
            },
            MenuCommand {
                name: "help".into(),
                title: "命令帮助".into(),
                description: "列出命令".into(),
                argument_hint: None,
            },
        ],
    };
    assert_eq!(
        serde_json::from_slice::<Value>(&menu.encode()).expect("json"),
        json!({"type": "commands", "channel": "main", "commands": [
            {"name": "plan", "title": "Plan 模式", "description": "先梳理方案",
             "argument_hint": "要做的事"},
            {"name": "help", "title": "命令帮助", "description": "列出命令"}]})
    );
    // An approval handed to a chat: what it asks, and the decisions the chat may give.
    let approval = ToService::Approval {
        channel: "main".into(),
        chat: ChatAddress { kind: PluginChatKind::Private, id: "u1".into() },
        approval: ApprovalOffer {
            id: "3".into(),
            title: "写入文件".into(),
            detail: "写结果\nagent-result.txt".into(),
            decisions: vec!["approve".into(), "deny".into()],
        },
        text: "Aura 请你决定（编号 3）：写入文件".into(),
    };
    assert_eq!(
        serde_json::from_slice::<Value>(&approval.encode()).expect("json"),
        json!({"type": "approval", "channel": "main", "chat": {"kind": "private", "id": "u1"},
            "approval": {"id": "3", "title": "写入文件", "detail": "写结果\nagent-result.txt",
                "decisions": ["approve", "deny"]},
            "text": "Aura 请你决定（编号 3）：写入文件"})
    );
}

#[test]
fn an_error_of_auras_own_keeps_its_code() {
    let error = ServiceError::from_reason(
        "plugin_session.capability_missing: 插件 chat-bot 没有声明 notify",
    );
    assert_eq!(error.code, "plugin_session.capability_missing");
    assert_eq!(error.message, "插件 chat-bot 没有声明 notify");
    let plain = ServiceError::from_reason("数据库忙: 稍后再试");
    assert_eq!(plain.code, "aura.failed");
    assert_eq!(plain.message, "数据库忙: 稍后再试");
}
