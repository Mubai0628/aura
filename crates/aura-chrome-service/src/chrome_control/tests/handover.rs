//! The user's Chrome tabs in Aura's @ menu, and a tab handed to a conversation with @: the
//! extension lists the tabs, a picked tab is read once for the message and shared with the
//! conversation until the user stops it, and a tab the extension did not list, a page Aura may not
//! have, a conversation that is gone or a page that cannot be read shares nothing.
use super::lock::Titles;
use super::test_extension::PlayedExtension;
use super::*;
use aura_chrome_domain::ChromeTabChoice;
use aura_chrome_domain::commands::{
    ChromeCommand, ChromeOpenedTab, ChromePageReport, ChromeShareView, ChromeTabsReport,
};
use aura_domain_common::SessionId;

const CART: &str = "https://shop.example/cart";

fn listed(tab_id: u32, url: &str, title: &str) -> ChromeOpenedTab {
    ChromeOpenedTab { tab_id, window_id: 1, url: url.to_owned(), title: title.to_owned() }
}

fn done(value: serde_json::Value) -> Option<CommandOutcome> {
    Some(CommandOutcome::Done { value })
}

/// Tab 7 is a cart page that reads; tab 8 cannot be read; tab 10 is a store page.
fn browser() -> test_extension::Answers {
    Box::new(|command| match command {
        ChromeCommand::Tabs => done(
            serde_json::to_value(ChromeTabsReport {
                tabs: vec![
                    listed(7, CART, "Cart"),
                    listed(8, "https://pay.example/", "Pay"),
                    listed(10, "https://chromewebstore.google.com/detail/x", "Store"),
                ],
            })
            .expect("json"),
        ),
        ChromeCommand::Snapshot { tab_id: 7, .. } => done(
            serde_json::to_value(ChromePageReport {
                url: CART.to_owned(),
                title: "Cart".to_owned(),
                snapshot: r#"button "Checkout" [ref=e1]"#.to_owned(),
                snapshot_truncated: false,
                console: Vec::new(),
                dialogs: Vec::new(),
                notice: None,
            })
            .expect("json"),
        ),
        ChromeCommand::Release { .. } => None,
        _ => Some(CommandOutcome::Failed {
            code: "chrome.refused".into(),
            message: "Another debugger is already attached to the tab".into(),
        }),
    })
}

async fn shared_with(extension: &PlayedExtension, tab_ids: &[u32]) -> Vec<ChromeShareView> {
    for _ in 0..500 {
        let shares = extension.shares.lock().expect("shares").clone();
        if shares.iter().map(|share| share.tab_id).collect::<Vec<_>>() == tab_ids {
            return shares;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the extension never had the shares {tab_ids:?}");
}

#[tokio::test]
async fn the_at_menu_lists_the_tabs_the_extension_lists_and_names_an_old_extension() {
    let fixture = fixture_with(|_| {});
    let nothing = fixture.control.tab_choices().await;
    assert_eq!((nothing.connected, nothing.tabs.len(), nothing.issue), (false, 0, None));
    fixture.enable().await;
    let (extension, _) = fixture.paired().await;
    let extension = PlayedExtension::play(extension, browser());
    let choices = fixture.control.tab_choices().await;
    assert!(choices.connected && choices.issue.is_none(), "{choices:?}");
    let choice = |tab_id, url: &str, title: &str| ChromeTabChoice {
        tab_id,
        title: title.to_owned(),
        url: url.to_owned(),
    };
    assert_eq!(
        choices.tabs,
        [choice(7, CART, "Cart"), choice(8, "https://pay.example/", "Pay")],
        "never a page Aura may not have"
    );
    assert_eq!(extension.commands(), [ChromeCommand::Tabs], "no page is read to list them");
    assert!(extension.shares.lock().expect("shares").is_empty(), "nothing is shared");

    let older = fixture_with(|_| {});
    older.enable().await;
    let (extension, _) = older.paired().await;
    let _extension = PlayedExtension::play(
        extension,
        Box::new(|_| {
            Some(CommandOutcome::Failed {
                code: "chrome.unsupported".into(),
                message: "Aura 发来的命令这个扩展版本不认识；请更新扩展。".into(),
            })
        }),
    );
    let choices = older.control.tab_choices().await;
    assert!(choices.connected && choices.tabs.is_empty(), "{choices:?}");
    assert!(choices.issue.as_deref().is_some_and(|issue| issue.contains("更新")), "{choices:?}");
}

#[tokio::test]
async fn a_tab_handed_over_is_read_once_and_shared_with_the_conversation_until_stopped() {
    let fixture = fixture_with(|_| {});
    let docs = SessionId::new();
    fixture.control.set_sessions(Arc::new(Titles(vec![(docs, "Docs")])));
    fixture.enable().await;
    let (extension, _) = fixture.paired().await;
    let extension = PlayedExtension::play(extension, browser());
    let handed = fixture.control.hand_over_tab(docs, 7).await.expect("handed over");
    assert_eq!(
        (handed.tab.as_str(), handed.title.as_str(), handed.url.as_str()),
        ("t1", "Cart", CART)
    );
    assert!(handed.snapshot.contains("[ref=e1]") && !handed.snapshot_truncated, "{handed:?}");
    assert_eq!(
        extension.commands(),
        [ChromeCommand::Tabs, ChromeCommand::Snapshot { tab_id: 7, max_chars: 12_000 }]
    );
    assert_eq!(
        extension.origins.lock().expect("origins").clone(),
        [None, Some("https://shop.example".to_owned())],
        "the page is read only on the site the user handed over"
    );
    let tabs = fixture.control.tabs(docs);
    assert_eq!(tabs.len(), 1);
    assert!(tabs[0].tab == "t1" && tabs[0].current && !tabs[0].opened_by_aura, "{tabs:?}");
    let shares = shared_with(&extension, &[7]).await;
    assert_eq!(shares[0].session_title, "Docs");
    let again = fixture.control.hand_over_tab(docs, 7).await.expect("again");
    assert_eq!(again.tab, "t1", "the same tab keeps its name");
    assert_eq!(fixture.control.tabs(docs).len(), 1);
    fixture.control.stop_session(docs);
    assert!(fixture.control.tabs(docs).is_empty(), "the user's Stop takes it back");
    shared_with(&extension, &[]).await;
}

#[tokio::test]
async fn a_tab_not_listed_a_page_aura_may_not_have_a_gone_conversation_or_an_unread_page_shares_nothing()
 {
    let fixture = fixture_with(|_| {});
    let docs = SessionId::new();
    fixture.control.set_sessions(Arc::new(Titles(vec![(docs, "Docs")])));
    let offline = fixture.control.hand_over_tab(docs, 7).await.expect_err("not connected");
    assert!(offline.to_string().starts_with("chrome.not_connected"), "{offline}");
    fixture.enable().await;
    let (extension, _) = fixture.paired().await;
    let extension = PlayedExtension::play(extension, browser());
    for (session, tab_id, code) in [
        (docs, 99, "chrome.tab_gone"),
        (docs, 10, "chrome.tab_gone"),
        (SessionId::new(), 7, "chrome.no_session"),
        (docs, 8, "chrome.refused"),
    ] {
        let refused = fixture.control.hand_over_tab(session, tab_id).await.expect_err(code);
        assert!(refused.to_string().starts_with(code), "{tab_id}: {refused}");
    }
    assert!(fixture.control.tabs(docs).is_empty(), "nothing is shared");
    // The extension leaves the tab it could not read; nobody waits for that.
    let released = ChromeCommand::Release { tab_ids: vec![8] };
    for _ in 0..500 {
        if extension.commands().contains(&released) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let commands = extension.commands();
    assert!(
        commands
            .iter()
            .all(|command| !matches!(command, ChromeCommand::Snapshot { tab_id: 99 | 10, .. })),
        "{commands:?}"
    );
    assert!(commands.contains(&released), "{commands:?}");
    assert!(extension.shares.lock().expect("shares").is_empty());
}
