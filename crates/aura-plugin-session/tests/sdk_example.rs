//! The Skill's example plugin as its README copies it out, run as its plugin would run it with a
//! fake Aura on its standard streams and the Skill's fake platform: an approval Aura hands it goes
//! to the private chat as its whole text, the platform having no buttons, and the person's answer
//! comes back to Aura as an ordinary message, for Aura to read the command in.
mod support;

use serde_json::{Value, json};
use support::fake_aura::FakeAura;
use support::fake_platform::FakePlatform;
use support::skill_file;

#[test]
fn the_skills_example_hands_an_approval_to_its_chat_and_the_answer_back_to_aura() {
    let platform = FakePlatform::start();
    let example = skill_file("examples/fake-chat/plugin/service/main.py");
    let mut aura = FakeAura::start("python", &example, "main.py", "python/aura_plugin.py");
    let address = format!("http://127.0.0.1:{}", platform.port);
    aura.send(&json!({"type": "hello", "protocol": 1, "plugin": "fake-chat",
        "channels": [{"id": "fake-main", "platform": "other", "title": "假聊天"}],
        "settings": {"address": address}}));
    assert_eq!(aura.next("ready")["type"], "ready");

    let text = "Aura 请你决定（编号 1）：写入文件\n写结果\nresult.txt\n\
                回复 /approve 批准这一次，/approve-session 本会话都允许，/deny 拒绝。";
    aura.send(&json!({"type": "approval", "channel": "fake-main",
        "chat": {"kind": "private", "id": "u1"},
        "approval": {"id": "1", "title": "写入文件", "detail": "写结果\nresult.txt",
            "decisions": ["approve", "approve_session", "deny"]},
        "text": text}));
    let sent = platform.sent_at_least(1);
    assert_eq!(sent, [json!({"chat": {"type": "private", "id": "u1"}, "text": text})]);

    platform.push("u1", "/approve 1");
    let asked = aura.request("the answer as a message");
    assert_eq!(asked["method"], "message.in");
    assert_eq!(
        asked["params"],
        json!({"channel": "fake-main", "chat": {"kind": "private", "id": "u1"},
            "user": {"id": "u1", "name": "老王"}, "message_id": "m-1", "text": "/approve 1"})
    );
    let answer = json!({"accepted": true, "command": "approve", "repeated": false});
    aura.send(&json!({"type": "response", "id": asked["id"], "result": answer}));
    let logged = aura.logged();
    let said = logged.strip_prefix("消息 m-1：").unwrap_or_else(|| panic!("{logged}"));
    assert_eq!(serde_json::from_str::<Value>(said).expect("JSON"), answer);
    assert_eq!(platform.sent().len(), 1, "what Aura answers comes as its reply, not the adapter's");
}
