//! A working session lists, reads and acts in the tab it shares without asking. An action is
//! looked at in the tab before admission; the model knows the tab by its reference only, and a
//! conversation the tab was not shared with cannot reach it.
use super::*;

const LIST_READ_CLICK: Scenario = Scenario::Sequence {
    calls: &[("chrome.tabs", "{}"), ("chrome.snapshot", "{}"), ("chrome.click", r#"{"ref":"e2"}"#)],
};

const READ_T1: Scenario =
    Scenario::Browser { tool: "chrome.snapshot", arguments: r#"{"tab":"t1"}"# };
const READ: Scenario = Scenario::Browser { tool: "chrome.snapshot", arguments: "{}" };

#[tokio::test]
async fn a_working_session_lists_reads_and_clicks_in_its_shared_tab_without_asking() {
    let chrome =
        run(LIST_READ_CLICK, Setup::sharing(PermissionMode::WorkspaceReadWrite), answers(ORDINARY))
            .await;
    let view = chrome.wait(|view| completed(view) || asking(view)).await;
    assert!(completed(&view), "nothing asked: {:?}", view.pending_approval_ids);
    let tabs = chrome.returned(0);
    assert!(tabs.contains(r#""tab":"t1""#) && tabs.contains(SHOP), "{tabs}");
    assert_eq!(
        chrome.extension.commands(),
        vec![
            ChromeCommand::Snapshot { tab_id: TAB, max_chars: 12_000 },
            ChromeCommand::Inspect {
                tab_id: TAB,
                element: Some("e2".into()),
                x: None,
                y: None,
                intent: ChromeIntent::Click
            },
            ChromeCommand::Click {
                tab_id: TAB,
                element: Some("e2".into()),
                x: None,
                y: None,
                button: ChromeMouseButton::Left,
                double: false
            },
        ]
    );
    let site = Some("https://shop.example".to_owned());
    assert_eq!(
        chrome.extension.origins.lock().expect("origins").clone(),
        [site.clone(), None, site],
        "each command names the site it was authorized for; the look before admission does not"
    );
    for ordinal in [1, 2] {
        let page = chrome.returned(ordinal);
        assert!(page.contains("Checkout") && page.contains(r#""tab":"t1""#), "{page}");
        assert!(!page.contains("tab_id"), "Chrome's own id never reaches the model: {page}");
    }
}

#[tokio::test]
async fn a_conversation_the_tab_was_not_shared_with_cannot_reach_it() {
    let setup = Setup { shared: Shared::Elsewhere, ..Setup::sharing(PermissionMode::FullAccess) };
    let chrome = run(READ_T1, setup, answers(ORDINARY)).await;
    chrome.wait(completed).await;
    assert!(chrome.extension.commands().is_empty());
    let text = chrome.returned(0);
    assert!(text.contains("chrome.no_tab") && text.contains("chrome.tabs"), "{text}");
}

#[tokio::test]
async fn a_tab_shared_for_one_turn_stops_being_shared_when_the_turn_ends() {
    let setup = Setup {
        scope: ChromeShareScope::Turn,
        ..Setup::sharing(PermissionMode::WorkspaceReadWrite)
    };
    let chrome = run(READ, setup, answers(ORDINARY)).await;
    chrome.wait(completed).await;
    assert!(chrome.returned(0).contains("Checkout"), "the share served its turn");
    let control = Arc::clone(&services(&chrome.runtime).chrome);
    until(|| !control.has_shares(chrome.session)).await;
    until(|| chrome.extension.shares.lock().expect("shares").is_empty()).await;
}

/// A click by position is looked at where it lands, not at the focused element.
#[tokio::test]
async fn a_click_at_a_point_is_looked_at_where_it_lands() {
    const AT_POINT: Scenario =
        Scenario::Browser { tool: "chrome.click", arguments: r#"{"x":280,"y":330}"# };
    let chrome =
        run(AT_POINT, Setup::sharing(PermissionMode::WorkspaceReadWrite), answers(ORDINARY)).await;
    chrome.wait(completed).await;
    assert_eq!(
        chrome.extension.commands()[0],
        ChromeCommand::Inspect {
            tab_id: TAB,
            element: None,
            x: Some(280.0),
            y: Some(330.0),
            intent: ChromeIntent::Click
        }
    );
}
