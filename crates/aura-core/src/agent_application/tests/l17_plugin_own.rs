//! What a plugin does with its own parts, through Core's own routes and its service's methods: one
//! of its prompt commands run in the conversation as its message, its text settings read and set
//! as the plugin page sets them, and a compaction told among the conversation's events. Each needs
//! what the plugin declared; a secret setting stays the plugin page's. Only the Provider is a
//! fixture; the service is a real Node.js program.
use super::l10_19_compaction::before_the_third;
use super::l17_plugin_channels::services;
use super::l17_plugin_session::{call, ended_turns, kinds, refused};
use super::*;
use provider_fixture::Scenario;

const PLUGIN: &str = "own-parts";

/// The plugin added in development and trusted through Settings → 插件's own routes: a panel, the
/// capabilities named, its prompt command `summary` and a text and a secret setting. With
/// `service`, that program is its background service, switched on, its scope `scope`.
async fn own_plugin(
    host: &crate::r03_tests::Fixture,
    runtime: &crate::RuntimeState,
    capabilities: &[&str],
    service: Option<(&str, WorkspaceId)>,
) {
    let folder = host.root.path().join("plugin-sources").join(PLUGIN);
    std::fs::create_dir_all(folder.join("panel")).expect("panel folder");
    std::fs::create_dir_all(folder.join("commands/summary")).expect("command folder");
    let declared = capabilities.iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>();
    let program = if service.is_some() {
        "\n[service]\ncommand = \"node\"\narguments = [\"${PLUGIN_ROOT}/service.mjs\"]\n"
    } else {
        ""
    };
    std::fs::write(
        folder.join("aura-plugin.toml"),
        format!(
            "schema_version = 2\nname = \"{PLUGIN}\"\nversion = \"0.1.0\"\n\
             description = \"自己的命令和设置\"\ncapabilities = [{}]\n\n[panel]\ntitle = \"自己的\"\n\
             {program}\n[[settings]]\nkey = \"greeting\"\ntitle = \"问候\"\n\n\
             [[settings]]\nkey = \"token\"\ntitle = \"令牌\"\nkind = \"secret\"\n",
            declared.join(", ")
        ),
    )
    .expect("manifest");
    std::fs::write(folder.join("panel/index.html"), "<!doctype html><title>自己的</title>")
        .expect("page");
    std::fs::write(
        folder.join("commands/summary/command.toml"),
        "name = \"summary\"\ndescription = \"总结改动\"\nargument_hint = \"<范围>\"\nversion = 1\n",
    )
    .expect("command");
    std::fs::write(folder.join("commands/summary/template.md"), "总结一下 $ARGUMENTS 的改动\n")
        .expect("template");
    if let Some((program, _)) = service {
        std::fs::write(folder.join("service.mjs"), program).expect("service");
    }
    let listed = crate::r03_tests::success(runtime, "agent.plugins.list", json!({})).await;
    crate::r03_tests::success(
        runtime,
        "agent.plugins.add_development",
        json!({"path": folder, "expected_revision": listed["revision"]}),
    )
    .await;
    let preview =
        crate::r03_tests::success(runtime, "agent.plugins.preview", json!({"name": PLUGIN})).await;
    let listed = crate::r03_tests::success(runtime, "agent.plugins.list", json!({})).await;
    crate::r03_tests::success(
        runtime,
        "agent.plugins.trust",
        json!({"name": PLUGIN, "expected_fingerprint": preview["fingerprint"],
            "expected_revision": listed["revision"]}),
    )
    .await;
    if let Some((_, scope)) = service {
        let listed = crate::r03_tests::success(runtime, "agent.plugins.list", json!({})).await;
        crate::r03_tests::success(
            runtime,
            "agent.plugins.configure",
            json!({"name": PLUGIN, "expected_revision": listed["revision"], "service": true,
                "scope": {"workspaces": [scope]}}),
        )
        .await;
    }
}

async fn bound(runtime: &crate::RuntimeState, session: SessionId, model: ModelProfileId) {
    crate::r03_tests::success(
        runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
}

fn setting(more: Value) -> Value {
    let mut request = json!({"plugin": PLUGIN});
    request.as_object_mut().expect("object").extend(more.as_object().expect("object").clone());
    request
}

/// Every entry the service logged as `<name> <json>`.
fn all_logged(runtime: &crate::RuntimeState, name: &str) -> Vec<Value> {
    let (entries, _) = services(runtime).plugins.recent_logs(PLUGIN, 200).expect("logs");
    let prefix = format!("{name} ");
    entries
        .iter()
        .filter_map(|entry| entry.text.strip_prefix(&prefix))
        .map(|json| serde_json::from_str(json).expect("logged JSON"))
        .collect()
}

#[tokio::test]
async fn l17_a_panel_runs_one_of_its_plugins_own_commands_as_the_plugins_message() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    bound(&runtime, session, model).await;
    own_plugin(&host, &runtime, &["commands"], None).await;

    let run = |name: &str| {
        call(
            PLUGIN,
            session,
            json!({"name": name, "arguments": "src/lib.rs", "dedupe_key": "run-1"}),
        )
    };
    let sent =
        crate::r03_tests::success(&runtime, "agent.plugin_session.run", run("summary")).await;
    assert_eq!(sent["repeated"], false, "{sent}");
    assert!(sent["message_id"].is_string(), "{sent}");
    let turns = ended_turns(&runtime, session).await;
    assert_eq!(turns[0]["turn"]["state"]["state"], "completed");
    let user = &turns[0]["user_messages"][0];
    // The template, with the arguments where it says so, sent as the plugin's.
    assert_eq!(user["text"], "总结一下 src/lib.rs 的改动", "{user}");
    assert_eq!(user["sender"], json!({"plugin": PLUGIN}), "{user}");

    // The same key again is the same message: no second turn.
    let again =
        crate::r03_tests::success(&runtime, "agent.plugin_session.run", run("summary")).await;
    assert_eq!(
        (again["repeated"].clone(), again["message_id"].clone()),
        (json!(true), json!(null))
    );
    let unknown = refused(&runtime, "agent.plugin_session.run", run("review")).await;
    assert!(unknown.starts_with("plugin_session.command_unknown"), "{unknown}");
    assert!(unknown.contains("/review"), "{unknown}");
    // It runs the plugin's own commands only: it is no way to send any other text.
    let send = refused(
        &runtime,
        "agent.plugin_session.send",
        call(PLUGIN, session, json!({"text": "hi"})),
    )
    .await;
    assert!(send.starts_with("plugin_session.capability_missing"), "{send}");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let shown = super::l17_plugin_session::history(&runtime, session).await;
    assert_eq!(shown["turns"].as_array().expect("turns").len(), 1);
}

#[tokio::test]
async fn l17_a_panel_reads_and_sets_its_plugins_text_settings_as_the_plugin_page_does() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, _) = prepare(&host, &provider).await;
    own_plugin(&host, &runtime, &["settings"], None).await;

    let none =
        crate::r03_tests::success(&runtime, "agent.plugin_session.settings", setting(json!({})))
            .await;
    assert_eq!(none, json!({"values": {}}));
    let set = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.set_setting",
        setting(json!({"key": "greeting", "value": " 你好 "})),
    )
    .await;
    assert_eq!(set, json!({"values": {"greeting": "你好"}}));
    let read =
        crate::r03_tests::success(&runtime, "agent.plugin_session.settings", setting(json!({})))
            .await;
    assert_eq!(read, set, "kept where the plugin page keeps it");
    // A secret setting is the plugin page's alone; a key the manifest does not declare and a
    // value of two lines are refused as the plugin page refuses them.
    for (key, value, code) in [
        ("token", "secret-value", "plugins.setting_secret"),
        ("ghost", "x", "plugins.setting_unknown"),
        ("greeting", "第一行\n第二行", "plugins.setting_invalid"),
    ] {
        let wrong = refused(
            &runtime,
            "agent.plugin_session.set_setting",
            setting(json!({"key": key, "value": value})),
        )
        .await;
        assert!(wrong.starts_with(code), "{key}: {wrong}");
    }
    let cleared = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.set_setting",
        setting(json!({"key": "greeting", "value": ""})),
    )
    .await;
    assert_eq!(cleared, json!({"values": {}}));
    // Its own commands need `commands`, which it did not declare.
    let run = refused(
        &runtime,
        "agent.plugin_session.run",
        call(PLUGIN, session, json!({"name": "summary"})),
    )
    .await;
    assert!(run.starts_with("plugin_session.capability_missing"), "{run}");
    assert!(run.contains("commands"), "{run}");
}

/// The events of a third turn whose history must be compacted before its third request, read from
/// before it began: two turns, then the window `l10_19_compaction` sets for the third.
async fn compacted_turn(provider: &Provider) -> Value {
    let host = crate::r03_tests::Fixture::new();
    let (runtime, session, model) = before_the_third(&host, provider, 64_000).await;
    own_plugin(&host, &runtime, &["session.events"], None).await;
    let now = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.events",
        call(PLUGIN, session, json!({})),
    )
    .await;
    let text = "AURA-L17-THIRD: answer at length";
    let third = super::l10_15_compaction::start(&runtime, session, model, text).await;
    wait(&runtime, &third, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.events",
        call(PLUGIN, session, json!({"after": now["cursor"]})),
    )
    .await
}

/// The compaction's place among the turn's events, and how each of its ends came out.
fn compaction_told(events: &Value, ends: usize) -> Vec<Value> {
    let mut expected = vec!["turn_started", "step_started", "step_ended", "step_started"];
    expected.extend(["step_ended", "compaction_started"]);
    expected.extend(std::iter::repeat_n("compaction_ended", ends));
    expected.extend(["step_started", "step_ended", "turn_ended"]);
    // It makes room for the third request; its own requests to the model are no steps.
    assert_eq!(kinds(events), expected, "{events}");
    let listed = events["events"].as_array().expect("events");
    let began = &listed[5]["event"];
    assert!(began["compaction_id"].is_string(), "{events}");
    listed[6..6 + ends]
        .iter()
        .map(|ended| {
            assert_eq!(ended["event"]["compaction_id"], began["compaction_id"], "{events}");
            ended["event"]["outcome"].clone()
        })
        .collect()
}

#[tokio::test]
async fn l17_a_panel_is_told_of_a_compaction_among_the_conversations_events() {
    let provider = Provider::scenario(Scenario::CompactOnPrefix).await;
    let events = compacted_turn(&provider).await;
    assert_eq!(compaction_told(&events, 1), [json!("completed")]);
}

#[tokio::test]
async fn l17_a_compaction_aura_completes_with_its_own_summary_is_told_failed_then_completed() {
    // Every compaction is answered far past its length, so both attempts fail.
    let provider = Provider::scenario_with_long_compactions(Scenario::CompactOnPrefix).await;
    let events = compacted_turn(&provider).await;
    assert_eq!(compaction_told(&events, 2), [json!("failed"), json!("completed")]);
}

/// Runs the plugin's command in `SESSION`, tries to set its secret setting, then sets its text
/// one; started again with the new value, it says so and does nothing more.
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
async function first() {
  const ran = await call('command.run', {
    session_id: 'SESSION', name: 'summary', arguments: '服务给的', dedupe_key: 'svc-1',
  });
  log(`ran ${shown(ran)}`);
  log(`secret ${shown(await call('settings.set', { key: 'token', value: 'secret-value' }))}`);
  log(`set ${shown(await call('settings.set', { key: 'greeting', value: '服务设的' }))}`);
}
createInterface({ input: process.stdin }).on('line', (line) => {
  const message = JSON.parse(line);
  if (message.type === 'hello') {
    say({ type: 'ready', protocol: 1, channels: [] });
    log(`hello ${JSON.stringify(message.settings)}`);
    if (!message.settings.greeting) void first();
  } else if (message.type === 'response') {
    const resolve = waiting.get(message.id);
    waiting.delete(message.id);
    resolve?.(message);
  } else if (message.type === 'shutdown') {
    process.exit(0);
  }
});
"#;

#[tokio::test]
async fn l17_a_service_runs_its_plugins_own_command_and_sets_its_own_setting() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    bound(&runtime, session, model).await;
    let projection = services(&runtime).persistence.workspace_session_projection().expect("read");
    let workspace = projection.sessions.iter().find(|s| s.session_id == session).expect("session");
    let program = SERVICE.replace("SESSION", &session.to_string());
    own_plugin(
        &host,
        &runtime,
        &["commands", "settings"],
        Some((&program, workspace.workspace_id)),
    )
    .await;

    // Started again with the value it set, as when the plugin page sets one.
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        while !all_logged(&runtime, "hello").contains(&json!({"greeting": "服务设的"})) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("started again: {:?}", all_logged(&runtime, "hello")));
    let ran = &all_logged(&runtime, "ran")[0];
    assert_eq!(ran["repeated"], false, "{ran}");
    assert_eq!(all_logged(&runtime, "secret")[0], json!({"error": "plugins.setting_secret"}));
    assert_eq!(all_logged(&runtime, "set")[0], json!({"values": {"greeting": "服务设的"}}));
    let turns = ended_turns(&runtime, session).await;
    let user = &turns[0]["user_messages"][0];
    assert_eq!(user["text"], "总结一下 服务给的 的改动", "{user}");
    assert_eq!(user["sender"], json!({"plugin": PLUGIN}), "{user}");
}
