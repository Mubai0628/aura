//! A plugin's notice and its status entry are one line of plain text, short enough for where
//! they show; a status entry without text clears it.
use aura_agent_domain::{PLUGIN_NOTICE_MAX_CHARS, PLUGIN_STATUS_MAX_CHARS};
use aura_plugin_session::{checked_notice, checked_status};

#[test]
fn a_notice_is_one_line_not_blank_and_short() {
    assert_eq!(checked_notice("  已同步 3 条消息  ").expect("notice"), "已同步 3 条消息");
    assert!(checked_notice(&"字".repeat(PLUGIN_NOTICE_MAX_CHARS)).is_ok());
    let long = "字".repeat(PLUGIN_NOTICE_MAX_CHARS + 1);
    for text in ["", "   ", "第一行\n第二行", "制表\t符", "\u{1b}[31m红", long.as_str()] {
        let refused = checked_notice(text).expect_err(text).to_string();
        assert!(refused.starts_with("plugin_session.notice_invalid"), "{refused}");
    }
}

#[test]
fn a_status_entry_is_one_short_line_and_no_text_clears_it() {
    assert_eq!(checked_status(Some(" 已连接 ")).expect("status"), Some("已连接".to_owned()));
    assert_eq!(checked_status(None).expect("cleared"), None);
    assert_eq!(checked_status(Some("   ")).expect("cleared"), None);
    assert!(checked_status(Some(&"字".repeat(PLUGIN_STATUS_MAX_CHARS))).is_ok());
    let long = "字".repeat(PLUGIN_STATUS_MAX_CHARS + 1);
    // Spaces and line breaks at the edges are dropped; one inside makes two lines.
    assert_eq!(checked_status(Some("连接中\n")).expect("trimmed"), Some("连接中".to_owned()));
    for text in ["连接\n中", "a\rb", long.as_str()] {
        let refused = checked_status(Some(text)).expect_err(text).to_string();
        assert!(refused.starts_with("plugin_session.status_invalid"), "{refused}");
    }
}
