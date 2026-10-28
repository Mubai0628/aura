//! What is refused before anything is asked, and what a lost connection leaves: a password field
//! is the user's alone; without a connected extension no Chrome tool runs; a click whose answer
//! never came is recorded as unknown and is not run again.
use super::*;

const TYPE_PASSWORD: Scenario =
    Scenario::Browser { tool: "chrome.type", arguments: r#"{"ref":"e5","text":"hunter2"}"# };
const READ: Scenario = Scenario::Browser { tool: "chrome.snapshot", arguments: "{}" };
const CLICK: Scenario = Scenario::Browser { tool: "chrome.click", arguments: r#"{"ref":"e2"}"# };

#[tokio::test]
async fn a_password_field_is_refused_before_anything_is_asked() {
    let password = Page { refused: Some("这是密码输入框"), ..ORDINARY };
    let chrome =
        run(TYPE_PASSWORD, Setup::sharing(PermissionMode::ReadOnly), answers(password)).await;
    let view = chrome.wait(|view| completed(view) || asking(view)).await;
    assert!(completed(&view), "refused, not asked");
    assert_eq!(
        chrome.extension.commands(),
        vec![ChromeCommand::Inspect {
            tab_id: TAB,
            element: Some("e5".into()),
            x: None,
            y: None,
            intent: ChromeIntent::Type
        }],
        "only the look before admission reached the tab"
    );
    let text = chrome.returned(0);
    assert!(
        text.contains("chrome.refused")
            && text.contains("这是密码输入框")
            && text.contains("chrome.request_user"),
        "{text}"
    );
}

/// A step that changed nothing is an ordinary refusal of its own kind: the model reads Chrome's
/// code, and the user's list names the kind instead of "工具暂不可用".
#[tokio::test]
async fn a_screenshot_of_a_tab_in_the_background_is_refused_as_that() {
    const SHOT: Scenario = Scenario::Browser { tool: "chrome.screenshot", arguments: "{}" };
    let behind: Answers = Box::new(|command| match command {
        ChromeCommand::Screenshot { .. } => Some(CommandOutcome::Failed {
            code: "chrome.background_tab".into(),
            message: "这个标签页在后台".into(),
        }),
        other => answers(ORDINARY)(other),
    });
    let chrome = run(SHOT, Setup::sharing(PermissionMode::FullAccess), behind).await;
    chrome.wait(completed).await;
    let text = chrome.returned(0);
    assert!(text.contains("chrome.background_tab") && text.contains("这个标签页在后台"), "{text}");
    let page = crate::r03_tests::success(
        &chrome.runtime,
        "agent.tool_activity",
        json!({"session_id": chrome.session, "before_call_id": null, "limit": 10, "call_ids": null}),
    )
    .await;
    let summary = &page["calls"][0]["summary"];
    assert_eq!(summary["outcome"]["type"], "operation_rejected", "{summary}");
    assert_eq!(summary["error_code"], "chrome.background_tab", "{summary}");
    assert_eq!(summary["error_class"], "unavailable", "{summary}");
    assert_eq!(summary["side_effect_state"], "no_effect", "{summary}");
}

#[tokio::test]
async fn without_a_connected_extension_no_chrome_tool_runs() {
    let setup = Setup { connected: false, ..Setup::sharing(PermissionMode::FullAccess) };
    let chrome = run(READ, setup, answers(ORDINARY)).await;
    let view = chrome.wait(|view| completed(view) || asking(view)).await;
    assert!(completed(&view));
    assert!(chrome.extension.commands().is_empty());
    let text = chrome.returned(0);
    assert!(!text.contains("Checkout"), "{text}");
}

#[tokio::test]
async fn a_click_whose_answer_never_came_is_unknown_and_not_run_again() {
    let silent: Answers = Box::new(|command| match command {
        ChromeCommand::Click { .. } => None,
        other => answers(ORDINARY)(other),
    });
    let chrome = run(CLICK, Setup::sharing(PermissionMode::FullAccess), silent).await;
    until(|| chrome.sent().iter().any(|command| matches!(command, ChromeCommand::Click { .. })))
        .await;
    chrome.extension.disconnect();
    chrome.wait(completed).await;
    let text = chrome.returned(0);
    assert!(
        text.contains("chrome.disconnected") && text.contains("不确定这一步是否已经执行"),
        "{text}"
    );
    assert_eq!(
        chrome
            .sent()
            .iter()
            .filter(|command| matches!(command, ChromeCommand::Click { .. }))
            .count(),
        1,
        "never run again"
    );
    let effects: Vec<String> = {
        let journal = services(&chrome.runtime).broker.lock().expect("journal");
        let mut query = journal
            .database
            .connection()
            .prepare(
                "SELECT r.side_effect FROM execution_results r \
                 JOIN operations o ON o.id = r.operation_id \
                 WHERE json_extract(o.payload,'$.content.action_kind') = 'chrome.click'",
            )
            .expect("query");
        query
            .query_map([], |row| row.get(0))
            .expect("rows")
            .collect::<Result<_, _>>()
            .expect("effects")
    };
    assert_eq!(effects, [r#""unknown""#], "the click's one result");
}
