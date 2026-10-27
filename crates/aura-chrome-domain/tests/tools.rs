use aura_chrome_domain::*;
use serde_json::json;

#[test]
fn page_tools_take_an_optional_tab_and_the_element_as_ref() {
    let click: ChromeClickInput =
        serde_json::from_value(json!({"tab": "t2", "ref": "e7"})).expect("click with a tab");
    assert_eq!(click.tab.as_deref(), Some("t2"));
    assert_eq!(click.element.as_deref(), Some("e7"));
    let current: ChromeTypeInput =
        serde_json::from_value(json!({"text": "hello"})).expect("type in the current tab");
    assert!(current.tab.is_none() && current.element.is_none());
}

#[test]
fn an_unknown_field_is_refused() {
    assert!(serde_json::from_value::<ChromeSnapshotInput>(json!({"script": "1"})).is_err());
    assert!(serde_json::from_value::<ChromeTabsInput>(json!({"window": 1})).is_err());
    assert!(serde_json::from_value::<ChromeOpenInput>(json!({})).is_err(), "open needs a url");
}

#[test]
fn navigate_takes_an_address_or_a_history_action() {
    let to: ChromeNavigateInput =
        serde_json::from_value(json!({"url": "https://example.com"})).expect("an address");
    assert_eq!(to.url.as_deref(), Some("https://example.com"));
    let back: ChromeNavigateInput =
        serde_json::from_value(json!({"action": "back"})).expect("a history action");
    assert_eq!(back.action, Some(ChromeHistoryAction::Back));
}

#[test]
fn the_tab_list_names_tabs_by_ref_only() {
    let schema = schemars::schema_for!(ChromeTabV1);
    let properties = schema.as_value()["properties"].as_object().expect("properties");
    let mut names: Vec<_> = properties.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(names, ["current", "opened_by_aura", "tab", "title", "url"]);
}
