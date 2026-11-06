//! What a plugin's channel passes in through its background service, short of a conversation: a
//! channel its manifest does not declare is refused, someone not on the allowlist is listed on
//! the plugin page and logged once, and with no conversation in the plugin's scope the chat is
//! told where to give it one, as a `reply` the service receives. The conversation calls wait
//! for Core's Agent application; Core's own test gives it one.
mod support;

use aura_plugin_session::{NO_SCOPE_REPLY, PluginServices};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Fixture, NODE_SERVICE, answer, eventually};

/// Sends each request once it is ready, logs every answer as `<id> ok <result>` or `<id> <code>`
/// and every reply as `reply <the line>`.
const SERVICE: &str = r#"
import { createInterface } from 'node:readline';
const say = (message) => process.stdout.write(JSON.stringify(message) + '\n');
const from = (user, message_id, change = {}) => ({
  channel: 'qq-main', chat: { kind: 'private', id: user }, user: { id: user, name: '老王' },
  message_id, text: '你好', ...change,
});
const requests = [
  ['message.in', from('stranger', 'm-1')],
  ['message.in', from('stranger', 'm-2', { text: '还在吗' })],
  ['message.in', from('friend', 'm-3', { channel: 'wx-main' })],
  ['message.in', from('friend', 'm-4', { reply_to: 'm-0' })],
  ['message.in', from('friend', 'm-5', { sender: 'admin' })],
  ['sessions.list', {}],
  ['session.stop', { session_id: 'ses_00000000000000000000000000' }],
];
createInterface({ input: process.stdin }).on('line', (line) => {
  const message = JSON.parse(line);
  if (message.type === 'hello') {
    say({ type: 'ready', protocol: 1, channels: [{ id: 'qq-main', max_text_chars: 2000 }] });
    requests.forEach(([method, params], id) => say({ type: 'request', id, method, params }));
  } else if (message.type === 'response') {
    const outcome = message.error ? message.error.code : 'ok ' + JSON.stringify(message.result);
    say({ type: 'log', level: 'info', text: `${message.id} ${outcome}` });
  } else if (message.type === 'reply') {
    say({ type: 'log', level: 'info', text: 'reply ' + JSON.stringify(message) });
  } else if (message.type === 'shutdown') {
    process.exit(0);
  }
});
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_short_of_a_conversation_is_refused_listed_or_answered_by_aura() {
    let f = Fixture::new();
    let manifest = format!(
        "capabilities = [\"session.send\", \"session.read\"]\n\n{NODE_SERVICE}\n\
         [[channels]]\nid = \"qq-main\"\nplatform = \"qq\"\ntitle = \"QQ 机器人\"\n"
    );
    f.plugin("chat-bot", &manifest, SERVICE);
    let friend = json!([{"channel": "qq-main", "id": "friend", "name": "朋友"}]);
    f.configure("chat-bot", json!({"service": true, "allowed": friend}));
    let services =
        PluginServices::start(Arc::clone(&f.plugins), &tokio::runtime::Handle::current());

    eventually("every request answered and the reply received", || {
        let logged = f.logged("chat-bot");
        answer(&logged, 6).is_some() && logged.iter().any(|line| line.starts_with("reply "))
    })
    .await;
    let logged = f.logged("chat-bot");
    let refused = r#"ok {"accepted":false,"reason":"user_not_allowed"}"#;
    assert_eq!(answer(&logged, 0).as_deref(), Some(refused));
    assert_eq!(answer(&logged, 1).as_deref(), Some(refused));
    assert_eq!(answer(&logged, 2).as_deref(), Some("channel.unknown"));
    let no_scope = r#"ok {"accepted":false,"reason":"no_scope"}"#;
    assert_eq!(answer(&logged, 3).as_deref(), Some(no_scope));
    assert_eq!(answer(&logged, 4).as_deref(), Some("protocol.bad_params"));
    assert_eq!(answer(&logged, 5).as_deref(), Some("aura.not_ready"), "no Agent application yet");
    let stop = answer(&logged, 6);
    assert_eq!(stop.as_deref(), Some("plugin_session.capability_missing"), "session.control");

    let reply: Value = logged
        .iter()
        .find_map(|line| line.strip_prefix("reply "))
        .map(|line| serde_json::from_str(line).expect("a reply line"))
        .expect("the reply");
    assert_eq!(
        reply,
        json!({"type": "reply", "channel": "qq-main", "chat": {"kind": "private", "id": "friend"},
            "reply_to": "m-4", "text": NO_SCOPE_REPLY, "format": "text"})
    );
    let noted = logged.iter().filter(|line| line.contains("不在白名单")).count();
    assert_eq!(noted, 1, "logged once, however often they write");
    let unknown = f.view("chat-bot")["remote"]["unknown"].clone();
    assert_eq!(unknown.as_array().map(Vec::len), Some(1));
    assert_eq!(
        [&unknown[0]["id"], &unknown[0]["name"], &unknown[0]["chat"]],
        [&json!("stranger"), &json!("老王"), &json!("private")]
    );
    services.stop();
}

/// Every `reply` the service received, in order.
fn replies(f: &Fixture) -> Vec<Value> {
    let logged = f.logged("chat-bot");
    let lines = logged.iter().filter_map(|line| line.strip_prefix("reply "));
    lines.map(|line| serde_json::from_str(line).expect("a reply line")).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reply_longer_than_its_channel_takes_comes_in_parts() {
    let f = Fixture::new();
    let manifest = format!(
        "capabilities = [\"session.send\", \"session.read\"]\n\n{NODE_SERVICE}\n\
         [[channels]]\nid = \"wx-main\"\nplatform = \"wechat\"\ntitle = \"微信\"\n\n\
         [[channels]]\nid = \"qq-main\"\nplatform = \"qq\"\ntitle = \"QQ 机器人\"\n"
    );
    // The chat's own channel decides; the other one takes far longer messages.
    let reported =
        "[{ id: 'wx-main', max_text_chars: 2000 }, { id: 'qq-main', max_text_chars: 20 }]";
    let service = SERVICE.replace("[{ id: 'qq-main', max_text_chars: 2000 }]", reported);
    assert_ne!(service, SERVICE);
    f.plugin("chat-bot", &manifest, &service);
    let friend = json!([{"channel": "qq-main", "id": "friend", "name": "朋友"}]);
    f.configure("chat-bot", json!({"service": true, "allowed": friend}));
    let services =
        PluginServices::start(Arc::clone(&f.plugins), &tokio::runtime::Handle::current());

    eventually("every part received", || {
        let parts = replies(&f);
        !parts.is_empty() && parts[0]["parts"].as_u64() == u64::try_from(parts.len()).ok()
    })
    .await;
    let parts = replies(&f);
    assert!(parts.len() > 1, "{parts:?}");
    for (at, part) in parts.iter().enumerate() {
        assert_eq!(
            [&part["part"], &part["reply_to"], &part["format"]],
            [&json!(at + 1), &json!("m-4"), &json!("text")]
        );
        let text = part["text"].as_str().expect("text");
        assert!((1..=20).contains(&text.chars().count()), "{part}");
    }
    let squeezed = |text: &str| text.split_whitespace().collect::<String>();
    let joined: String = parts.iter().map(|part| part["text"].as_str().expect("text")).collect();
    assert_eq!(squeezed(&joined), squeezed(NO_SCOPE_REPLY));
    services.stop();
}
