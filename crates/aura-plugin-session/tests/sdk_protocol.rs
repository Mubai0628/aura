//! The helper libraries the plugin-writing Skill gives a background service, in Python and
//! Node.js, against a fake Aura that plays the protocol line by line: the handshake, at most 16
//! requests open at once, answers matched by id whatever their order, a refusal as an error with
//! its code, a request given up after its time, a line of a kind it does not know skipped, a
//! reply, a command menu and an approval handed to their handlers (what comes before its handler
//! is registered waits for it, in order), and leaving by itself on `shutdown`: a request still
//! open then and one asked for afterwards fail without reaching Aura. Nothing but protocol lines
//! reaches standard output. Their text splitting is checked on the way.
mod support;

use serde_json::{Value, json};
use std::io::Read;
use std::time::Duration;
use support::fake_aura::FakeAura;

const PYTHON: &str = r#"
import json, threading
from aura_plugin import AuraError, AuraService, split_text

aura = AuraService()
aura.start([{"id": "qq-main", "max_text_chars": 2000}])
print("not a protocol line")
splits = [("一二\n\n三四五六七", 4), ("a\n\nb\nc", 5), ("ab\ncde", 4), ("", 5), ("😀😀😀\n\nab", 6)]
aura.log("split " + json.dumps([split_text(text, most) for text, most in splits]))
answers = {}

def ask(n):
    try:
        answers[n] = aura.call("echo", {"n": n})
    except AuraError as error:
        answers[n] = error.code

threads = [threading.Thread(target=ask, args=(n,)) for n in range(20)]
for thread in threads:
    thread.start()
for thread in threads:
    thread.join()
aura.log("answers " + json.dumps([answers[n] for n in range(20)]))
try:
    aura.call("slow", timeout=0.5)
    aura.log("slow answered")
except TimeoutError:
    aura.log("slow timed out")

@aura.on_reply
def got(reply):
    aura.log("reply " + reply["text"])

@aura.on_commands
def menu(menu):
    names = ",".join(command["name"] for command in menu["commands"])
    aura.log("commands " + menu["channel"] + " " + names)

@aura.on_approval
def asked(asked):
    offer = asked["approval"]
    aura.log("approval " + asked["chat"]["id"] + " " + offer["id"] + " " + ",".join(offer["decisions"]))

@aura.on_shutdown
def bye():
    try:
        aura.call("late")
        aura.log("late answered")
    except AuraError as error:
        aura.log("late " + error.code)
    aura.log("bye")

try:
    aura.call("pending")
    aura.log("pending answered")
except AuraError as error:
    aura.log("pending " + error.code)
aura.wait()
aura.log("ended")
"#;

const NODE: &str = r#"
import { AuraError, connect, splitText } from './aura-plugin.mjs';
const aura = await connect({ channels: [{ id: 'qq-main', max_text_chars: 2000 }] });
aura.ended.then(() => aura.log('ended'));
console.log('not a protocol line');
const splits = [['一二\n\n三四五六七', 4], ['a\n\nb\nc', 5], ['ab\ncde', 4], ['', 5], ['😀😀😀\n\nab', 6]];
aura.log('split ' + JSON.stringify(splits.map(([text, most]) => splitText(text, most))));
const answers = await Promise.all(
  Array.from({ length: 20 }, (_, n) =>
    aura.call('echo', { n }).catch((error) => (error instanceof AuraError ? error.code : String(error))),
  ),
);
aura.log('answers ' + JSON.stringify(answers));
const slow = await aura.call('slow', {}, { timeoutMs: 500 }).then(
  () => 'answered',
  (error) => (error.name === 'TimeoutError' ? 'timed out' : String(error)),
);
aura.log('slow ' + slow);
aura.onReply((reply) => aura.log('reply ' + reply.text));
aura.onCommands((menu) =>
  aura.log('commands ' + menu.channel + ' ' + menu.commands.map((command) => command.name).join(',')),
);
aura.onApproval((asked) =>
  aura.log('approval ' + asked.chat.id + ' ' + asked.approval.id + ' ' + asked.approval.decisions.join(',')),
);
aura.onShutdown(async () => {
  const late = await aura.call('late').then(() => 'answered', (error) => error.code);
  aura.log('late ' + late);
  aura.log('bye');
});
const pending = await aura.call('pending').then(() => 'answered', (error) => error.code);
aura.log('pending ' + pending);
"#;

/// Services that register their handlers only once Aura answered their first request.
const PYTHON_LATE: &str = r#"
from aura_plugin import AuraService

aura = AuraService()
aura.start([{"id": "qq-main", "max_text_chars": 2000, "command_menu": True}])
aura.call("go")

@aura.on_commands
def menu(menu):
    aura.log("commands " + ",".join(command["name"] for command in menu["commands"]))

@aura.on_approval
def asked(asked):
    aura.log("approval " + asked["approval"]["id"])

aura.wait()
"#;

const NODE_LATE: &str = r#"
import { connect } from './aura-plugin.mjs';
const aura = await connect({ channels: [{ id: 'qq-main', max_text_chars: 2000, command_menu: true }] });
await aura.call('go');
aura.onCommands((menu) => aura.log('commands ' + menu.commands.map((command) => command.name).join(',')));
aura.onApproval((asked) => aura.log('approval ' + asked.approval.id));
"#;

fn plays_the_protocol(mut aura: FakeAura) {
    aura.send(&json!({"type": "hello", "protocol": 1, "plugin": "chat-bot",
        "channels": [{"id": "qq-main", "platform": "qq", "title": "QQ 机器人"}], "settings": {}}));
    let ready = aura.next("ready");
    assert_eq!(
        ready,
        json!({"type": "ready", "protocol": 1, "channels": [{"id": "qq-main", "max_text_chars": 2000}]})
    );
    let split = aura.logged();
    let split: Value =
        serde_json::from_str(split.strip_prefix("split ").expect("split")).expect("JSON");
    assert_eq!(
        split,
        json!([["一二", "三四五六", "七"], ["a", "b\nc"], ["ab", "cde"], [""], ["😀😀😀", "ab"]])
    );
    let mut open = Vec::new();
    for _ in 0..16 {
        let request = aura.next("a request");
        assert_eq!((&request["type"], &request["method"]), (&json!("request"), &json!("echo")));
        open.push(request);
    }
    aura.quiet_for(Duration::from_millis(500));
    // Answered in the reverse order; one is refused.
    for request in open.iter().rev() {
        let n = request["params"]["n"].as_u64().expect("n");
        let answer = if n == 3 {
            json!({"type": "response", "id": request["id"],
                "error": {"code": "plugin_session.capability_missing", "message": "没有声明"}})
        } else {
            json!({"type": "response", "id": request["id"], "result": {"twice": n * 2}})
        };
        aura.send(&answer);
    }
    for _ in 0..4 {
        let request = aura.next("the rest of the requests");
        let n = request["params"]["n"].as_u64().expect("n");
        aura.send(&json!({"type": "response", "id": request["id"], "result": {"twice": n * 2}}));
    }
    let mut expected: Vec<Value> = (0..20).map(|n| json!({"twice": n * 2})).collect();
    expected[3] = json!("plugin_session.capability_missing");
    let answers = aura.logged();
    let answers = answers.strip_prefix("answers ").unwrap_or_else(|| panic!("{answers}"));
    let answers: Value = serde_json::from_str(answers).expect("JSON");
    assert_eq!(answers, Value::from(expected));
    let slow = aura.next("a request it stops waiting for");
    assert_eq!(slow["method"], "slow");
    assert_eq!(aura.logged(), "slow timed out");
    let pending = aura.next("a request still open when Aura leaves");
    assert_eq!(pending["method"], "pending");
    aura.send(&json!({"type": "response", "id": slow["id"], "result": {}}));

    aura.send(&json!({"type": "something_new", "detail": 1}));
    aura.send(&json!({"type": "reply", "channel": "qq-main",
        "chat": {"kind": "private", "id": "u1"}, "reply_to": "m-1", "text": "在的",
        "format": "markdown"}));
    assert_eq!(aura.logged(), "reply 在的");
    aura.send(&json!({"type": "commands", "channel": "qq-main", "commands": [
        {"name": "plan", "title": "Plan 模式", "description": "按 Plan 模式开始一轮",
         "argument_hint": "要做的事"},
        {"name": "help", "title": "命令帮助", "description": "列出命令"}]}));
    assert_eq!(aura.logged(), "commands qq-main plan,help");
    aura.send(&json!({"type": "approval", "channel": "qq-main",
        "chat": {"kind": "private", "id": "u1"},
        "approval": {"id": "3", "title": "写入文件", "detail": "写结果\nagent-result.txt",
            "decisions": ["approve", "approve_session", "deny"]},
        "text": "Aura 请你决定（编号 3）：写入文件"}));
    assert_eq!(aura.logged(), "approval u1 3 approve,approve_session,deny");
    aura.send(&json!({"type": "shutdown"}));
    // The open request and the shutdown handler end on threads of their own; waiting for the
    // end lasts until the handler is done.
    let leaving: Vec<String> = (0..4).map(|_| aura.logged()).collect();
    let at = |text: &str| {
        leaving.iter().position(|line| line == text).unwrap_or_else(|| panic!("{leaving:?}"))
    };
    at("pending aura.stopping");
    assert!(at("late aura.stopping") < at("bye"), "nothing is sent once Aura left: {leaving:?}");
    assert!(at("bye") < at("ended"), "{leaving:?}");
    let status = aura.ended();
    assert!(status.success(), "it leaves by itself: {status}");
    let mut errors = String::new();
    aura.child.stderr.take().expect("stderr").read_to_string(&mut errors).expect("stderr");
    assert!(errors.contains("not a protocol line"), "{errors}");
}

#[test]
fn the_python_helper_plays_the_protocol() {
    plays_the_protocol(FakeAura::start("python", PYTHON, "service.py", "python/aura_plugin.py"));
}

#[test]
fn the_node_helper_plays_the_protocol() {
    plays_the_protocol(FakeAura::start("node", NODE, "service.mjs", "node/aura-plugin.mjs"));
}

/// Aura sends the menu as soon as the service is ready, and an approval may follow at once:
/// both wait for the handler the service registers later, and come in the order they were sent.
fn holds_what_comes_before_its_handler(mut aura: FakeAura) {
    aura.send(&json!({"type": "hello", "protocol": 1, "plugin": "chat-bot",
        "channels": [{"id": "qq-main", "platform": "qq", "title": "QQ 机器人"}], "settings": {}}));
    assert_eq!(aura.next("ready")["type"], "ready");
    let go = aura.request("the service's first request");
    aura.send(&json!({"type": "commands", "channel": "qq-main", "commands": [
        {"name": "plan", "title": "Plan 模式", "description": "按 Plan 模式开始一轮"}]}));
    for id in ["1", "2"] {
        aura.send(&json!({"type": "approval", "channel": "qq-main",
            "chat": {"kind": "private", "id": "u1"},
            "approval": {"id": id, "title": "写入文件", "detail": "", "decisions": ["deny"]},
            "text": "Aura 请你决定"}));
    }
    aura.send(&json!({"type": "response", "id": go["id"], "result": {}}));
    let logged: Vec<String> = (0..3).map(|_| aura.logged()).collect();
    assert_eq!(logged, ["commands plan", "approval 1", "approval 2"]);
    aura.send(&json!({"type": "shutdown"}));
    let status = aura.ended();
    assert!(status.success(), "{status}");
}

#[test]
fn the_python_helper_holds_what_comes_before_its_handler() {
    let aura = FakeAura::start("python", PYTHON_LATE, "service.py", "python/aura_plugin.py");
    holds_what_comes_before_its_handler(aura);
}

#[test]
fn the_node_helper_holds_what_comes_before_its_handler() {
    let aura = FakeAura::start("node", NODE_LATE, "service.mjs", "node/aura-plugin.mjs");
    holds_what_comes_before_its_handler(aura);
}
