use aura_domain_common::{Revision, jcs::canonical_sha256};
use aura_tool_contract::ToolSettingsSnapshotV1;
use serde_json::{Value, json};

fn legacy_snapshot() -> Value {
    let material = json!({
        "revision": "7",
        "max_dynamic_tool_schemas": 50,
        "max_concurrent_tool_calls": 4,
        "default_timeout_ms": 120_000,
        "max_retries": 3
    });
    let mut wire = material;
    wire["content_hash"] = json!({
        "algorithm": "sha256", "canonicalization_version": "rfc8785_jcs_v1",
        "hex": "4b22864ad46cd1abac2f3461051be9d2777099e3f78f317b39c35b97f2c24d85"
    });
    wire
}

fn with_proxy(proxy: Value) -> Value {
    let mut wire = legacy_snapshot();
    wire["network_proxy"] = proxy;
    let mut material = wire.clone();
    material.as_object_mut().expect("snapshot object").remove("content_hash");
    wire["content_hash"] = serde_json::to_value(canonical_sha256(&material).expect("new hash"))
        .expect("serialize hash");
    wire
}

#[test]
fn old_snapshot_round_trips_with_identical_hash_and_no_proxy_field() {
    let old = legacy_snapshot();
    let snapshot: ToolSettingsSnapshotV1 = serde_json::from_value(old.clone()).expect("old wire");
    snapshot.validate().expect("old hash remains valid");
    assert_eq!(serde_json::to_value(snapshot).expect("serialize"), old);
    let defaults = serde_json::to_value(ToolSettingsSnapshotV1::default()).expect("defaults");
    assert!(defaults.get("network_proxy").is_none(), "missing means no proxy");
}

#[test]
fn frozen_proxy_is_hash_bound_and_cannot_be_changed_in_place() {
    let mut wire = with_proxy(json!({
        "enabled": true, "protocol": "socks5", "host": "127.0.0.1", "port": 10808
    }));
    let snapshot: ToolSettingsSnapshotV1 =
        serde_json::from_value(wire.clone()).expect("valid frozen proxy");
    assert_eq!(snapshot.revision, Revision::new(7));
    assert_eq!(serde_json::to_value(snapshot).expect("serialize"), wire);
    assert_ne!(wire["content_hash"], legacy_snapshot()["content_hash"]);
    wire["network_proxy"]["port"] = json!(10809);
    assert!(serde_json::from_value::<ToolSettingsSnapshotV1>(wire).is_err(), "tampering");
}

#[test]
fn only_explicit_loopback_hosts_and_two_protocols_are_accepted() {
    for host in ["127.0.0.1", "127.2.3.4", "::1", "localhost"] {
        for protocol in ["http", "socks5"] {
            serde_json::from_value::<ToolSettingsSnapshotV1>(with_proxy(json!({
                "enabled": true, "protocol": protocol, "host": host, "port": 65535
            })))
            .expect("supported loopback proxy");
        }
    }
    for host in [
        "10.0.0.1",
        "8.8.8.8",
        "0.0.0.0",
        "::",
        "::ffff:127.0.0.1",
        "example.test",
        "user@localhost",
        "localhost/path",
    ] {
        assert!(
            serde_json::from_value::<ToolSettingsSnapshotV1>(with_proxy(json!({
                "enabled": true, "protocol": "http", "host": host, "port": 10809
            })))
            .is_err(),
            "invalid host {host}"
        );
    }
    for port in [0, 65536] {
        assert!(
            serde_json::from_value::<ToolSettingsSnapshotV1>(with_proxy(json!({
                "enabled": true, "protocol": "http", "host": "127.0.0.1", "port": port
            })))
            .is_err()
        );
    }
    for protocol in ["https", "socks4", "socks5h"] {
        assert!(
            serde_json::from_value::<ToolSettingsSnapshotV1>(with_proxy(json!({
                "enabled": true, "protocol": protocol, "host": "127.0.0.1", "port": 1
            })))
            .is_err()
        );
    }
    assert!(
        serde_json::from_value::<ToolSettingsSnapshotV1>(with_proxy(json!({
            "enabled": true, "protocol": "http", "host": "localhost", "port": 1,
            "username": "unauthorized"
        })))
        .is_err(),
        "proxy authentication is unsupported"
    );
}
