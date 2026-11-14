use serde_json::json;

/// The settings page reaches Chrome control through Core's own route table.
#[tokio::test]
async fn the_settings_page_reads_chrome_control_and_cannot_pair_while_it_is_off() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let status = crate::r03_tests::success(&runtime, "chrome.status", json!({})).await;
    assert_eq!(status["enabled"], json!(false));
    assert_eq!(status["connection"], json!("off"));
    assert_eq!(status["paired"], json!(null));
    let refused = crate::r03_tests::request(&runtime, "chrome.pair.begin", json!({})).await;
    assert!(refused.to_string().contains("chrome.off"), "{refused}");
}

/// The @ menu reaches Chrome control through Core's route table too: with Chrome control off it
/// lists no tab, and handing one over shares nothing.
#[tokio::test]
async fn the_at_menu_lists_no_chrome_tab_and_hands_none_over_while_chrome_control_is_off() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let tabs = crate::r03_tests::success(&runtime, "chrome.tabs.list", json!({})).await;
    assert_eq!(tabs, json!({ "connected": false, "tabs": [], "issue": null }));
    let session = aura_domain_common::SessionId::new();
    let handed = json!({ "session_id": session, "tab_id": 7 });
    let refused = crate::r03_tests::request(&runtime, "chrome.tabs.hand_over", handed).await;
    assert!(refused.to_string().contains("chrome.not_connected"), "{refused}");
}
