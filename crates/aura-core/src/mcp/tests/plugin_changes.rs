//! What the user changes in a plugin's server: it is switched off and on, saved with a
//! configuration of the user's own (one that runs something else is trusted on its own), given
//! its plugin's configuration back, and kept across the plugin's update, whose preview names it
//! when the update changes the plugin's own configuration too. Removing and untrusting it stay
//! with the plugin, and nothing is written to `mcp.json`.
use super::plugins::{Plugins, source};
use super::support::Fixture;
use serde_json::{Value, json};

fn server(view: &Value, id: &str) -> Value {
    view["servers"]
        .as_array()
        .and_then(|servers| servers.iter().find(|server| server["config"]["id"] == id).cloned())
        .expect("listed")
}

fn changes(f: &Fixture) -> aura_plugins::PluginChanges {
    f.settings.plugin_registry().plugin("kit").cloned().expect("saved").changes
}

fn save(f: &Fixture, config: &Value) -> anyhow::Result<Value> {
    f.dispatch(
        "agent.mcp.save",
        json!({"server": config, "replaces": "kit-files", "expected_revision": f.revision()}),
    )
}

#[test]
fn a_plugins_server_is_switched_off_and_on_in_its_plugins_entry() {
    let f = Fixture::new();
    let plugins = Plugins::attach(&f);
    plugins.install(&source(&f, "", None, &[], ""));
    let off = f
        .dispatch(
            "agent.mcp.set_enabled",
            json!({"id": "kit-files", "enabled": false, "expected_revision": f.revision()}),
        )
        .expect("switched off");
    assert_eq!(server(&off, "kit-files")["state"], "disabled");
    assert!(changes(&f).switched_off.servers.contains("kit-files"));
    f.dispatch(
        "agent.mcp.set_enabled",
        json!({"id": "kit-files", "enabled": true, "expected_revision": f.revision()}),
    )
    .expect("switched on");
    assert_ne!(f.view("kit-files")["state"], "disabled");
    assert!(changes(&f).is_empty(), "nothing is kept once it is back as the plugin has it");
    assert_eq!(f.settings.mcp_registry().servers.len(), 0, "never written to mcp.json");
}

#[test]
fn a_plugins_server_saved_with_the_users_configuration_is_trusted_on_its_own_and_put_back() {
    let f = Fixture::new();
    let plugins = Plugins::attach(&f);
    plugins.install(&source(&f, "", None, &[], ""));
    let default = f.view("kit-files")["config"].clone();

    // A longer call timeout runs the same program, so the plugin's trust still holds.
    let mut config = default.clone();
    config["call_timeout_ms"] = json!(321_000);
    save(&f, &config).expect("saved");
    let view = f.view("kit-files");
    assert_eq!(view["plugin_changed"], true, "{view}");
    assert_eq!(view["config"]["call_timeout_ms"], 321_000);
    assert_eq!(view["config"]["trusted_fingerprint"], view["fingerprint"], "still trusted");

    // Another argument runs something else: it is trusted on its own, as the user's own are.
    let mut config = view["config"].clone();
    config["transport"]["arguments"] = json!(["--env-tool"]);
    save(&f, &config).expect("saved");
    let changed = f.view("kit-files");
    assert_eq!(changed["state"], "config_changed", "{changed}");
    f.dispatch(
        "agent.mcp.trust",
        json!({"id": "kit-files", "expected_fingerprint": changed["fingerprint"], "expected_revision": f.revision()}),
    )
    .expect("trusted");
    assert_ne!(f.view("kit-files")["state"], "config_changed");

    // It is still removed and untrusted only with its plugin.
    for method in ["agent.mcp.remove", "agent.mcp.untrust"] {
        let refused = f
            .dispatch(method, json!({"id": "kit-files", "expected_revision": f.revision()}))
            .expect_err("with its plugin");
        assert!(refused.to_string().starts_with("mcp.from_plugin"), "{method}: {refused}");
    }

    // The plugin's own configuration comes back.
    f.dispatch(
        "agent.mcp.restore_plugin_default",
        json!({"id": "kit-files", "expected_revision": f.revision()}),
    )
    .expect("restored");
    let back = f.view("kit-files");
    assert_eq!(back["plugin_changed"], Value::Null, "{back}");
    assert_eq!(back["config"]["call_timeout_ms"], default["call_timeout_ms"]);
    assert_eq!(back["config"]["transport"], default["transport"]);
    assert!(changes(&f).servers.is_empty());
    assert_eq!(f.settings.mcp_registry().servers.len(), 0, "never written to mcp.json");
}

#[test]
fn the_users_configuration_stays_across_an_update_that_names_it() {
    let f = Fixture::new();
    let plugins = Plugins::attach(&f);
    plugins.install(&source(&f, "", None, &[], ""));
    let mut config = f.view("kit-files")["config"].clone();
    config["call_timeout_ms"] = json!(321_000);
    save(&f, &config).expect("saved");

    // A newer copy that changes the plugin's own configuration of the same server.
    let newer = source(&f, "", None, &["--env-tool"], "");
    plugins
        .context
        .dispatch(
            "agent.plugins.import",
            json!({"path": newer.display().to_string(), "expected_revision": plugins.revision()}),
        )
        .expect("imported as an update");
    let listed = plugins.context.dispatch("agent.plugins.list", json!({})).expect("list");
    let update = &listed["plugins"][0]["pending_update"];
    assert_eq!(update["changed_servers"], json!(["files"]), "{update}");
    plugins
        .context
        .dispatch(
            "agent.plugins.trust",
            json!({"name": "kit", "pending": true, "expected_fingerprint": update["fingerprint"], "expected_revision": plugins.revision()}),
        )
        .expect("updated");
    let view = f.view("kit-files");
    assert_eq!(view["plugin_changed"], true, "{view}");
    assert_eq!(view["config"]["call_timeout_ms"], 321_000, "the user's configuration stays");
    assert_eq!(view["config"]["trusted_fingerprint"], view["fingerprint"], "{view}");
}
