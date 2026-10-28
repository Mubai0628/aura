//! What asks the user: acting in ReadOnly (an answer may cover the shared tab for the rest of the
//! turn), a sensitive action (every time, in every mode, only once), and a site the user has not
//! confirmed (once, which the share keeps, or for the session). Nothing reaches the tab before the
//! answer.
use super::*;

const READ_THEN_TWO_CLICKS: Scenario = Scenario::Sequence {
    calls: &[
        ("chrome.snapshot", "{}"),
        ("chrome.click", r#"{"ref":"e2"}"#),
        ("chrome.click", r#"{"ref":"e3"}"#),
    ],
};

const BUY_TWICE: Scenario = Scenario::Sequence {
    calls: &[("chrome.click", r#"{"ref":"e2"}"#), ("chrome.click", r#"{"ref":"e2"}"#)],
};

const READ_TWICE: Scenario =
    Scenario::Sequence { calls: &[("chrome.snapshot", "{}"), ("chrome.snapshot", "{}")] };

const OPEN_THEN_READ: Scenario = Scenario::Sequence {
    calls: &[("chrome.open", r#"{"url":"shop.example/cart"}"#), ("chrome.snapshot", "{}")],
};

const PAY: &str = "https://pay.example/checkout";

fn clicks(chrome: &Chrome) -> usize {
    chrome.sent().iter().filter(|command| matches!(command, ChromeCommand::Click { .. })).count()
}

#[tokio::test]
async fn read_only_reads_without_asking_and_a_turn_answer_covers_the_next_click() {
    let chrome =
        run(READ_THEN_TWO_CLICKS, Setup::sharing(PermissionMode::ReadOnly), answers(ORDINARY))
            .await;
    let (first, approval) = chrome.question().await;
    assert_eq!(decisions(&approval), ["allow_once", "allow_for_current_turn", "deny"]);
    assert_eq!(
        chrome.sent(),
        vec![ChromeCommand::Snapshot { tab_id: TAB, max_chars: 12_000 }],
        "the read ran; the click waits for the answer"
    );
    chrome.answer(first, &approval, "allow_for_current_turn").await;
    let view = chrome
        .wait(|view| completed(view) || view.pending_approval_ids.iter().any(|id| *id != first))
        .await;
    assert!(completed(&view), "the turn answer covers the second click");
    assert_eq!(clicks(&chrome), 2);
}

#[tokio::test]
async fn a_sensitive_action_asks_every_time_even_with_full_access() {
    let sensitive = Page { sensitive: true, ..ORDINARY };
    let chrome =
        run(BUY_TWICE, Setup::sharing(PermissionMode::FullAccess), answers(sensitive)).await;
    let (first, approval) = chrome.question().await;
    assert_eq!(decisions(&approval), ["allow_once", "deny"]);
    assert_eq!(clicks(&chrome), 0, "nothing acts before the answer");
    chrome.answer(first, &approval, "allow_once").await;
    let view = chrome
        .wait(|view| completed(view) || view.pending_approval_ids.iter().any(|id| *id != first))
        .await;
    assert!(asking(&view), "the same action asks again");
    assert_eq!(clicks(&chrome), 1);
    let (second, approval) = chrome.question().await;
    assert_ne!(second, first);
    assert_eq!(decisions(&approval), ["allow_once", "deny"]);
    chrome.answer(second, &approval, "deny").await;
    chrome.wait(completed).await;
    assert_eq!(clicks(&chrome), 1, "a refused action never reaches the tab");
}

#[tokio::test]
async fn a_site_the_user_has_not_confirmed_asks_once_and_the_share_keeps_it() {
    let setup = Setup { moved_to: Some(PAY), ..Setup::sharing(PermissionMode::FullAccess) };
    let chrome = run(READ_TWICE, setup, answers(Page { url: PAY, ..ORDINARY })).await;
    let (id, approval) = chrome.question().await;
    assert_eq!(decisions(&approval), ["allow_once", "allow_session", "deny"]);
    assert!(chrome.sent().is_empty(), "nothing reaches the tab before the answer");
    chrome.answer(id, &approval, "allow_once").await;
    let view = chrome
        .wait(|view| completed(view) || view.pending_approval_ids.iter().any(|other| *other != id))
        .await;
    assert!(completed(&view), "the share keeps the confirmed site");
    assert_eq!(chrome.sent().len(), 2);
    let shares = chrome.extension.shares.lock().expect("shares").clone();
    assert_eq!(shares.len(), 1);
    assert_eq!(shares[0].state, ChromeShareState::Active);
    assert_eq!(shares[0].origin, "https://pay.example");
}

#[tokio::test]
async fn opening_a_page_asks_for_its_site_and_the_tab_becomes_the_conversations() {
    let setup = Setup { shared: Shared::Nothing, ..Setup::sharing(PermissionMode::FullAccess) };
    let chrome = run(OPEN_THEN_READ, setup, answers(ORDINARY)).await;
    let (id, approval) = chrome.question().await;
    assert_eq!(decisions(&approval), ["allow_once", "allow_session", "deny"]);
    assert!(chrome.sent().is_empty());
    chrome.answer(id, &approval, "allow_once").await;
    let view = chrome
        .wait(|view| completed(view) || view.pending_approval_ids.iter().any(|other| *other != id))
        .await;
    assert!(completed(&view), "the opened tab's site is confirmed for it");
    assert_eq!(
        chrome.sent(),
        vec![
            ChromeCommand::Open { url: SHOP.into() },
            ChromeCommand::Snapshot { tab_id: 41, max_chars: 12_000 },
        ]
    );
    assert_eq!(
        chrome.extension.origins.lock().expect("origins").clone(),
        [None, Some("https://shop.example".to_owned())],
        "opening replaces the page; reading it is held to the site it opened"
    );
    let opened = chrome.returned(0);
    assert!(opened.contains(r#""tab":"t1""#) && opened.contains("Checkout"), "{opened}");
}
