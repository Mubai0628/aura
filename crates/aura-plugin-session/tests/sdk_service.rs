//! The plugin-writing Skill's helper libraries under Aura's own service host, copied into a
//! plugin's folder as the Skill tells the model to: the text settings from `hello` and from
//! `settings.get`, a request Aura has no method for refused with its code, a message from someone
//! not on the allowlist refused, and one from a friend with no conversation in the plugin's scope
//! answered by Aura's reply, handed to the service's handler. What the service prints reaches its
//! log, and turning its switch off reaches its shutdown handler. The Skill's example plugin,
//! copied out as its README says, does the same against its fake chat platform.
mod support;

use aura_plugin_session::{NO_SCOPE_REPLY, PluginServices};
use serde_json::{Value, json};
use std::sync::Arc;
use support::fake_platform::FakePlatform;
use support::{Fixture, eventually, sdk, skill_file};

const PYTHON: &str = r#"
import json
from aura_plugin import AuraError, AuraService

aura = AuraService()
aura.start([{"id": "qq-main", "max_text_chars": 2000}])
print("printed " + aura.setting("app_id", "none"))

@aura.on_reply
def replied(reply):
    aura.log("reply " + json.dumps(reply, ensure_ascii=False))

@aura.on_shutdown
def leaving():
    aura.log("bye")

def tried(name, call):
    try:
        aura.log(name + " ok " + json.dumps(call(), ensure_ascii=False))
    except AuraError as error:
        aura.log(name + " " + error.code)

tried("settings", aura.settings)
tried("widen", lambda: aura.call("sessions.configure", {"scope": "all"}))
tried("stranger", lambda: aura.message_in(
    "qq-main", "private", "stranger", "stranger", "m-1", "你好", user_name="老王"))
tried("friend", lambda: aura.message_in("qq-main", "private", "friend", "friend", "m-2", "在吗"))
aura.wait()
"#;

const NODE: &str = r#"
import { AuraError, connect } from './aura-plugin.mjs';
const aura = await connect({ channels: [{ id: 'qq-main', max_text_chars: 2000 }] });
console.log('printed ' + aura.setting('app_id', 'none'));
aura.onReply((reply) => aura.log('reply ' + JSON.stringify(reply)));
aura.onShutdown(() => aura.log('bye'));
const tried = async (name, call) => {
  try {
    aura.log(`${name} ok ${JSON.stringify(await call())}`);
  } catch (error) {
    aura.log(`${name} ${error instanceof AuraError ? error.code : error}`);
  }
};
const chat = (id) => ({ channel: 'qq-main', chat: { kind: 'private', id }, user: { id } });
await tried('settings', () => aura.settings());
await tried('widen', () => aura.call('sessions.configure', { scope: 'all' }));
await tried('stranger', () =>
  aura.messageIn({ ...chat('stranger'), user: { id: 'stranger', name: '老王' }, message_id: 'm-1', text: '你好' }),
);
await tried('friend', () => aura.messageIn({ ...chat('friend'), message_id: 'm-2', text: '在吗' }));
"#;

/// What the service logged for `name`: `ok <result>` or the error's code.
fn said(logged: &[String], name: &str) -> Option<String> {
    let prefix = format!("{name} ");
    logged.iter().find_map(|text| text.strip_prefix(&prefix).map(str::to_owned))
}

fn result(logged: &[String], name: &str) -> Value {
    let said = said(logged, name).unwrap_or_else(|| panic!("{name}: {logged:?}"));
    let result = said.strip_prefix("ok ").unwrap_or_else(|| panic!("{name}: {said}"));
    serde_json::from_str(result).expect("JSON")
}

async fn works_under_aura(command: &str, files: &[(&str, &str)]) {
    let f = Fixture::new();
    let manifest = format!(
        "capabilities = [\"session.send\", \"settings\"]\n\n{command}\n\
         [[settings]]\nkey = \"app_id\"\ntitle = \"AppID\"\n\n\
         [[channels]]\nid = \"qq-main\"\nplatform = \"qq\"\ntitle = \"QQ 机器人\"\n"
    );
    f.plugin_with("chat-bot", &manifest, files);
    let friend = json!([{"channel": "qq-main", "id": "friend", "name": "朋友"}]);
    let change = json!({"service": true, "text": {"app_id": "1024"}, "allowed": friend});
    f.configure("chat-bot", change);
    let services =
        PluginServices::start(Arc::clone(&f.plugins), &tokio::runtime::Handle::current());

    eventually("the friend answered and the reply handed over", || {
        let logged = f.logged("chat-bot");
        said(&logged, "friend").is_some() && said(&logged, "reply").is_some()
    })
    .await;
    let logged = f.logged("chat-bot");
    assert!(logged.iter().any(|text| text == "printed 1024"), "{logged:?}");
    assert_eq!(result(&logged, "settings"), json!({"app_id": "1024"}));
    assert_eq!(said(&logged, "widen").as_deref(), Some("protocol.unknown_method"));
    let stranger = result(&logged, "stranger");
    assert_eq!(stranger, json!({"accepted": false, "reason": "user_not_allowed"}));
    let unknown = f.view("chat-bot")["remote"]["unknown"].clone();
    assert_eq!([&unknown[0]["id"], &unknown[0]["name"]], [&json!("stranger"), &json!("老王")]);
    assert_eq!(result(&logged, "friend"), json!({"accepted": false, "reason": "no_scope"}));
    let reply: Value = serde_json::from_str(&said(&logged, "reply").expect("reply")).expect("JSON");
    assert_eq!(
        reply,
        json!({"type": "reply", "channel": "qq-main", "chat": {"kind": "private", "id": "friend"},
            "reply_to": "m-2", "text": NO_SCOPE_REPLY, "format": "text"})
    );

    f.configure("chat-bot", json!({"service": false}));
    eventually("its shutdown handler run", || f.logged("chat-bot").iter().any(|t| t == "bye"))
        .await;
    services.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_python_helper_works_under_aura() {
    let service =
        "[service]\ncommand = \"python\"\narguments = [\"${PLUGIN_ROOT}/service/main.py\"]\n";
    let helper = sdk("python/aura_plugin.py");
    works_under_aura(service, &[("service/main.py", PYTHON), ("service/aura_plugin.py", &helper)])
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_node_helper_works_under_aura() {
    let service =
        "[service]\ncommand = \"node\"\narguments = [\"${PLUGIN_ROOT}/service/main.mjs\"]\n";
    let helper = sdk("node/aura-plugin.mjs");
    works_under_aura(service, &[("service/main.mjs", NODE), ("service/aura-plugin.mjs", &helper)])
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_skills_example_plugin_answers_its_fake_platform() {
    let platform = FakePlatform::start();
    let f = Fixture::new();
    let example = |file: &str| skill_file(&format!("examples/fake-chat/plugin/{file}"));
    let (manifest, service, helper) =
        (example("aura-plugin.toml"), example("service/main.py"), sdk("python/aura_plugin.py"));
    let files = [
        ("aura-plugin.toml", manifest.as_str()),
        ("service/main.py", service.as_str()),
        ("service/aura_plugin.py", helper.as_str()),
    ];
    f.plugin_files("fake-chat", &files);
    let friend = json!([{"channel": "fake-main", "id": "u1", "name": "老王"}]);
    let address = format!("http://127.0.0.1:{}", platform.port);
    let change = json!({"service": true, "text": {"address": address}, "allowed": friend});
    f.configure("fake-chat", change);
    let services =
        PluginServices::start(Arc::clone(&f.plugins), &tokio::runtime::Handle::current());

    let sent = || platform.call("GET", "/sent", None)["sent"].as_array().cloned();
    platform.push("stranger", "你好");
    platform.push("u1", "在吗");
    eventually("the friend told where to give it a conversation", || {
        sent().is_some_and(|sent| !sent.is_empty())
    })
    .await;
    // A message read twice would be answered twice.
    platform.push("u1", "还在吗");
    eventually("the next one answered", || sent().is_some_and(|sent| sent.len() > 1)).await;
    let told = |id: &str| json!({"chat": {"type": "private", "id": "u1"}, "reply_to": id, "text": NO_SCOPE_REPLY});
    assert_eq!(sent(), Some(vec![told("m-2"), told("m-3")]));
    let logged = f.logged("fake-chat");
    let answered = |id: &str| -> Value {
        let prefix = format!("消息 {id}：");
        let line = logged.iter().find_map(|text| text.strip_prefix(&prefix));
        serde_json::from_str(line.unwrap_or_else(|| panic!("{id}: {logged:?}"))).expect("JSON")
    };
    assert_eq!(answered("m-1"), json!({"accepted": false, "reason": "user_not_allowed"}));
    assert_eq!(answered("m-2"), json!({"accepted": false, "reason": "no_scope"}));
    services.stop();
}
