//! A chat's `/` commands through Core's own pieces. The chat has the composer's commands, the user's
//! and the plugins' prompt commands and those of its conversation's workspace, and the ones only a
//! chat has; each is answered in the chat; a prompt command goes to the conversation as its
//! template says; a name that is no command goes as it was typed; the turns a command starts are
//! marked with who sent them; widening the permission mode stays on the computer; a command message
//! counts once; and a channel that shows a menu is sent one. The service is a real Node.js program;
//! only the Provider is a fixture.
use super::l17_plugin_channels::{chat_bot, logged, mode_of, services};
use super::*;
use provider_fixture::Scenario;

/// Sends each step's message once it is ready, after the reply to the step before, and logs the
/// answer as `answer <label> <json>` and the reply as `reply <label> <json>`. `SECOND` becomes the
/// number `/model` listed the second model under.
const SERVICE: &str = r#"
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
const shown = (answer) => JSON.stringify(answer.error ? { error: answer.error.code } : answer.result);
// The label, the text, and the platform's id of the message when it is not the label.
const STEPS = [
  ['help', '/help'],
  ['status-none', '/status'],
  ['hello', '你好'],
  ['help-2', '/help'],
  ['status', '/status'],
  ['models', '/model'],
  ['model', '/model SECOND'],
  ['modes', '/permission'],
  ['stricter', '/permission 1'],
  ['wider', '/permission 3'],
  ['stop', '/stop'],
  ['approve-off', '/approve'],
  ['context', '/context'],
  ['review', '/review src/lib.rs'],
  ['foo', '/foo 文字'],
  ['compact', '/compact 只留结论'],
  ['init', '/init 只看 crates'],
  ['new', '/new 第二个'],
  ['sessions', '/sessions'],
  ['switch', '/switch 2'],
  ['switched', '/status'],
  ['new-plan', '/new'],
  ['plan', '/plan 写一个方案'],
  ['new-goal', '/new'],
  ['goal', '/goal 做完它'],
  ['again', '/help', 'help'],
];
async function run() {
  let second = '?';
  for (const [label, text, id = label] of STEPS) {
    const reply = new Promise((resolve) => replies.set(id, resolve));
    const answer = await call('message.in', {
      channel: 'qq-main', chat: { kind: 'private', id: 'friend' },
      user: { id: 'friend', name: '老王' }, message_id: id, text: text.replace('SECOND', second),
    });
    log('answer ' + label + ' ' + shown(answer));
    if (answer.result?.accepted && !answer.result.repeated) {
      const said = await reply;
      log('reply ' + label + ' ' + JSON.stringify(said));
      const listed = said.text.match(/^(\d+)\. Second fixture/m);
      if (listed) second = listed[1];
    }
  }
  log('done {}');
}
createInterface({ input: process.stdin }).on('line', (line) => {
  const message = JSON.parse(line);
  if (message.type === 'hello') {
    say({ type: 'ready', protocol: 1,
      channels: [{ id: 'qq-main', max_text_chars: 2000, command_menu: true }] });
    run();
  } else if (message.type === 'response') {
    waiting.get(message.id)?.(message);
  } else if (message.type === 'reply') {
    replies.get(message.reply_to)?.(message);
  } else if (message.type === 'commands') {
    log('menu ' + JSON.stringify(message));
  } else if (message.type === 'shutdown') {
    process.exit(0);
  }
});
"#;

/// A prompt command's folder, as the user or a workspace keeps it.
fn prompt_command(root: &std::path::Path, name: &str, template: &str) {
    let folder = root.join(name);
    std::fs::create_dir_all(&folder).expect("command folder");
    std::fs::write(
        folder.join("command.toml"),
        format!(
            "name = \"{name}\"\ndescription = \"测试命令\"\nargument_hint = \"范围\"\nversion = 1\n"
        ),
    )
    .expect("command manifest");
    std::fs::write(folder.join("template.md"), template).expect("command template");
}

/// The conversation's turns, oldest first, each as its mode and its first message.
async fn turns_of(runtime: &crate::RuntimeState, session: &Value) -> Vec<(Value, Value)> {
    let history = crate::r03_tests::success(
        runtime,
        "agent.history",
        json!({"session_id": session, "before_turn_task_id": null, "within_turn": null, "limit": 10}),
    )
    .await;
    let turns = history["turns"].as_array().expect("turns");
    turns
        .iter()
        .map(|turn| (turn["turn"]["mode"].clone(), turn["user_messages"][0].clone()))
        .collect()
}

#[tokio::test]
async fn l17_a_chats_commands_are_the_composers_and_those_only_a_chat_has() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, _, model) = prepare(&host, &provider).await;
    let second = profile_after_existing(&runtime, &provider.url).await;
    let snapshot =
        crate::r03_tests::success(&runtime, "models.settings.get_snapshot", json!({})).await;
    crate::r03_tests::success(
        &runtime,
        "models.default.set",
        json!({"expected_revision": snapshot["aura_revision"], "model_profile_id": model}),
    )
    .await;
    let defaults = crate::r03_tests::success(&runtime, "permission.get_defaults", json!({})).await;
    if defaults["mode"] != "workspace_read_write" {
        crate::r03_tests::success(
            &runtime,
            "permission.set_defaults",
            json!({"expected_revision": defaults["revision"], "mode": "workspace_read_write"}),
        )
        .await;
    }
    let root = host.root.path().join("chats-workspace");
    prompt_command(&root.join(".aura").join("commands"), "deploy", "部署 $ARGUMENTS");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root": root, "display_name": "Chats"}),
    )
    .await;
    let chats: WorkspaceId =
        serde_json::from_value(created["workspace"]["workspace_id"].clone()).expect("workspace");
    let user_commands = services(&runtime)._settings.paths().config_root.join("commands");
    prompt_command(&user_commands, "review", "审查 $ARGUMENTS 的改动");
    chat_bot(&host, &runtime, chats, SERVICE).await;

    tokio::time::timeout(std::time::Duration::from_secs(240), async {
        while logged(&runtime, "done").is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the service sent every step");

    let answer = |label: &str| {
        logged(&runtime, &format!("answer {label}")).unwrap_or_else(|| panic!("answer {label}"))
    };
    let reply = |label: &str| -> String {
        let reply =
            logged(&runtime, &format!("reply {label}")).unwrap_or_else(|| panic!("reply {label}"));
        assert_eq!(
            [&reply["channel"], &reply["chat"], &reply["reply_to"], &reply["format"]],
            [
                &json!("qq-main"),
                &json!({"kind": "private", "id": "friend"}),
                &json!(label),
                &json!("text")
            ],
            "{reply}"
        );
        reply["text"].as_str().expect("the reply's text").to_owned()
    };
    let command = |label: &str, name: &str| {
        let answer = answer(label);
        assert_eq!(
            (&answer["accepted"], &answer["command"], &answer["repeated"]),
            (&json!(true), &json!(name), &json!(false)),
            "{label}: {answer}"
        );
        answer["session_id"].clone()
    };

    // Before the chat has a conversation.
    assert_eq!(command("help", "help"), Value::Null);
    let help = reply("help");
    for listed in ["/plan 要做的事 — Plan 模式", "/sessions", "/new", "/status", "/review 范围"]
    {
        assert!(help.contains(listed), "{listed}: {help}");
    }
    // The decisions on approvals are listed only while the plugin's chats may decide them.
    for missing in ["/context", "/tools", "/deploy", "/approve", "/deny"] {
        assert!(!help.contains(missing), "{missing}: {help}");
    }
    assert_eq!(command("status-none", "status"), Value::Null);
    assert_eq!(reply("status-none"), "这个聊天还没有会话，发一条消息就会新建一个。");

    let first = answer("hello");
    assert_eq!((&first["accepted"], &first["command"]), (&json!(true), &Value::Null), "{first}");
    let session = first["session_id"].clone();
    assert!(!reply("hello").is_empty());
    // The conversation's workspace has a command of its own: `/help` names it, the menu does not.
    assert_eq!(command("help-2", "help"), session);
    assert!(reply("help-2").contains("/deploy 范围"));

    assert_eq!(command("status", "status"), session);
    let status = reply("status");
    for shown in ["QQ 机器人 · 老王", "Chats", "Fixture", "工作区读写", "空闲"] {
        assert!(status.contains(shown), "{shown}: {status}");
    }
    command("models", "model");
    let models = reply("models");
    assert!(models.contains("Fixture（当前）") && models.contains("Second fixture"), "{models}");
    command("model", "model");
    assert_eq!(reply("model"), "已切换到模型 Second fixture。");
    let id: SessionId = serde_json::from_value(session.clone()).expect("session");
    let bound = services(&runtime).persistence.session_model_binding(id).expect("binding");
    assert_eq!(bound.map(|binding| binding.model_profile_id), Some(second));

    command("modes", "permission");
    let modes = reply("modes");
    for listed in ["1. 只读", "2. 工作区读写（当前）", "3. 完全访问"] {
        assert!(modes.contains(listed), "{listed}: {modes}");
    }
    command("stricter", "permission");
    assert_eq!(reply("stricter"), "权限已切换到 只读。");
    command("wider", "permission");
    assert_eq!(reply("wider"), "放宽权限要在电脑上的 Aura 里切。");
    assert_eq!(mode_of(&runtime, &session).await, "read_only");
    command("stop", "stop");
    assert_eq!(reply("stop"), "现在没有在运行的任务。");
    command("approve-off", "approve");
    assert_eq!(
        reply("approve-off"),
        "这个插件没有打开远程批准；在 Aura 的 设置 → 插件 里可以打开。"
    );
    command("context", "context");
    assert_eq!(reply("context"), "/context 要在电脑上的 Aura 里用。");

    // A prompt command goes as its template says, and its turn's answer comes back.
    let review = answer("review");
    assert_eq!(
        (&review["command"], &review["session_id"], &review["repeated"]),
        (&json!("review"), &session, &json!(false)),
        "{review}"
    );
    assert!(!reply("review").is_empty());
    let foo = answer("foo");
    assert_eq!((&foo["command"], &foo["session_id"]), (&Value::Null, &session), "{foo}");
    assert!(!reply("foo").is_empty());
    assert_eq!(command("compact", "compact"), session);
    assert!(!reply("compact").is_empty());
    assert_eq!(command("init", "init"), session);
    assert!(!reply("init").is_empty());
    let sender = json!({"plugin": "chat-bot", "from": {"channel": "qq-main", "platform": "qq",
        "chat": "private", "user_id": "friend", "user_name": "老王"}});
    let texts: Vec<(Value, Value)> = turns_of(&runtime, &session)
        .await
        .into_iter()
        .map(|(_, message)| (message["text"].clone(), message["sender"].clone()))
        .collect();
    assert_eq!(
        texts,
        [
            "你好",
            "审查 src/lib.rs 的改动",
            "/foo 文字",
            "/compact\n\n只留结论",
            "/init\n\n只看 crates"
        ]
        .map(|text| (json!(text), sender.clone()))
    );

    // Conversations the chat makes and goes between.
    let made = command("new", "new");
    assert_ne!(made, session);
    assert_eq!(reply("new"), "已新建会话 第二个，这个聊天已换过去。");
    assert_eq!(command("sessions", "sessions"), made);
    // Only the scope's: the conversation `prepare` made in another workspace is not listed.
    let listed = reply("sessions");
    assert_eq!(
        listed,
        "会话（/switch 编号 切换）：\n1. 第二个 · Chats（当前）\n2. QQ 机器人 · 老王 · Chats"
    );
    assert_eq!(command("switch", "switch"), session);
    assert_eq!(reply("switch"), "已切换到会话 QQ 机器人 · 老王。");
    assert_eq!(command("switched", "status"), session);
    assert!(reply("switched").starts_with("会话：QQ 机器人 · 老王\n"));

    // `/plan` and `/goal` start a turn in their mode in an idle conversation, marked as the chat's.
    for (made, label, mode, text) in
        [("new-plan", "plan", "plan", "写一个方案"), ("new-goal", "goal", "goal", "做完它")]
    {
        let made = command(made, "new");
        assert_eq!(command(label, mode), made);
        assert!(!reply(label).is_empty());
        let turns = turns_of(&runtime, &made).await;
        assert_eq!(turns.len(), 1, "{label}: {turns:?}");
        assert_eq!(
            (&turns[0].0, &turns[0].1["text"], &turns[0].1["sender"]),
            (&json!(mode), &json!(text), &sender),
            "{label}"
        );
    }

    // The same message from the platform again is not run again.
    assert_eq!(answer("again"), json!({"accepted": true, "command": "help", "repeated": true}));
    // Sent once: the commands did not change while the service ran.
    let (entries, _) = services(&runtime).plugins.recent_logs("chat-bot", 200).expect("logs");
    let menus = entries.iter().filter(|entry| entry.text.starts_with("menu ")).count();
    assert_eq!(menus, 1);
    let menu = logged(&runtime, "menu").expect("the channel shows a menu");
    assert_eq!(menu["channel"], "qq-main");
    let names: Vec<&str> = menu["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .map(|command| command["name"].as_str().expect("name"))
        .collect();
    assert_eq!(
        names,
        [
            "plan",
            "goal",
            "init",
            "model",
            "permission",
            "stop",
            "compact",
            "help",
            "sessions",
            "switch",
            "new",
            "status",
            "review"
        ]
    );
    assert_eq!(
        menu["commands"][0],
        json!({"name": "plan", "title": "Plan 模式",
            "description": "按 Plan 模式开始一轮：先梳理方案，由你决定何时实施",
            "argument_hint": "要做的事"})
    );
}
