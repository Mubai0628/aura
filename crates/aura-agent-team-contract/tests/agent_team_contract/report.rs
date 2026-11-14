//! A report's summary: an empty one and an overlong one are refused for what each is, and the
//! overlong one is told its length and where long content goes instead.
use aura_agent_team_contract::{Report, ReportStatus};

fn ready(summary: &str) -> Report {
    Report {
        status: ReportStatus::Ready,
        summary: summary.into(),
        question: None,
        files: Vec::new(),
        checks: Vec::new(),
        remaining: None,
    }
}

#[test]
fn an_empty_summary_is_refused_as_empty() {
    let refused = ready(" \n ").check().expect_err("empty").to_string();
    assert!(refused.starts_with("agent.report_invalid:"), "{refused}");
    assert!(refused.contains("空"), "{refused}");
    assert!(!refused.contains("8000"), "an empty summary is not a long one: {refused}");
}

#[test]
fn an_overlong_summary_is_told_its_length_and_where_long_content_goes() {
    assert!(ready(&"字".repeat(8_000)).check().is_ok(), "8000 characters fit");
    let refused = ready(&"字".repeat(8_001)).check().expect_err("too long").to_string();
    assert!(refused.starts_with("agent.report_invalid:"), "{refused}");
    assert!(refused.contains("8001") && refused.contains("8000"), "{refused}");
    assert!(refused.contains("文件") && refused.contains("最终回答"), "{refused}");
}
