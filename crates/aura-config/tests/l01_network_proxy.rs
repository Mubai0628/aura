use std::fs;

use aura_config::{
    AuraPaths, ConfigModule, DomainHealthState, ModuleConfigDocument, SettingsFacade,
    encode_document,
};
use serde_json::{Value, json};

fn proxy() -> Value {
    json!({"enabled": true, "protocol": "http", "host": "127.0.0.1", "port": 10809})
}

fn paths(root: &tempfile::TempDir) -> AuraPaths {
    AuraPaths::from_roots(root.path().join("roaming"), root.path().join("local"))
}

#[test]
fn tools_save_freezes_proxy_and_later_edits_do_not_mutate_the_snapshot() {
    let root = tempfile::tempdir().expect("root");
    let facade = SettingsFacade::open(paths(&root)).expect("facade");
    let default =
        serde_json::to_value(facade.tool_settings_snapshot().expect("defaults")).expect("wire");
    assert!(default.get("network_proxy").is_none());
    facade
        .update(ConfigModule::Tools, 1, |values| {
            values.insert("network_proxy".into(), proxy());
            values.insert("max_concurrent_tool_calls".into(), json!(8));
        })
        .expect("save valid proxy");
    let frozen = facade.tool_settings_snapshot().expect("frozen");
    facade
        .update(ConfigModule::Tools, 2, |values| {
            values.get_mut("network_proxy").expect("proxy")["enabled"] = json!(false);
        })
        .expect("disable later");
    let frozen_wire = serde_json::to_value(&frozen).expect("frozen wire");
    assert_eq!(frozen_wire["network_proxy"]["enabled"], true);
    assert_eq!(frozen.max_concurrent_tool_calls, 8);
    let later = facade.tool_settings_snapshot().expect("later");
    assert_ne!(later.content_hash, frozen.content_hash);
    assert_eq!(serde_json::to_value(later).expect("later wire")["network_proxy"]["enabled"], false);
}

#[test]
fn invalid_proxy_save_reports_the_field_and_leaves_tools_unchanged() {
    let root = tempfile::tempdir().expect("root");
    let facade = SettingsFacade::open(paths(&root)).expect("facade");
    for (field, value) in
        [("host", json!("10.0.0.1")), ("port", json!(0)), ("protocol", json!("socks4"))]
    {
        let mut invalid = proxy();
        invalid[field] = value;
        let error = facade
            .update(ConfigModule::Tools, 1, |values| {
                values.insert("network_proxy".into(), invalid);
            })
            .expect_err("invalid proxy");
        assert!(error.to_string().contains(&format!("network_proxy.{field}")), "{error}");
        assert_eq!(facade.tool_settings_snapshot().expect("tools usable").revision.value(), 1);
    }
}

#[test]
fn damaged_proxy_file_behaves_as_disabled_without_losing_other_tool_settings() {
    let root = tempfile::tempdir().expect("root");
    let paths = paths(&root);
    paths.ensure_layout().expect("layout");
    let mut document = ModuleConfigDocument::empty();
    document.revision = 7;
    document.values.insert("network_proxy".into(), json!({"enabled": true, "host": "broken"}));
    document.values.insert("max_concurrent_tool_calls".into(), json!(8));
    document.values.insert("max_retries".into(), json!(2));
    let raw = encode_document(ConfigModule::Tools, &document).expect("encode");
    fs::write(paths.config_path(ConfigModule::Tools), &raw).expect("damaged proxy file");
    let facade = SettingsFacade::open(paths.clone()).expect("facade opens");
    assert_eq!(facade.health(ConfigModule::Tools).state, DomainHealthState::Ready);
    let settings = facade.tool_settings_snapshot().expect("tools remain usable");
    assert_eq!(settings.max_concurrent_tool_calls, 8);
    assert_eq!(settings.max_retries, 2);
    assert!(serde_json::to_value(settings).expect("wire").get("network_proxy").is_none());
    assert_eq!(fs::read(paths.config_path(ConfigModule::Tools)).expect("raw preserved"), raw);
    facade
        .update(ConfigModule::Tools, 7, |values| {
            values.insert("max_retries".into(), json!(1));
        })
        .expect("unrelated Tools setting can still be saved");
    assert_eq!(facade.tool_settings_snapshot().expect("updated").max_retries, 1);
}
