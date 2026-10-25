//! Settings → MCP: what is saved, what trust covers, and what each state means.
use super::support::Fixture;
use serde_json::json;

#[test]
fn a_new_server_starts_untrusted_and_off_and_trust_lists_its_tools() {
    let f = Fixture::new();
    let mut server = f.server("files", &[]);
    server["enabled"] = json!(true);
    server["trusted_fingerprint"] = json!("0".repeat(64));
    f.save(server);
    let view = f.view("files");
    assert_eq!(view["state"], "not_trusted", "{view}");
    assert_eq!(view["config"]["enabled"], false, "a save never switches a server on");
    assert!(view["config"]["trusted_fingerprint"].is_null(), "a save never carries trust");
    assert_eq!(view["resolved_command"]["path"], f.server_exe.display().to_string());
    let ready = f.trust("files");
    assert_eq!(ready["state"], "ready", "{ready}");
    assert_eq!(ready["tool_count"], 5);
    assert_eq!(ready["server_name"], "aura-mcp-fixture");
    let detail = f.dispatch("agent.mcp.describe", json!({"id": "files"})).expect("detail");
    let names: Vec<_> = detail["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["canonical_name"].as_str().expect("name").to_owned())
        .collect();
    assert_eq!(
        names,
        ["mcp.files.echo", "mcp.files.slow", "mcp.files.fail", "mcp.files.crash", "mcp.files.grow"]
    );
    assert_eq!(f.runtime.running_instances("files"), 0, "discovery stops its server");
}

#[test]
fn an_http_servers_trust_covers_its_address_and_headers() {
    let f = Fixture::new();
    let remote = |team: &str| {
        json!({
            "id": "remote",
            "display_name": "远程",
            "transport": {
                "type": "http",
                "url": "https://mcp.example.com/mcp",
                "headers": [{"name": "X-Team", "value": team}],
            },
        })
    };
    f.save(remote("docs"));
    let view = f.view("remote");
    assert_eq!(view["state"], "not_trusted", "{view}");
    assert_eq!(view["problems"], json!([]), "{view}");
    let shown = view["fingerprint"].clone();
    assert!(shown.is_string(), "{view}");
    // Without Core's network gateway nothing is reached, and the reason is shown.
    let failed = f.trust("remote");
    assert_eq!(failed["state"], "failed", "{failed}");
    assert!(failed["last_error"].as_str().is_some_and(|e| e.contains("网络网关")), "{failed}");
    // Another header value is another configuration: it is no longer trusted.
    f.dispatch(
        "agent.mcp.save",
        json!({"server": remote("ops"), "replaces": "remote", "expected_revision": f.revision()}),
    )
    .expect("saved");
    let changed = f.view("remote");
    assert_eq!(changed["state"], "config_changed", "{changed}");
    assert_ne!(changed["fingerprint"], shown);
    assert_eq!(f.runtime.running_instances("remote"), 0);
}

#[test]
fn trust_is_only_given_at_the_shown_fingerprint_and_lost_when_the_program_changes() {
    let f = Fixture::new();
    f.save(f.server("files", &[]));
    let shown = f.view("files")["fingerprint"].clone();
    // The program is replaced after the user looked at it.
    let mut bytes = std::fs::read(&f.server_exe).expect("program");
    bytes.extend_from_slice(b"changed");
    std::fs::write(&f.server_exe, &bytes).expect("replaced program");
    let stale = f.dispatch(
        "agent.mcp.trust",
        json!({"id": "files", "expected_fingerprint": shown, "expected_revision": f.revision()}),
    );
    assert!(stale.expect_err("stale").to_string().starts_with("mcp.config_changed"));
    assert_eq!(f.view("files")["state"], "not_trusted");
    f.trust("files");
    // Editing what runs keeps the old trust, which no longer matches.
    f.dispatch(
        "agent.mcp.save",
        json!({"server": f.server("files", &["--stderr", "changed"]), "replaces": "files", "expected_revision": f.revision()}),
    )
    .expect("edited");
    let view = f.view("files");
    assert_eq!(view["state"], "config_changed", "{view}");
    assert!(f.runtime.usable(&f.config("files")).is_err());
    let renamed = f.dispatch(
        "agent.mcp.save",
        json!({"server": f.server("other", &[]), "replaces": "files", "expected_revision": f.revision()}),
    );
    assert!(renamed.expect_err("fixed id").to_string().starts_with("mcp.id_fixed"));
}

#[test]
fn secrets_relative_programs_and_stale_revisions_are_refused() {
    let f = Fixture::new();
    let mut secret = f.server("files", &[]);
    secret["transport"]["environment"] = json!([{"name": "GITHUB_TOKEN", "value": "ghp_x"}]);
    let refused = f.dispatch(
        "agent.mcp.save",
        json!({"server": secret, "replaces": null, "expected_revision": f.revision()}),
    );
    assert!(refused.expect_err("secret").to_string().contains("密钥"));
    let mut relative = f.server("files", &[]);
    relative["transport"]["command"] = json!("servers\\fixture-server.exe");
    f.save(relative);
    let view = f.view("files");
    assert_eq!(view["state"], "invalid", "{view}");
    assert!(view["fingerprint"].is_null());
    let stale = f.dispatch(
        "agent.mcp.save",
        json!({"server": f.server("second", &[]), "replaces": null, "expected_revision": 0}),
    );
    assert!(stale.expect_err("stale").to_string().starts_with("mcp.revision_changed"));
}

#[test]
fn switching_off_untrusting_and_removing_stop_its_instances_and_keep_others() {
    let f = Fixture::new();
    f.save(f.server("first", &[]));
    f.save(f.server("second", &[]));
    f.trust("first");
    f.trust("second");
    let session = aura_domain_common::SessionId::new();
    let first = f.runtime.connection(session, &f.config("first")).expect("first runs");
    let second = f.runtime.connection(session, &f.config("second")).expect("second runs");
    f.dispatch(
        "agent.mcp.set_enabled",
        json!({"id": "first", "enabled": false, "expected_revision": f.revision()}),
    )
    .expect("off");
    assert!(!first.alive(), "switching off stops its instance");
    assert!(second.alive(), "another server is unaffected");
    assert_eq!(f.view("first")["state"], "disabled");
    f.dispatch("agent.mcp.untrust", json!({"id": "second", "expected_revision": f.revision()}))
        .expect("untrusted");
    assert!(!second.alive());
    assert_eq!(f.view("second")["state"], "not_trusted");
    f.dispatch("agent.mcp.remove", json!({"id": "first", "expected_revision": f.revision()}))
        .expect("removed");
    let list = f.dispatch("agent.mcp.list", json!({})).expect("list");
    assert_eq!(list["servers"].as_array().map(Vec::len), Some(1));
    assert!(f.runtime.catalog("first").is_none(), "a removed server's tools are forgotten");
    assert!(f.server_exe.exists(), "removing never touches the program");
}
