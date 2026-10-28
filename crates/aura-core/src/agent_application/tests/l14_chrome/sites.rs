//! A site the user allows for the session asks no more in any of the session's tabs, and
//! `chrome.open` on a site the conversation has a tab of uses that tab: it goes to a new address,
//! and stays on the one it shows.
use super::*;
use std::sync::Mutex;

const CART: &str = "https://shop.example/cart";
const ORDERS: &str = "https://shop.example/orders";

const OPEN_AGAIN: Scenario = Scenario::Sequence {
    calls: &[
        ("chrome.open", r#"{"url":"shop.example/cart"}"#),
        ("chrome.open", r#"{"url":"shop.example/orders"}"#),
        ("chrome.open", r#"{"url":"https://shop.example/orders"}"#),
        ("chrome.close", "{}"),
        ("chrome.open", r#"{"url":"shop.example/cart"}"#),
    ],
};

/// An extension whose tab is wherever it was last sent.
fn following() -> Answers {
    let at = Mutex::new(String::new());
    Box::new(move |command| {
        let mut at = at.lock().expect("address");
        match command {
            ChromeCommand::Open { url } => {
                url.clone_into(&mut at);
                done(ChromeOpenReport {
                    tab: ChromeOpenedTab {
                        tab_id: 41,
                        window_id: 1,
                        url: url.clone(),
                        title: "Shop".to_owned(),
                    },
                    page: page(url),
                })
            }
            ChromeCommand::Navigate { url: Some(url), .. } => {
                url.clone_into(&mut at);
                done(page(url))
            }
            ChromeCommand::Close { .. } => done(ChromeCloseReport { closed: true }),
            ChromeCommand::Release { .. } => None,
            _ => done(page(&at)),
        }
    })
}

#[tokio::test]
async fn a_site_allowed_for_the_session_asks_no_more_and_opens_again_in_its_tab() {
    let setup =
        Setup { shared: Shared::Nothing, ..Setup::sharing(PermissionMode::WorkspaceReadWrite) };
    let chrome = run(OPEN_AGAIN, setup, following()).await;
    let (id, approval) = chrome.question().await;
    assert_eq!(decisions(&approval), ["allow_once", "allow_session", "deny"]);
    chrome.answer(id, &approval, "allow_session").await;
    let view = chrome
        .wait(|view| completed(view) || view.pending_approval_ids.iter().any(|other| *other != id))
        .await;
    assert!(completed(&view), "nothing else asked: {:?}", view.pending_approval_ids);
    assert_eq!(
        chrome.sent(),
        vec![
            ChromeCommand::Open { url: CART.into() },
            ChromeCommand::Navigate { tab_id: 41, url: Some(ORDERS.into()), action: None },
            ChromeCommand::Snapshot { tab_id: 41, max_chars: 12_000 },
            ChromeCommand::Close { tab_id: 41 },
            ChromeCommand::Open { url: CART.into() },
        ],
        "the site's tab is used again; after it closed, the session still knows the site"
    );
    let moved = chrome.returned(1);
    assert!(moved.contains(r#""tab":"t1""#) && moved.contains("已在后台转到这个地址"), "{moved}");
    let stayed = chrome.returned(2);
    assert!(stayed.contains("没有重新加载") && stayed.contains(ORDERS), "{stayed}");
    let reopened = chrome.returned(4);
    assert!(reopened.contains(r#""tab":"t2""#), "{reopened}");
}
