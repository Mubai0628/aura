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
