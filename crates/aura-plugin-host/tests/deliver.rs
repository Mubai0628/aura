//! Aura's own lines to a service, such as the reply to a message its channel passed in, go only to
//! a service that is running and said it is ready; one that stopped reading is ended.
mod support;
use aura_agent_domain::PluginChatKind;
use aura_plugin_host::{ChatAddress, ReplyFormat, ToService};
use serde_json::json;
use std::sync::atomic::Ordering;
use support::*;

fn reply(text: &str) -> ToService {
    ToService::Reply {
        channel: "main".into(),
        chat: ChatAddress { kind: PluginChatKind::Private, id: "A1B2".into() },
        reply_to: Some("msg-1".into()),
        text: text.into(),
        format: ReplyFormat::Markdown,
        part: None,
        parts: None,
    }
}

#[test]
fn a_reply_reaches_only_a_service_that_is_running_and_ready() {
    let rig = rig();
    let refused = rig.host.deliver("chat-bot", &reply("早"));
    assert!(refused.is_err_and(|why| why.contains("没有在运行")), "nothing runs yet");
    rig.host.reconcile(vec![launch("a")]);
    let service = rig.launcher.last();
    assert!(rig.host.deliver("chat-bot", &reply("早")).is_err(), "not before it is ready");
    service.ready();
    eventually("running", || rig.host.deliver("chat-bot", &reply("你好")).is_ok());
    let replies: Vec<_> =
        service.lines().into_iter().filter(|line| line["type"] == "reply").collect();
    assert_eq!(
        replies,
        vec![json!({"type": "reply", "channel": "main", "chat": {"kind": "private", "id": "A1B2"},
            "reply_to": "msg-1", "text": "你好", "format": "markdown"})]
    );
    assert!(rig.host.deliver("mail-bot", &reply("你好")).is_err(), "another plugin's is not it");
}

#[test]
fn a_service_that_stopped_reading_is_not_given_a_reply_and_is_ended() {
    let rig = rig();
    let service = rig.running();
    service.full.store(true, Ordering::Release);
    let refused = rig.host.deliver("chat-bot", &reply("你好"));
    assert!(refused.is_err_and(|why| why.contains("不再读")));
    eventually("ended", || service.stopped.load(Ordering::Acquire));
}
