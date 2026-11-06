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
