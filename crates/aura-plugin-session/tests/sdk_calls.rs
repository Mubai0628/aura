//! The helper libraries' calls for a plugin's own parts, named as the protocol names them: one of
//! its prompt commands run in a conversation, with arguments and a key only when given, and one of
//! its text settings set, answered with the values as `settings` gives them.
mod support;

use serde_json::{Value, json};
use support::fake_aura::FakeAura;

const PYTHON: &str = r#"
import json
from aura_plugin import AuraService

aura = AuraService()
aura.start([])
aura.log("ran " + json.dumps(aura.run_command("ses_1", "summary", "src/lib.rs", dedupe_key="k-1")))
aura.log("plain " + json.dumps(aura.run_command("ses_1", "summary")))
aura.log("set " + json.dumps(aura.set_setting("greeting", "你好")))
aura.wait()
"#;

const NODE: &str = r#"
import { connect } from './aura-plugin.mjs';
const aura = await connect({ channels: [] });
aura.log('ran ' + JSON.stringify(await aura.runCommand('ses_1', 'summary', 'src/lib.rs', 'k-1')));
aura.log('plain ' + JSON.stringify(await aura.runCommand('ses_1', 'summary')));
aura.log('set ' + JSON.stringify(await aura.setSetting('greeting', '你好')));
"#;

/// The next request, answered with `result`, and what the service logged of the answer.
fn answered(aura: &mut FakeAura, what: &str, result: &Value) -> (Value, Value) {
    let request = aura.request(what);
    aura.send(&json!({"type": "response", "id": request["id"], "result": result}));
    let logged = aura.logged();
    let (_, shown) = logged.split_once(' ').unwrap_or_else(|| panic!("{logged}"));
    (
        json!({"method": request["method"], "params": request["params"]}),
        serde_json::from_str(shown).expect("JSON"),
    )
}

fn names_its_calls(mut aura: FakeAura) {
    aura.send(&json!({"type": "hello", "protocol": 1, "plugin": "own-parts", "channels": [],
        "settings": {}}));
    assert_eq!(aura.next("ready")["type"], "ready");
    let sent = json!({"message_id": "evt_1", "queued": false, "repeated": false});
    let (ran, shown) = answered(&mut aura, "the command run", &sent);
    assert_eq!(
        ran,
        json!({"method": "command.run", "params": {"session_id": "ses_1", "name": "summary",
            "arguments": "src/lib.rs", "dedupe_key": "k-1"}})
    );
    assert_eq!(shown, sent);
    let (plain, _) = answered(&mut aura, "the command run with nothing more", &sent);
    assert_eq!(
        plain,
        json!({"method": "command.run", "params": {"session_id": "ses_1", "name": "summary"}})
    );
    let values = json!({"values": {"greeting": "你好"}});
    let (set, shown) = answered(&mut aura, "the setting set", &values);
    assert_eq!(
        set,
        json!({"method": "settings.set", "params": {"key": "greeting", "value": "你好"}})
    );
    // The values afterwards, as `settings` gives them.
    assert_eq!(shown, values["values"]);
    aura.send(&json!({"type": "shutdown"}));
    let status = aura.ended();
    assert!(status.success(), "{status}");
}

#[test]
fn the_python_helper_names_its_calls_as_the_protocol_does() {
    names_its_calls(FakeAura::start("python", PYTHON, "service.py", "python/aura_plugin.py"));
}

#[test]
fn the_node_helper_names_its_calls_as_the_protocol_does() {
    names_its_calls(FakeAura::start("node", NODE, "service.mjs", "node/aura-plugin.mjs"));
}
