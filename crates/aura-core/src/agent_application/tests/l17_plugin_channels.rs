//! A plugin's background service in conversations, through Core's own pieces. A message one of
//! its channels received from someone on the allowlist goes to a new conversation in the
//! plugin's scope, in the default permission mode, bound to the user's default model and marked
//! with who sent it; the turn's final reply comes back to the service as `reply`, as plain text
//! for a channel that shows no Markdown; the same
//! message again is no second turn; a conversation outside the scope is refused; the service
//! lists its scope and starts a conversation in it. The service is a real Node.js program; only
//! the Provider is a fixture.
use super::*;
use provider_fixture::Scenario;

/// Runs its calls one after another once it is ready and logs each answer as `<name> <json>`.
const SERVICE: &str = r#"
import { createInterface } from 'node:readline';
const say = (message) => process.stdout.write(JSON.stringify(message) + '\n');
const log = (text) => say({ type: 'log', level: 'info', text });
const waiting = new Map();
let next = 1;
const call = (method, params) => new Promise((resolve) => {
  const id = next++;
  waiting.set(id, resolve);
  say({ type: 'request', id, method, params });
});
const shown = (answer) => JSON.stringify(answer.error ? { error: answer.error.code } : answer.result);
let replied;
const reply = new Promise((resolve) => { replied = resolve; });
const from = (message_id) => ({
  channel: 'qq-main', chat: { kind: 'private', id: 'friend' },
  user: { id: 'friend', name: '老王' }, message_id, text: '你好，在吗？',
});
async function run() {
  const first = await call('message.in', from('m-1'));
  log('first ' + shown(first));
  if (first.result?.accepted) log('reply ' + JSON.stringify(await reply));
  const made = { title: '机器人建的', dedupe_key: 'k-1' };
  log('made ' + shown(await call('session.new', made)));
  log('made_again ' + shown(await call('session.new', made)));
  log('again ' + shown(await call('message.in', from('m-1'))));
  log('outside ' + shown(await call('session.state', { session_id: 'OUTSIDE_SESSION' })));
  log('listed ' + shown(await call('sessions.list', {})));
  log('elsewhere ' + shown(await call('session.new', { workspace_id: 'OUTSIDE_WORKSPACE' })));
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
    replied(message);
  } else if (message.type === 'shutdown') {
    process.exit(0);
  }
});
"#;

pub(super) fn services(runtime: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services
}

/// What the service logged as `<name> <json>`.
pub(super) fn logged(runtime: &crate::RuntimeState, name: &str) -> Option<Value> {
    let (entries, _) = services(runtime).plugins.recent_logs("chat-bot", 200).expect("logs");
    let prefix = format!("{name} ");
    entries.iter().find_map(|entry| {
        let json = entry.text.strip_prefix(&prefix)?;
        Some(serde_json::from_str(json).expect("logged JSON"))
    })
}

pub(super) async fn mode_of(runtime: &crate::RuntimeState, session: &Value) -> Value {
    let state = crate::r03_tests::success(
        runtime,
        "permission.get_session_state",
        json!({"session_id": session}),
    )
    .await;
    state["mode"].clone()
}

/// The plugin added in development and trusted with `service` as its service, the service on, its
/// scope the workspace given and `friend` on the allowlist of its channel, all through
/// Settings → 插件's own routes.
pub(super) async fn chat_bot(
    host: &crate::r03_tests::Fixture,
    runtime: &crate::RuntimeState,
    scope: WorkspaceId,
    service: &str,
) {
    let settings = json!({"service": true, "scope": {"workspaces": [scope]},
        "allowed": [{"channel": "qq-main", "id": "friend", "name": "朋友"}]});
    chat_bot_with(host, runtime, settings, service).await;
}

/// As [`chat_bot`], with `settings` as what Settings → 插件 sets for it.
pub(super) async fn chat_bot_with(
    host: &crate::r03_tests::Fixture,
    runtime: &crate::RuntimeState,
    settings: Value,
    service: &str,
) {
    let folder = host.root.path().join("plugin-sources").join("chat-bot");
    std::fs::create_dir_all(&folder).expect("plugin folder");
    std::fs::write(
        folder.join("aura-plugin.toml"),
        "schema_version = 2\nname = \"chat-bot\"\nversion = \"0.1.0\"\ndescription = \"测试机器人\"\n\
         capabilities = [\"session.read\", \"session.send\", \"session.control\"]\n\n\
         [service]\ncommand = \"node\"\narguments = [\"${PLUGIN_ROOT}/service.mjs\"]\n\n\
         [[channels]]\nid = \"qq-main\"\nplatform = \"qq\"\ntitle = \"QQ 机器人\"\n",
    )
    .expect("manifest");
    std::fs::write(folder.join("service.mjs"), service).expect("service");
    let revision = |listed: &Value| listed["revision"].clone();
    let listed = crate::r03_tests::success(runtime, "agent.plugins.list", json!({})).await;
    crate::r03_tests::success(
        runtime,
        "agent.plugins.add_development",
        json!({"path": folder, "expected_revision": revision(&listed)}),
    )
    .await;
    let preview =
        crate::r03_tests::success(runtime, "agent.plugins.preview", json!({"name": "chat-bot"}))
            .await;
    let listed = crate::r03_tests::success(runtime, "agent.plugins.list", json!({})).await;
    crate::r03_tests::success(
        runtime,
        "agent.plugins.trust",
        json!({"name": "chat-bot", "expected_fingerprint": preview["fingerprint"],
            "expected_revision": revision(&listed)}),
    )
    .await;
    let listed = crate::r03_tests::success(runtime, "agent.plugins.list", json!({})).await;
    let mut configured = json!({"name": "chat-bot", "expected_revision": revision(&listed)});
    let fields = configured.as_object_mut().expect("an object");
    fields.extend(settings.as_object().expect("the settings").clone());
    crate::r03_tests::success(runtime, "agent.plugins.configure", configured).await;
}

#[tokio::test]
async fn l17_a_chat_message_goes_to_a_conversation_in_scope_and_its_turns_reply_comes_back() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, outside, model) = prepare(&host, &provider).await;
    let snapshot =
        crate::r03_tests::success(&runtime, "models.settings.get_snapshot", json!({})).await;
    crate::r03_tests::success(
        &runtime,
        "models.default.set",
        json!({"expected_revision": snapshot["aura_revision"], "model_profile_id": model}),
    )
    .await;
    let root = host.root.path().join("chats-workspace");
    std::fs::create_dir(&root).expect("workspace folder");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root": root, "display_name": "Chats"}),
    )
    .await;
    let chats: WorkspaceId =
        serde_json::from_value(created["workspace"]["workspace_id"].clone()).expect("workspace");
    let projection = services(&runtime).persistence.workspace_session_projection().expect("read");
    let elsewhere = projection.sessions.iter().find(|s| s.session_id == outside).expect("session");
    // New conversations start in the mode the user set as the default, whichever it is.
    let defaults = crate::r03_tests::success(&runtime, "permission.get_defaults", json!({})).await;
    let mode = if defaults["mode"] == "read_only" { "workspace_read_write" } else { "read_only" };
    crate::r03_tests::success(
        &runtime,
        "permission.set_defaults",
        json!({"expected_revision": defaults["revision"], "mode": mode}),
    )
    .await;
    let service = SERVICE
        .replace("OUTSIDE_SESSION", &outside.to_string())
        .replace("OUTSIDE_WORKSPACE", &elsewhere.workspace_id.to_string());
    chat_bot(&host, &runtime, chats, &service).await;

    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        while logged(&runtime, "done").is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the service ran all its calls");

    let first = logged(&runtime, "first").expect("first");
    assert_eq!(
        (first["accepted"].clone(), first["repeated"].clone()),
        (json!(true), json!(false)),
        "{first}"
    );
    let session: SessionId = serde_json::from_value(first["session_id"].clone()).expect("session");
    assert_ne!(session, outside, "the conversation outside the scope is not used");
    let projection = services(&runtime).persistence.workspace_session_projection().expect("read");
    let made = projection.sessions.iter().find(|s| s.session_id == session).expect("made");
    assert_eq!((made.workspace_id, made.title.as_str()), (chats, "QQ 机器人 · 老王"));
    assert_eq!(mode_of(&runtime, &first["session_id"]).await, mode);

    // The channel did not report Markdown, so the answer comes as plain text, in one piece.
    let reply = logged(&runtime, "reply").expect("reply");
    assert_eq!(
        [&reply["channel"], &reply["chat"], &reply["reply_to"], &reply["format"], &reply["parts"]],
        [
            &json!("qq-main"),
            &json!({"kind": "private", "id": "friend"}),
            &json!("m-1"),
            &json!("text"),
            &Value::Null
        ]
    );
    let history = crate::r03_tests::success(
        &runtime,
        "agent.history",
        json!({"session_id": session, "before_turn_task_id": null, "within_turn": null, "limit": 10}),
    )
    .await;
    let turns = history["turns"].as_array().expect("turns");
    assert_eq!(turns.len(), 1, "the same message again was no second turn");
    assert_eq!(turns[0]["turn"]["state"]["state"], "completed", "bound to the default model");
    let said = reply["text"].as_str().expect("the reply's text");
    assert!(!said.is_empty() && history.to_string().contains(said), "the turn's answer: {history}");
    let user = &turns[0]["user_messages"][0];
    assert_eq!(user["text"], "你好，在吗？");
    assert_eq!(
        user["sender"],
        json!({"plugin": "chat-bot", "from": {"channel": "qq-main", "platform": "qq",
            "chat": "private", "user_id": "friend", "user_name": "老王"}}),
        "{user}"
    );

    let new = logged(&runtime, "made").expect("made");
    assert_eq!(
        (new["workspace_id"].clone(), new["title"].clone()),
        (json!(chats), json!("机器人建的"))
    );
    assert_eq!(logged(&runtime, "made_again"), Some(new.clone()), "the same key, the same one");
    assert_eq!(mode_of(&runtime, &new["session_id"]).await, mode);
    // The chat stays in its conversation although a newer one is in the scope now.
    let again = logged(&runtime, "again").expect("again");
    assert_eq!(
        again,
        json!({"accepted": true, "session_id": session, "queued": false, "repeated": true})
    );
    let outside_scope = json!({"error": "plugin_session.out_of_scope"});
    assert_eq!(logged(&runtime, "outside"), Some(outside_scope.clone()));
    assert_eq!(logged(&runtime, "elsewhere"), Some(outside_scope));
    let listed = logged(&runtime, "listed").expect("listed");
    assert_eq!(listed["workspaces"], json!([{"workspace_id": chats, "name": "Chats"}]));
    assert_eq!(
        listed["sessions"],
        json!([
            {"session_id": new["session_id"], "workspace_id": chats, "title": "机器人建的"},
            {"session_id": session, "workspace_id": chats, "title": "QQ 机器人 · 老王"},
        ]),
        "the newest first"
    );
    assert_eq!(listed["more"], false);
}
