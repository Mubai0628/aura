//! The projection's own rules: short lines, bounded details, a row's target.
use super::*;
use aura_domain_common::ErrorEnvelope;
fn view() -> AgentToolDisplay {
    AgentToolDisplay {
        title: "".into(),
        target: None,
        summary: "".into(),
        details: vec![],
        truncated: false,
        internal: false,
        image: None,
        offer: None,
    }
}

fn error(category: ErrorCategory) -> ErrorEnvelope {
    ErrorEnvelope::new(
        aura_domain_common::ErrorCode::new("fs.not_found").expect("code"),
        category,
        "The path was not found.",
        false,
        aura_domain_common::CorrelationId::new(),
        None,
    )
}

fn envelope(code: &str, category: ErrorCategory, message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(
        aura_domain_common::ErrorCode::new(code).expect("code"),
        category,
        message,
        false,
        aura_domain_common::CorrelationId::new(),
        None,
    )
}

/// A computer call admission refused as a resource it could not resolve, with the desktop's
/// message.
fn admitted(message: &str) -> ToolResultOutcomeV1 {
    ToolResultOutcomeV1::NotExecuted {
        reason: ToolNotExecutedReasonV1::ResourceClaimResolutionFailed,
        error: Some(envelope(
            "tool.resource.identity_resolution_failed",
            ErrorCategory::Validation,
            message,
        )),
    }
}

/// A call the desktop refused with `code`.
fn rejected(code: &str, category: ErrorCategory) -> ToolResultOutcomeV1 {
    ToolResultOutcomeV1::OperationRejected {
        error: envelope(code, category, &format!("{code}: 没有执行")),
    }
}

fn result(outcome: ToolResultOutcomeV1) -> ToolResultV1 {
    ToolResultV1 {
        tool_call_id: aura_domain_common::ToolCallId::new(),
        outcome,
        side_effect_state: SideEffectStateV1::NotStarted,
        attempts: vec![],
        model_visible_result_ref: None,
        user_audit_result_ref: None,
        internal_diagnostic_ref: None,
        aggregate_side_effect_evidence_ref: None,
        started_at: None,
        completed_at: aura_domain_common::UtcTimestamp::now(),
    }
}

/// The four Shell cleanup refusals were `operation_rejected` + `not_found` and read
/// as 已拒绝. The row now names the reason the Store recorded.
#[test]
fn a_rejected_call_names_the_category_the_store_recorded() {
    for (category, expected) in [
        (ErrorCategory::NotFound, "未执行：目标不存在"),
        (ErrorCategory::Permission, "未执行：权限拒绝"),
        (ErrorCategory::Conflict, "未执行：状态已变化"),
        (ErrorCategory::Validation, "未执行：请求无效"),
        (ErrorCategory::Unavailable, "未执行：当前不可用"),
    ] {
        assert_eq!(
            outcome_summary_text(&ToolResultOutcomeV1::OperationRejected {
                error: error(category)
            }),
            expected,
            "{category:?} must not share a sentence with another category"
        );
    }
}

#[test]
fn a_short_line_carries_the_diagnostic_into_the_details_instead_of_cutting_it() {
    let mut failure = error(ErrorCategory::Validation);
    failure.message = "The call was not executed. `_aura` may contain only `purpose`; the field(s) path must not be inside it.".into();
    failure.details.insert("fields".into(), serde_json::json!(["path", "limit"]));
    let result = result(ToolResultOutcomeV1::NotExecuted {
        reason: ToolNotExecutedReasonV1::SchemaValidationFailed,
        error: Some(failure),
    });
    assert_eq!(outcome_summary_text(&result.outcome), "未执行：参数错误");
    let details = error_detail(&result);
    assert!(details.iter().any(|line| line.contains("fs.not_found")), "{details:?}");
    assert!(
        details.iter().any(|line| line.contains("_aura")),
        "the stored correction is what the reader expands: {details:?}"
    );
    assert!(
        details.iter().any(|line| line.contains("path, limit")),
        "the named fields survive into the details: {details:?}"
    );
}

#[test]
fn an_unconfirmed_side_effect_outranks_every_error_line() {
    // The projection decides this before reading any error, so a row whose effect is
    // unknown is never presented as a refusal or as a success.
    let mut with_unknown = view();
    with_unknown.summary = "操作已返回，实际影响尚未确认".into();
    assert_eq!(with_unknown.summary, "操作已返回，实际影响尚未确认");
}

#[test]
fn a_legacy_result_without_an_error_projects_a_neutral_line() {
    let result = result(ToolResultOutcomeV1::NotExecuted {
        reason: ToolNotExecutedReasonV1::SchemaValidationFailed,
        error: None,
    });
    assert_eq!(outcome_summary_text(&result.outcome), "未执行：参数错误");
    assert!(
        error_detail(&result).is_empty(),
        "no stored error means no diagnostic to expand, not a fabricated one"
    );
}
#[test]
fn summaries_are_bounded_facts_not_raw_payloads() {
    let mut v = view();
    summarize(
        &mut v,
        "fs.list",
        &serde_json::json!({"entries":[{"name":"中文.txt","kind":"file"},{"name":"目录","kind":"directory"}],"truncated":true,"private":"not exposed"}),
    );
    assert_eq!(v.summary, "1 个文件，1 个目录 · 还有下一页");
    assert_eq!(v.details, vec!["中文.txt", "目录"]);
    let matches = (0..120)
        .map(|_| serde_json::json!({"path":"a.txt","line_index":4,"excerpt":"中".repeat(1000)}))
        .collect::<Vec<_>>();
    summarize(
        &mut v,
        "fs.search",
        &serde_json::json!({"matches":matches,"completion":{"status":"partial"}}),
    );
    assert_eq!(v.details.len(), 5);
    assert!(v.details.iter().all(|s| s.chars().count() < 150));
    assert!(v.truncated);
}

/// The search and find tools work in the workspace root when the model names no
/// directory, so their row names it instead of showing no target at all.
#[test]
fn a_directory_left_to_its_default_is_shown_as_the_workspace_root() {
    use serde_json::json;
    assert_eq!(target("fs.find", Some(&json!({"file_names":["a.rs"]}))).as_deref(), Some("."));
    assert_eq!(target("fs.search", Some(&json!({"pattern":{}}))).as_deref(), Some("."));
    assert_eq!(target("fs.search", Some(&json!({"root":"src"}))).as_deref(), Some("src"));
    assert_eq!(target("fs.read", Some(&json!({"range":{}}))), None, "no default to show");
    assert_eq!(target("fs.find", None), None, "an unreadable call names nothing");
}

/// A Skill row showed only the tool's name. It names the Skill loaded (with where it is from
/// when the call said) or the words searched for, and what came back.
#[test]
fn a_skill_row_names_the_skill_or_the_words_and_what_came_back() {
    use serde_json::json;
    assert_eq!(title("skills.load"), "加载 Skill");
    assert_eq!(title("skills.search"), "搜索 Skill");
    assert_eq!(target("skills.load", Some(&json!({"name":"notes"}))).as_deref(), Some("notes"));
    assert_eq!(
        target("skills.load", Some(&json!({"name":"notes","from":"agent_global"}))).as_deref(),
        Some("notes [agent_global]")
    );
    assert_eq!(
        target("skills.search", Some(&json!({"query":"pdf form"}))).as_deref(),
        Some("pdf form")
    );
    assert_eq!(target("skills.search", Some(&json!({"query":" "}))), None, "every Skill");
    let mut found = view();
    summarize(&mut found, "skills.search", &json!({"skills":[{"name":"a"},{"name":"b"}]}));
    assert_eq!(found.summary, "找到 2 个 Skill");
    let mut more = view();
    summarize(&mut more, "skills.search", &json!({"skills":[{"name":"a"}],"truncated":true}));
    assert_eq!(more.summary, "找到 1 个 Skill，还有更多");
    let mut loaded = view();
    summarize(
        &mut loaded,
        "skills.load",
        &json!({"name":"notes","folder":"C:\\Users\\me\\.agents\\skills\\notes","files":["references/guide.md"]}),
    );
    assert_eq!(loaded.summary, "已加载，另有 1 个文件");
    assert_eq!(loaded.details, vec!["文件夹：C:\\Users\\me\\.agents\\skills\\notes"]);
}

/// A Shell row showed only `.`, the working directory, so nobody saw what ran. It
/// names the command, and opens to all of it and a directory the model chose.
#[test]
fn a_shell_row_names_its_command_and_opens_to_all_of_it() {
    use serde_json::json;
    let one = json!({"script":"git status"});
    assert_eq!(target("shell.cmd", Some(&one)).as_deref(), Some("git status"));
    assert_eq!(input_details("shell.cmd", Some(&one)), vec!["命令：git status"]);
    let several = json!({"script":"\n  cd src\r\ncargo test\n","working_directory":"crates"});
    assert_eq!(target("shell.powershell", Some(&several)).as_deref(), Some("cd src …"));
    assert_eq!(
        input_details("shell.powershell", Some(&several)),
        vec!["命令：cd src\r\ncargo test", "目录：crates"]
    );
    let long = json!({"script":"x".repeat(300)});
    let shown = target("shell.cmd", Some(&long)).expect("a preview");
    assert_eq!(shown.chars().count(), 121, "{shown}");
    assert!(shown.ends_with('…'));
    assert_eq!(target("shell.cmd", Some(&json!({"script":"  "}))), None);
}

#[test]
fn a_filename_lookup_reports_hits_and_whether_the_answer_is_whole() {
    let mut v = view();
    summarize(
        &mut v,
        "fs.find",
        &serde_json::json!({
            "matches":[{"path":"src/main.rs"},{"path":"文档/说明.md"}],
            "complete":false,
            "scope_revision":"12",
            "private":"not exposed"
        }),
    );
    assert_eq!(v.summary, "找到 2 个文件 · 结果未完整");
    assert_eq!(v.details, vec!["src/main.rs", "文档/说明.md"]);
    assert!(v.truncated, "an incomplete page is never presented as the whole answer");

    let matches = (0..40)
        .map(|index| serde_json::json!({"path":format!("dir/{index}.txt")}))
        .collect::<Vec<_>>();
    summarize(&mut v, "fs.find", &serde_json::json!({"matches":matches,"complete":true}));
    assert_eq!(v.summary, "找到 40 个文件 · 展示前 10 项");
    assert_eq!(v.details.len(), 10);
    assert!(v.truncated);
}

/// A computer call the user ended read “无法确定操作目标” or “操作被拒绝”: admission refuses a
/// taken-over turn as a resource it could not resolve, and the desktop's own refusal is a
/// cancellation. The row says what the user did instead.
#[test]
fn a_computer_call_the_user_ended_says_so() {
    for (outcome, said) in [
        (
            admitted("computer.taken_over: 用户已接管电脑，这一轮不能再操作电脑；没有执行"),
            "未执行：你已接管电脑",
        ),
        (rejected("computer.taken_over", ErrorCategory::Cancelled), "未执行：你已接管电脑"),
        (rejected("computer.busy", ErrorCategory::Unavailable), "未执行：另一段对话正在操控电脑"),
        (
            admitted(
                "computer.stopped: 用户已停止这个程序的电脑操控，这一回合不能再操作；没有执行",
            ),
            "未执行：你已停止操控这个程序",
        ),
        (rejected("computer.stopped", ErrorCategory::Cancelled), "未执行：你已停止操控这个程序"),
        (admitted("computer.point: 这个点不在程序的窗口里"), "未执行：无法确定操作目标"),
        (rejected("computer.stale", ErrorCategory::Conflict), "未执行：状态已变化"),
    ] {
        assert_eq!(summary_line("computer.input", &outcome), said, "{outcome:?}");
    }
    // Only a computer call is read this way, and only for a call that changed nothing.
    let taken = admitted("computer.taken_over: 用户已接管电脑");
    assert_eq!(summary_line("browser.click", &taken), "未执行：无法确定操作目标");
    let failed = ToolResultOutcomeV1::Failed {
        error: envelope(
            "tool.runtime.dispatch_failed",
            ErrorCategory::Unavailable,
            "computer.busy: x",
        ),
    };
    assert_eq!(summary_line("computer.input", &failed), "执行失败：当前不可用");
}

/// The row names what took the desktop over when the refusal says it, from admission and from
/// the desktop alike.
#[test]
fn a_computer_call_the_user_ended_says_what_they_did() {
    for (outcome, said) in [
        (
            admitted(
                "computer.taken_over: 用户已接管电脑（点了鼠标），这一轮不能再操作电脑；没有执行",
            ),
            "未执行：你已接管电脑（点了鼠标）",
        ),
        (
            ToolResultOutcomeV1::OperationRejected {
                error: envelope(
                    "computer.taken_over",
                    ErrorCategory::Cancelled,
                    "computer.taken_over: 用户已接管电脑（滚了滚轮，来自另一个程序），这一轮不要再操作电脑",
                ),
            },
            "未执行：你已接管电脑（滚了滚轮，来自另一个程序）",
        ),
        (admitted("computer.taken_over: 用户已接管电脑（），没有执行"), "未执行：你已接管电脑"),
    ] {
        assert_eq!(summary_line("computer.input", &outcome), said, "{outcome:?}");
    }
}

/// A computer call the user took the desktop over from offers nothing on its row or its turn:
/// the desktop is given back from 近期活动. No other computer refusal offers anything either.
#[test]
fn a_computer_call_the_user_took_over_offers_nothing() {
    let offered = |name: &str, outcome: ToolResultOutcomeV1| row_offer(name, &result(outcome));
    for (name, outcome) in [
        (
            "computer.input",
            admitted(
                "computer.taken_over: 用户已接管电脑，用户交还操作权之前不要再操作电脑；没有执行",
            ),
        ),
        ("computer.act", rejected("computer.taken_over", ErrorCategory::Cancelled)),
        (
            "computer.input",
            ToolResultOutcomeV1::Indeterminate {
                reason: aura_tool_contract::ToolIndeterminateReasonV1::DispatchOutcomeUnknown,
                error: envelope(
                    "computer.taken_over",
                    ErrorCategory::Unavailable,
                    "computer.taken_over: 用户已接管小窗，这一步可能执行了部分",
                ),
            },
        ),
        ("computer.input", rejected("computer.busy", ErrorCategory::Unavailable)),
        ("computer.input", rejected("computer.stopped", ErrorCategory::Cancelled)),
        ("computer.input", admitted("computer.point: 这个点不在程序的窗口里")),
        ("browser.click", admitted("computer.taken_over: 用户已接管电脑")),
        (
            "computer.input",
            ToolResultOutcomeV1::Failed {
                error: envelope(
                    "tool.runtime.dispatch_failed",
                    ErrorCategory::Unavailable,
                    "computer.taken_over: x",
                ),
            },
        ),
        ("computer.input", ToolResultOutcomeV1::Succeeded),
    ] {
        let shown = format!("{name} {outcome:?}");
        assert_eq!(offered(name, outcome), None, "{shown}");
    }
}

/// A call stopped for that reason offers the switch on its row.
#[test]
fn a_page_debugging_call_stopped_by_the_switch_offers_it() {
    let refused = |message: &str| {
        result(ToolResultOutcomeV1::Failed {
            error: ErrorEnvelope::new(
                aura_domain_common::ErrorCode::new("tool.runtime.dispatch_failed").expect("code"),
                ErrorCategory::Unavailable,
                message,
                false,
                aura_domain_common::CorrelationId::new(),
                None,
            ),
        })
    };
    let off = refused("browser.debugging_off: 网页调试没有开启。");
    assert_eq!(row_offer("browser.evaluate", &off), Some(AgentToolOffer::EnableBrowserDebugging));
    assert_eq!(row_offer("browser.network", &off), Some(AgentToolOffer::EnableBrowserDebugging));
    // Another reason, or another tool, offers nothing.
    assert_eq!(row_offer("browser.evaluate", &refused("browser.refused: 已接管")), None);
    assert_eq!(row_offer("fs.read", &off), None);
    assert_eq!(row_offer("browser.evaluate", &result(ToolResultOutcomeV1::Succeeded)), None);
}
