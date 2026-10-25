//! Work a server began before it was stopped (switched off, untrusted, changed or removed)
//! is dropped. Before, a listing that finished after the server was removed was stored again, and
//! an instance that finished starting after the server was switched off kept running until the
//! idle stop ten minutes later.
use super::support::Fixture;
use aura_domain_common::SessionId;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// The fixture answers `initialize` only after this, so a test can act while it starts.
const SLOW: &[&str] = &["--initialize-delay-ms", "1500"];

#[test]
fn a_listing_that_ends_after_the_server_was_removed_is_not_stored() {
    let f = Fixture::new();
    f.save(f.server("slow", SLOW));
    let fingerprint = f.view("slow")["fingerprint"].clone();
    // Trusting starts the listing in the background; the server answers 1.5 s later.
    f.dispatch(
        "agent.mcp.trust",
        json!({"id": "slow", "expected_fingerprint": fingerprint, "expected_revision": f.revision()}),
    )
    .expect("trusted");
    std::thread::sleep(Duration::from_millis(300));
    f.dispatch("agent.mcp.remove", json!({"id": "slow", "expected_revision": f.revision()}))
        .expect("removed");
    std::thread::sleep(Duration::from_millis(2_500));
    assert!(f.runtime.catalog("slow").is_none(), "the listing came back after the removal");
}

#[test]
fn an_instance_that_was_starting_when_the_server_was_switched_off_is_not_kept() {
    let f = Fixture::new();
    f.save(f.server("slow", SLOW));
    f.trust("slow");
    let server = f.config("slow");
    let runtime = Arc::clone(&f.runtime);
    let starting = std::thread::spawn(move || runtime.connection(SessionId::new(), &server));
    std::thread::sleep(Duration::from_millis(300));
    f.dispatch(
        "agent.mcp.set_enabled",
        json!({"id": "slow", "enabled": false, "expected_revision": f.revision()}),
    )
    .expect("switched off");
    let started = starting.join().expect("start thread");
    assert!(started.is_err(), "an instance started before the switch-off was kept");
    assert_eq!(f.runtime.running_instances("slow"), 0);
}
