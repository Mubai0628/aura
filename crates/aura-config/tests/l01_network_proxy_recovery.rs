use std::{fs, net::SocketAddr};

use aura_config::{
    AuraPaths, ConfigModule, NetworkProxyConfigState, SettingsFacade, encode_document,
};
use aura_tool_contract::{ToolNetworkProxyProtocolV1, ToolNetworkProxyV1};
use serde_json::json;

fn proxy(host: &str, port: u16) -> ToolNetworkProxyV1 {
    ToolNetworkProxyV1::new(true, ToolNetworkProxyProtocolV1::Http, host.into(), port)
        .expect("proxy")
}

#[test]
fn proxy_and_localhost_alias_cannot_name_registered_control_endpoints() {
    let endpoints: Vec<SocketAddr> =
        vec!["127.0.0.1:17801".parse().expect("v4"), "[::1]:17802".parse().expect("v6")];
    for (host, port) in
        [("127.0.0.1", 17801), ("localhost", 17801), ("::1", 17802), ("localhost", 17802)]
    {
        assert!(proxy(host, port).validate_control_endpoints(&endpoints).is_err());
    }
    proxy("127.0.0.2", 17801).validate_control_endpoints(&endpoints).expect("exact endpoint only");
    let root = tempfile::tempdir().expect("root");
    let paths = AuraPaths::from_roots(root.path().join("r"), root.path().join("l"));
    let facade = SettingsFacade::open(paths).expect("facade");
    assert!(facade.update_network_proxy(1, Some(proxy("localhost", 17801)), &endpoints).is_err());
    assert_eq!(facade.tool_settings_snapshot().expect("unchanged").revision.value(), 1);
}

#[test]
fn field_recovery_preserves_other_settings_and_enforces_revision_conflicts() {
    let root = tempfile::tempdir().expect("root");
    let paths = AuraPaths::from_roots(root.path().join("r"), root.path().join("l"));
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    facade
        .update(ConfigModule::Tools, 1, |values| {
            values.insert("max_concurrent_tool_calls".into(), json!(8));
            values.insert("workspace_files".into(), json!({"retained": true}));
        })
        .expect("other settings");
    facade.update_network_proxy(2, Some(proxy("127.0.0.1", 17800)), &[]).expect("proxy");
    let mut damaged = (*facade.snapshot(ConfigModule::Tools).expect("document").document).clone();
    damaged.revision = 4;
    damaged.values.insert("network_proxy".into(), json!({"enabled": true, "host": "damaged"}));
    fs::write(
        paths.config_path(ConfigModule::Tools),
        encode_document(ConfigModule::Tools, &damaged).expect("encode"),
    )
    .expect("external field fault");
    assert!(matches!(
        facade.retry_network_proxy().expect("retry"),
        NetworkProxyConfigState::Unavailable(_)
    ));
    let preview = facade.previous_valid_network_proxy().expect("preview").expect("valid history");
    assert_eq!(preview.source_revision, 3);
    assert_eq!(preview.expected_revision, 4);
    facade
        .update_network_proxy(preview.expected_revision, preview.proxy, &[])
        .expect("restore proxy only");
    let restored = facade.snapshot(ConfigModule::Tools).expect("restored");
    assert_eq!(restored.document.values["workspace_files"], json!({"retained": true}));
    assert_eq!(facade.tool_settings_snapshot().expect("settings").max_concurrent_tool_calls, 8);
    assert!(
        facade.update_network_proxy(4, None, &[]).is_err(),
        "stale repair cannot erase a newer save"
    );
    facade.update_network_proxy(5, None, &[]).expect("reset proxy only");
    assert_eq!(facade.network_proxy_state().expect("status"), NetworkProxyConfigState::Disabled);
    assert_eq!(facade.tool_settings_snapshot().expect("still usable").max_concurrent_tool_calls, 8);
}

#[test]
fn without_valid_history_only_the_field_reset_is_available() {
    let root = tempfile::tempdir().expect("root");
    let paths = AuraPaths::from_roots(root.path().join("r"), root.path().join("l"));
    paths.ensure_layout().expect("layout");
    fs::write(
        paths.config_path(ConfigModule::Tools),
        br#"{"schema_version":1,"revision":7,"max_retries":2,"network_proxy":false}"#,
    )
    .expect("broken field");
    let facade = SettingsFacade::open(paths).expect("facade");
    assert!(matches!(
        facade.network_proxy_state().expect("fault"),
        NetworkProxyConfigState::Unavailable(_)
    ));
    assert!(facade.previous_valid_network_proxy().expect("history").is_none());
    facade.update_network_proxy(7, None, &[]).expect("reset field");
    assert_eq!(facade.tool_settings_snapshot().expect("other setting").max_retries, 2);
}
