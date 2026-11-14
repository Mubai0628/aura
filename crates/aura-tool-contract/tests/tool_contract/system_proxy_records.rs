//! Settings records written before tools could follow the system proxy still decode, and records
//! written while the choice is left alone carry nothing an older Core would refuse.
use aura_tool_contract::{AgentToolSettingsUpdate, AgentToolSettingsView, ToolSettingsSnapshotV1};
use serde_json::json;

fn old_update() -> serde_json::Value {
    json!({
        "expected_revision": "7",
        "fixed_tool_ids": [],
        "max_dynamic_tool_schemas": 50,
        "max_concurrent_tool_calls": 4,
        "default_timeout_ms": 120_000,
        "max_retries": 3,
        "use_recommended_fixed_tool_ids": false,
        "network_proxy": {"enabled": true, "protocol": "socks5", "host": "127.0.0.1", "port": 10808}
    })
}

fn old_view() -> serde_json::Value {
    json!({
        "revision": "7",
        "max_dynamic_tool_schemas": 50,
        "max_concurrent_tool_calls": 4,
        "default_timeout_ms": 120_000,
        "max_retries": 3,
        "fixed_tool_ids": [],
        "tools": [],
        "network_proxy": {"enabled": true, "protocol": "socks5", "host": "127.0.0.1", "port": 10808}
    })
}

#[test]
fn an_update_recorded_before_the_choice_decodes_and_leaves_it_alone() {
    let update: AgentToolSettingsUpdate =
        serde_json::from_value(old_update()).expect("an update recorded by an older Core");
    assert_eq!(update.network_proxy_follows_system, None, "absent stays absent");
    assert_eq!(serde_json::to_value(&update).expect("encode"), old_update(), "same bytes back");
}

#[test]
fn a_view_recorded_before_the_choice_decodes_as_not_following() {
    let view: AgentToolSettingsView =
        serde_json::from_value(old_view()).expect("a view recorded by an older Core");
    assert!(!view.network_proxy_follows_system);
    assert_eq!(view.system_network_proxy, None);
    let encoded = serde_json::to_value(&view).expect("encode");
    assert!(encoded.get("network_proxy_follows_system").is_none());
    assert!(encoded.get("system_network_proxy").is_none());
}

#[test]
fn a_step_that_follows_the_system_freezes_the_field_older_cores_know() {
    // The proxy Windows names is frozen as the step's ordinary proxy: no new field in the snapshot.
    let proxy = aura_tool_contract::ToolNetworkProxyV1::new(
        true,
        aura_tool_contract::ToolNetworkProxyProtocolV1::Http,
        "127.0.0.1".into(),
        7890,
    )
    .expect("a loopback proxy");
    let snapshot =
        ToolSettingsSnapshotV1::default().with_network_proxy(Some(proxy)).expect("frozen");
    let fields = serde_json::to_value(&snapshot).expect("encode");
    let mut names: Vec<_> = fields.as_object().expect("snapshot object").keys().cloned().collect();
    names.sort();
    assert_eq!(
        names,
        [
            "content_hash",
            "default_timeout_ms",
            "max_concurrent_tool_calls",
            "max_dynamic_tool_schemas",
            "max_retries",
            "network_proxy",
            "revision"
        ]
    );
}
