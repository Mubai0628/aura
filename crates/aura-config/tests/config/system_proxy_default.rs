//! Tools and model requests follow the system's proxy unless a proxy is switched on in Aura's own
//! settings, which then replaces it for both. The system's values here are injected, never the
//! machine's own; a test build reads none until a test gives some.
use std::{fs, sync::Arc};

use aura_config::{
    AuraPaths, ConfigModule, ModuleConfigDocument, SettingsFacade, TOOLS_PROXY_FIELD,
    encode_document,
};
use aura_system_proxy::{FixedSystemProxy, InternetSettingsValues};
use aura_tool_contract::{ToolNetworkProxyProtocolV1, ToolNetworkProxyV1};
use serde_json::{Value, json};

fn paths(root: &tempfile::TempDir) -> AuraPaths {
    AuraPaths::from_roots(root.path().join("roaming"), root.path().join("local"))
}

fn system(server: &str) -> Arc<FixedSystemProxy> {
    Arc::new(FixedSystemProxy(Ok(InternetSettingsValues {
        proxy_enable: Some(1),
        proxy_server: Some(server.to_owned()),
        auto_config_url: None,
    })))
}

fn own(enabled: bool) -> Value {
    json!({"enabled": enabled, "protocol": "socks5", "host": "127.0.0.1", "port": 10808})
}

fn http(port: u16) -> ToolNetworkProxyV1 {
    ToolNetworkProxyV1::new(true, ToolNetworkProxyProtocolV1::Http, "127.0.0.1".into(), port)
        .expect("proxy")
}

fn frozen_proxy(facade: &SettingsFacade) -> Option<ToolNetworkProxyV1> {
    facade.tool_settings_snapshot().expect("tool settings").network_proxy
}

#[test]
fn a_test_build_reads_no_system_proxy_until_a_test_gives_one() {
    let root = tempfile::tempdir().expect("root");
    let facade = SettingsFacade::open(paths(&root)).expect("facade");
    assert_eq!(facade.system_proxy().proxy(), None);
    assert_eq!(frozen_proxy(&facade), None);
    assert_eq!(facade.model_proxy(), None);
}

#[test]
fn tools_and_model_requests_follow_the_system_while_no_proxy_is_switched_on() {
    let root = tempfile::tempdir().expect("root");
    let facade = SettingsFacade::open(paths(&root)).expect("facade");
    facade.use_system_proxy_source(system("127.0.0.1:10809"));
    assert_eq!(frozen_proxy(&facade), Some(http(10809)), "followed with nothing set");
    assert_eq!(facade.model_proxy(), Some(http(10809)));
    facade
        .tool_settings_snapshot()
        .expect("snapshot")
        .validate()
        .expect("a frozen snapshot that validates, hash included");

    // A proxy typed in but switched off changes nothing.
    facade
        .update(ConfigModule::Tools, 1, |values| {
            values.insert("network_proxy".into(), own(false));
        })
        .expect("the typed-in proxy, switched off");
    assert_eq!(frozen_proxy(&facade), Some(http(10809)));
    assert_eq!(facade.model_proxy(), Some(http(10809)));

    // What the system names is read for each step and each model request, not once.
    facade.use_system_proxy_source(system("127.0.0.1:7890"));
    assert_eq!(frozen_proxy(&facade).map(|proxy| proxy.port), Some(7890));
    assert_eq!(facade.model_proxy().map(|proxy| proxy.port), Some(7890));
    // One Aura cannot use leaves both direct, never on the typed-in proxy.
    facade.use_system_proxy_source(system("proxy.corp.example:8080"));
    assert_eq!(frozen_proxy(&facade), None);
    assert_eq!(facade.model_proxy(), None);
}

#[test]
fn a_proxy_switched_on_replaces_the_systems_for_tools_and_model_requests() {
    let root = tempfile::tempdir().expect("root");
    let facade = SettingsFacade::open(paths(&root)).expect("facade");
    facade.use_system_proxy_source(system("127.0.0.1:7890"));
    facade
        .update(ConfigModule::Tools, 1, |values| {
            values.insert("network_proxy".into(), own(true));
        })
        .expect("the typed-in proxy, switched on");
    let typed = ToolNetworkProxyV1::new(
        true,
        ToolNetworkProxyProtocolV1::Socks5,
        "127.0.0.1".into(),
        10808,
    )
    .expect("proxy");
    assert_eq!(frozen_proxy(&facade), Some(typed.clone()));
    assert_eq!(facade.model_proxy(), Some(typed.clone()));
    // Aura's own update checks read the proxy as stored.
    assert_eq!(facade.stored_tool_settings_snapshot().expect("stored").network_proxy, Some(typed));
}

#[test]
fn an_older_cores_follow_choice_still_reads_and_no_longer_decides() {
    let root = tempfile::tempdir().expect("root");
    let paths = paths(&root);
    paths.ensure_layout().expect("layout");
    let mut network = ModuleConfigDocument::empty();
    network.revision = 2;
    network.values.insert(TOOLS_PROXY_FIELD.into(), json!("system"));
    fs::write(
        paths.config_path(ConfigModule::Network),
        encode_document(ConfigModule::Network, &network).expect("encode"),
    )
    .expect("an older Core's choice");
    let mut tools = ModuleConfigDocument::empty();
    tools.revision = 4;
    tools.values.insert("network_proxy".into(), own(true));
    tools.values.insert("max_retries".into(), json!(2));
    fs::write(
        paths.config_path(ConfigModule::Tools),
        encode_document(ConfigModule::Tools, &tools).expect("encode"),
    )
    .expect("an older Tools file");
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    facade.use_system_proxy_source(system("127.0.0.1:7890"));
    let snapshot = facade.tool_settings_snapshot().expect("snapshot");
    assert_eq!(snapshot.max_retries, 2);
    assert_eq!(snapshot.network_proxy.map(|proxy| proxy.port), Some(10808), "the one switched on");
    // A damaged choice is only a fault of that file; tools still follow the system.
    let mut damaged = ModuleConfigDocument::empty();
    damaged.revision = 3;
    damaged.values.insert(TOOLS_PROXY_FIELD.into(), json!("pac"));
    fs::write(
        paths.config_path(ConfigModule::Network),
        encode_document(ConfigModule::Network, &damaged).expect("encode"),
    )
    .expect("damaged network.toml");
    let mut switched_off = tools.clone();
    switched_off.values.insert("network_proxy".into(), own(false));
    fs::write(
        paths.config_path(ConfigModule::Tools),
        encode_document(ConfigModule::Tools, &switched_off).expect("encode"),
    )
    .expect("tools with the proxy switched off");
    let reopened = SettingsFacade::open(paths).expect("facade opens");
    reopened.use_system_proxy_source(system("127.0.0.1:7890"));
    assert_eq!(frozen_proxy(&reopened).map(|proxy| proxy.port), Some(7890));
    assert!(reopened.tool_settings_snapshot().is_ok(), "Tools stays usable");
}
