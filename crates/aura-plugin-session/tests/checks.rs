//! A plugin acts in a conversation only when it is installed, usable (trusted, switched on and
//! unchanged since) and declared the capability; each refusal says which of these is missing and
//! where the user changes it. A message's de-duplication key always names the same command for
//! the same plugin and conversation, so a message pushed twice is one message.
use aura_domain_common::SessionId;
use aura_plugin_session::{Standing, checked_key, checked_text, message_command, permitted};
use aura_plugins::{PluginCapability, PluginState};

fn refusal(result: anyhow::Result<()>) -> String {
    result.expect_err("refused").to_string()
}

#[test]
fn a_plugin_acts_only_when_installed_usable_and_declaring_the_capability() {
    let declared = [PluginCapability::SessionRead, PluginCapability::SessionEvents];
    permitted("usage-panel", Standing::Ready(&declared), PluginCapability::SessionRead)
        .expect("declared");
    let missing = refusal(permitted(
        "usage-panel",
        Standing::Ready(&declared),
        PluginCapability::SessionSend,
    ));
    assert!(missing.starts_with("plugin_session.capability_missing"), "{missing}");
    assert!(missing.contains("usage-panel") && missing.contains("session.send"), "{missing}");

    let absent = refusal(permitted("ghost", Standing::Missing, PluginCapability::SessionRead));
    assert!(absent.starts_with("plugin_session.plugin_missing") && absent.contains("ghost"));

    for (state, says) in [
        (PluginState::NotTrusted, "信任"),
        (PluginState::ContentChanged, "重新信任"),
        (PluginState::Disabled, "打开"),
        (PluginState::Invalid, "设置 → 插件"),
        (PluginState::Unavailable, "设置 → 插件"),
    ] {
        let refused = refusal(permitted(
            "usage-panel",
            Standing::Installed(state),
            PluginCapability::SessionRead,
        ));
        assert!(refused.starts_with("plugin_session.not_ready"), "{state:?}: {refused}");
        assert!(refused.contains("usage-panel") && refused.contains(says), "{state:?}: {refused}");
    }
    // A plugin that is installed and ready is only ever reported as ready.
    let ready = refusal(permitted(
        "usage-panel",
        Standing::Installed(PluginState::Ready),
        PluginCapability::SessionRead,
    ));
    assert!(ready.starts_with("plugin_session.not_ready"), "{ready}");
}

#[test]
fn the_same_key_from_the_same_plugin_in_the_same_conversation_is_the_same_message() {
    let session = SessionId::new();
    let once = message_command("qq-bot", session, "msg-1");
    assert_eq!(once, message_command("qq-bot", session, "msg-1"));
    assert_ne!(once, message_command("qq-bot", session, "msg-2"));
    assert_ne!(once, message_command("other-bot", session, "msg-1"));
    assert_ne!(once, message_command("qq-bot", SessionId::new(), "msg-1"));
    // The parts cannot be shifted into one another.
    assert_ne!(message_command("a", session, "bc"), message_command("ab", session, "c"));
}

#[test]
fn a_key_and_a_message_are_checked_before_anything_is_sent() {
    checked_key("C2C_3f8a").expect("plain");
    for bad in ["", " ", &"k".repeat(129), "line\nbreak", "tab\there"] {
        let refused = checked_key(bad).expect_err("refused").to_string();
        assert!(refused.starts_with("plugin_session.dedupe_key_invalid"), "{bad:?}: {refused}");
    }
    checked_text("本会话用了多少").expect("plain");
    checked_text(&"字".repeat(16_000)).expect("at the limit");
    for bad in ["", "   \n", &"字".repeat(16_001)] {
        let refused = checked_text(bad).expect_err("refused").to_string();
        assert!(refused.starts_with("plugin_session.message_invalid"), "{refused}");
    }
}
