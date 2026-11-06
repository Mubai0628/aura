//! Approvals decided in a chat, through Core's own pieces. With the plugin's 远程批准 on, the
//! approval a chat's turn waits for is handed to that private chat, one at a time for the plugin
//! with the rest waiting in order, the next once the one out was decided in its chat or in Aura's
//! window; a decision is taken only from that chat, a while after it was handed, and only as the
//! approval allows from a chat; who decided shows on the call's row and goes with the
//! conversation. A wider permission mode is taken the same way, a while after the modes were
//! listed. The service is a real Node.js program; only the Provider is a fixture.
use super::l17_plugin_channels::{chat_bot_with, logged, mode_of, services};
use super::*;
use provider_fixture::Scenario;

const SERVICE: &str = include_str!("l17_plugin_approvals.mjs");

/// A turn that waits for its approval while the plugin's chats may not decide approvals: the
/// service logs any approval handed to it, and the chat's answer.
const QUIET_SERVICE: &str = r#"
import { createInterface } from 'node:readline';
const say = (message) => process.stdout.write(JSON.stringify(message) + '\n');
const log = (text) => say({ type: 'log', level: 'info', text });
const waiting = new Map();
const replies = new Map();
let next = 1;
const call = (method, params) => new Promise((resolve) => {
  const id = next++;
  waiting.set(id, resolve);
  say({ type: 'request', id, method, params });
});
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const reply = (id) => new Promise((resolve) => replies.set(id, resolve));
const shown = (answer) => JSON.stringify(answer.error ? { error: answer.error.code } : answer.result);
const from = (message_id, text) => ({ channel: 'qq-main', chat: { kind: 'private', id: 'friend' },
  user: { id: 'friend', name: '老王' }, message_id, text });
async function run() {
  const written = reply('write');
  const answer = await call('message.in', from('write', '写一个文件'));
  log('answer write ' + shown(answer));
  for (;;) {
    const state = await call('session.state', { session_id: answer.result.session_id });
    if (state.result?.waiting_for === 'approval') break;
    await sleep(100);
  }
  // A few rounds of handing out pass.
  await sleep(2500);
  const told = reply('approve');
  log('answer approve ' + shown(await call('message.in', from('approve', '/approve'))));
  log('reply approve ' + JSON.stringify(await told));
  log('waited {}');
  log('reply write ' + JSON.stringify(await written));
  log('done {}');
}
createInterface({ input: process.stdin }).on('line', (line) => {
  const message = JSON.parse(line);
  if (message.type === 'hello') {
    say({ type: 'ready', protocol: 1, channels: [{ id: 'qq-main', max_text_chars: 2000 }] });
    run();
  } else if (message.type === 'response') {
    waiting.get(message.id)?.(message);
  } else if (message.type === 'reply') {
    replies.get(message.reply_to)?.(message);
  } else if (message.type === 'approval') {
    log('got ' + message.chat.id + ' ' + message.approval.id);
  } else if (message.type === 'shutdown') {
    process.exit(0);
  }
});
"#;

/// One write, which asks first in 只读.
const ONE_WRITE: &[(&str, &str)] =
    &[("fs.write", r#"{"path":"quiet.txt","text":"q","expected":{"expected_kind":"absent"}}"#)];

/// Each turn writes a file, then deletes it for good: in 只读 each asks first, and deleting for
/// good may only be allowed once.
const CALLS: &[(&str, &str)] = &[
    ("fs.write", r#"{"path":"remote-a.txt","text":"a","expected":{"expected_kind":"absent"}}"#),
    ("fs.delete", r#"{"path":"remote-a.txt","permanent":true}"#),
];

/// The calls of the conversation, oldest first: the tool, and who decided its approval in a chat.
async fn decided(runtime: &crate::RuntimeState, session: &Value) -> Vec<(Value, Value)> {
    let page = crate::r03_tests::success(
        runtime,
        "agent.tool_activity",
        json!({"session_id": session, "before_call_id": null, "limit": 20}),
    )
    .await;
    let mut calls: Vec<(Value, Value)> = page["calls"]
        .as_array()
        .expect("calls")
        .iter()
        .map(|call| (call["summary"]["display_name"].clone(), call["remote_decision"].clone()))
        .collect();
    calls.reverse();
    calls
}

/// The fixture's model as the default, new conversations in 只读, and the workspace the chats'
/// conversations go to.
async fn read_only_chats(
    host: &crate::r03_tests::Fixture,
    runtime: &crate::RuntimeState,
    model: &Value,
) -> (WorkspaceId, std::path::PathBuf) {
    let snapshot =
        crate::r03_tests::success(runtime, "models.settings.get_snapshot", json!({})).await;
    crate::r03_tests::success(
        runtime,
        "models.default.set",
        json!({"expected_revision": snapshot["aura_revision"], "model_profile_id": model}),
    )
    .await;
    let defaults = crate::r03_tests::success(runtime, "permission.get_defaults", json!({})).await;
    if defaults["mode"] != "read_only" {
        crate::r03_tests::success(
            runtime,
            "permission.set_defaults",
            json!({"expected_revision": defaults["revision"], "mode": "read_only"}),
        )
        .await;
    }
    let root = host.root.path().join("chats-workspace");
    std::fs::create_dir(&root).expect("workspace folder");
    let created = crate::r03_tests::success(
        runtime,
        "workspace.create",
        json!({"selected_root": root, "display_name": "Chats"}),
    )
    .await;
    let chats = serde_json::from_value(created["workspace"]["workspace_id"].clone());
    (chats.expect("workspace"), root)
}

/// The user decides, in Aura's window, the approval the conversation's turn waits for.
async fn on_computer(runtime: &crate::RuntimeState, session: &Value, decision: &str) {
    let approval_id = {
        let id: SessionId = serde_json::from_value(session.clone()).expect("session");
        let path = services(runtime).broker.database_path().expect("the store's path");
        let reader = aura_persistence_core::open_reader(&path).expect("a reader");
        let turn = aura_persistence_v2::agent::session::load(&reader, id).expect("session");
        let turn = turn.active_turn.expect("a turn under way");
        let run =
            aura_persistence_v2::agent::turns::load(&reader, turn).expect("turn").agent_run_id;
        let pending = aura_persistence_v2::agent::approval::pending(&reader, run).expect("pending");
        pending.first().copied().expect("an approval")
    };
    let asked = json!({"session_id": session, "approval_id": approval_id});
    let approval = crate::r03_tests::success(runtime, "agent.approval.get", asked).await;
    crate::r03_tests::success(
        runtime,
        "agent.approval.resolve",
        json!({"session_id": session, "approval_id": approval_id,
            "expected_revision": approval["approval"]["revision"], "decision": decision}),
    )
    .await;
}

/// What the service logged as `<name> <json>`, once it did; otherwise what it logged instead.
async fn awaited(runtime: &crate::RuntimeState, name: &str) -> Value {
    let found = tokio::time::timeout(std::time::Duration::from_secs(240), async {
        loop {
            if let Some(found) = logged(runtime, name) {
                return found;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });
    match found.await {
        Ok(found) => found,
        Err(_) => {
            let logs = services(runtime).plugins.recent_logs("chat-bot", 200);
            let texts: Vec<String> = match logs {
                Ok((entries, _)) => entries.into_iter().map(|entry| entry.text).collect(),
                Err(error) => vec![format!("{error:#}")],
            };
            panic!("the service never logged {name}: {texts:#?}")
        }
    }
}

#[tokio::test]
async fn l17_a_chat_decides_the_approval_handed_to_it_under_the_remote_approval_rules() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Sequence { calls: CALLS }).await;
    let (runtime, _, model) = prepare(&host, &provider).await;
    let (chats, root) = read_only_chats(&host, &runtime, &json!(model)).await;
    let settings = json!({"service": true, "scope": {"workspaces": [chats]},
        "allowed": [{"channel": "qq-main", "id": "friend", "name": "朋友"},
            {"channel": "qq-main", "id": "buddy", "name": "伙伴"}],
        "remote_approval": true});
    chat_bot_with(&host, &runtime, settings, SERVICE).await;
    // While friend's deletion is out, buddy's write waits; the user refuses the deletion in
    // Aura's window, and buddy's goes out after it.
    awaited(&runtime, "buddy-waiting").await;
    let (entries, _) = services(&runtime).plugins.recent_logs("chat-bot", 200).expect("logs");
    assert!(!entries.iter().any(|entry| entry.text.starts_with("got buddy")), "one out at a time");
    let written = logged(&runtime, "answer write").expect("answered");
    on_computer(&runtime, &written["session_id"], "deny").await;
    awaited(&runtime, "done").await;

    let reply = |label: &str| -> String {
        let reply =
            logged(&runtime, &format!("reply {label}")).unwrap_or_else(|| panic!("reply {label}"));
        assert_eq!(reply["reply_to"], json!(label), "{reply}");
        reply["text"].as_str().expect("the reply's text").to_owned()
    };
    let approval = |label: &str| {
        logged(&runtime, &format!("approval {label}")).unwrap_or_else(|| panic!("approval {label}"))
    };

    // While its chats may decide approvals, the decisions are listed and in the menu.
    let help = reply("help");
    for listed in ["/approve 编号（可选） — 批准", "/approve-session 编号（可选）", "/deny"]
    {
        assert!(help.contains(listed), "{listed}: {help}");
    }
    let menu = logged(&runtime, "menu").expect("the channel shows a menu");
    let names: Vec<&str> = menu["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .map(|command| command["name"].as_str().expect("name"))
        .collect();
    assert!(names.ends_with(&["approve", "approve-session", "deny"]), "{names:?}");
    assert_eq!(reply("none"), "现在没有等这个聊天决定的审批。");
    let stranger = logged(&runtime, "answer stranger").expect("answered");
    assert_eq!(stranger, json!({"accepted": false, "reason": "user_not_allowed"}));
    // The write friend's turn waits for, handed to friend's private chat.
    let first = approval("first");
    let purpose = "Verify the user-requested mode boundary";
    assert_eq!(
        first,
        json!({"type": "approval", "channel": "qq-main", "chat": {"kind": "private", "id": "friend"},
            "approval": {"id": "1", "title": "写入文件", "detail": format!("{purpose}\nremote-a.txt"),
                "decisions": ["approve", "approve_session", "deny"]},
            "text": format!("Aura 请你决定（编号 1）：写入文件\n{purpose}\nremote-a.txt\n\
                回复 /approve 批准这一次，/approve-session 本会话都允许，/deny 拒绝。")})
    );
    assert_eq!(reply("soon"), "审批交给你后 3 秒内的回复不算，请再发一次。");
    assert_eq!(reply("group"), "审批和放宽权限只能在私聊里决定。");
    assert_eq!(reply("other"), "现在等你决定的是编号 1 的审批。");
    assert_eq!(reply("approve"), "已批准（仅此一次）。");
    let again = logged(&runtime, "answer approve-again").expect("answered");
    assert_eq!(again, json!({"accepted": true, "command": "approve", "repeated": true}));

    // friend's deletion for good may only be refused from a chat.
    let second = approval("second");
    assert_eq!(
        (&second["chat"]["id"], &second["approval"]["id"], &second["approval"]["title"]),
        (&json!("friend"), &json!("2"), &json!("删除文件"))
    );
    assert_eq!(second["approval"]["decisions"], json!(["deny"]));
    let text = second["text"].as_str().expect("text");
    assert!(text.contains("这条只能在电脑上的 Aura 里批准。\n回复 /deny 拒绝。"), "{text}");
    assert_eq!(reply("buddy-new"), "已新建会话 QQ 机器人 · 小李，这个聊天已换过去。");
    assert_eq!(reply("approve-risky"), "这条审批只能在电脑上的 Aura 里批准；可以回 /deny。");
    assert_eq!(reply("write"), "Done.");
    // buddy's went out once the deletion was refused in Aura's window.
    let third = approval("third");
    assert_eq!((&third["chat"]["id"], &third["approval"]["id"]), (&json!("buddy"), &json!("3")));
    assert_eq!(third["approval"]["decisions"], json!(["approve", "approve_session", "deny"]));
    assert_eq!(reply("buddy-session"), "已批准（本会话）。");
    let fourth = approval("fourth");
    assert_eq!((&fourth["chat"]["id"], &fourth["approval"]["id"]), (&json!("buddy"), &json!("4")));
    assert_eq!(fourth["approval"]["decisions"], json!(["deny"]));
    assert_eq!(reply("buddy-deny"), "已拒绝。");
    assert_eq!(reply("buddy-write"), "Done.");
    // Never to the group, and no approval twice.
    let (entries, _) = services(&runtime).plugins.recent_logs("chat-bot", 200).expect("logs");
    let handed: Vec<&str> =
        entries.iter().filter_map(|entry| entry.text.strip_prefix("got ")).collect();
    assert_eq!(handed, ["friend 1", "friend 2", "buddy 3", "buddy 4"]);
    assert!(root.join("remote-a.txt").exists(), "the deletions were refused");

    // Who decided shows on each call, as the conversation lists them; nobody in a chat decided
    // what was refused in Aura's window.
    let friend = json!({"plugin": "chat-bot", "from": {"channel": "qq-main", "platform": "qq",
        "chat": "private", "user_id": "friend", "user_name": "老王"}});
    let buddy = json!({"plugin": "chat-bot", "from": {"channel": "qq-main", "platform": "qq",
        "chat": "private", "user_id": "buddy", "user_name": "小李"}});
    let theirs = logged(&runtime, "answer buddy-write").expect("answered");
    assert_ne!(theirs["session_id"], written["session_id"], "buddy's own conversation");
    assert_eq!(
        decided(&runtime, &written["session_id"]).await,
        [
            (json!("fs.write"), json!({"sender": friend, "decision": "allow_once"})),
            (json!("fs.delete"), Value::Null)
        ]
    );
    assert_eq!(
        decided(&runtime, &theirs["session_id"]).await,
        [
            (json!("fs.write"), json!({"sender": buddy, "decision": "allow_session"})),
            (json!("fs.delete"), json!({"sender": buddy, "decision": "deny"}))
        ]
    );

    // A wider mode only a while after the modes were listed.
    assert_eq!(reply("wider-unlisted"), "先用 /permission 看一下列表，3 秒后再选。");
    assert!(reply("modes").contains("1. 只读（当前）"));
    assert_eq!(reply("wider-soon"), "列表发出后 3 秒内的选择不算，请再发一次。");
    assert_eq!(reply("wider"), "权限已切换到 完全访问。");
    assert_eq!(mode_of(&runtime, &written["session_id"]).await, "full_access");

    // Who decided goes with the conversation.
    let path = services(&runtime).broker.database_path().expect("the store's path");
    let kept = || -> i64 {
        aura_persistence_core::open_reader(&path)
            .expect("a reader")
            .query_row(
                "SELECT count(*) FROM events WHERE kind='agent.approval_decider'",
                [],
                |row| row.get(0),
            )
            .expect("the count")
    };
    assert_eq!(kept(), 3);
    let session = json!({"session_id": theirs["session_id"]});
    crate::r03_tests::success(&runtime, "session.archive", session.clone()).await;
    crate::r03_tests::success(&runtime, "session.delete", session).await;
    crate::session_lifecycle::deleted(&runtime).await;
    assert_eq!(kept(), 1);
}

#[tokio::test]
async fn l17_no_approval_is_handed_to_a_chat_while_the_plugins_chats_may_not_decide_them() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Sequence { calls: ONE_WRITE }).await;
    let (runtime, _, model) = prepare(&host, &provider).await;
    let (chats, root) = read_only_chats(&host, &runtime, &json!(model)).await;
    let settings = json!({"service": true, "scope": {"workspaces": [chats]},
        "allowed": [{"channel": "qq-main", "id": "friend", "name": "朋友"}]});
    chat_bot_with(&host, &runtime, settings, QUIET_SERVICE).await;
    awaited(&runtime, "waited").await;
    let told = logged(&runtime, "reply approve").expect("told why");
    assert_eq!(told["text"], "这个插件没有打开远程批准；在 Aura 的 设置 → 插件 里可以打开。");

    // The user decides on the computer instead.
    let session = logged(&runtime, "answer write").expect("answered")["session_id"].clone();
    on_computer(&runtime, &session, "allow_once").await;
    assert_eq!(awaited(&runtime, "reply write").await["text"], "Done.");
    assert!(root.join("quiet.txt").exists());
    let (entries, _) = services(&runtime).plugins.recent_logs("chat-bot", 200).expect("logs");
    assert!(entries.iter().all(|entry| !entry.text.starts_with("got ")), "nothing handed out");
    // Decided in Aura's window: the call's row names no chat.
    assert_eq!(decided(&runtime, &session).await, [(json!("fs.write"), Value::Null)]);
}
