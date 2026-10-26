//! Servers a plugin brings. They are listed and run beside the user's own, trusted with the
//! plugin, changed only with it, run under its limits, and their credentials stay until the
//! plugin goes.
use super::support::Fixture;
use crate::mcp::connection::RequestFailure;
use crate::plugins::{PluginContext, PluginRuntime};
use aura_domain_common::SessionId;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn call(name: &str, arguments: Value) -> Option<Value> {
    Some(json!({"name": name, "arguments": arguments}))
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    std::fs::write(path, text).expect("file");
}

/// A plugin `kit` with one stdio server `files`: `command` (the plugin's own copy of the test
/// server when `None`), `arguments` and `extra` lines for the server's transport.
pub(super) fn source(
    f: &Fixture,
    limits: &str,
    command: Option<&Path>,
    arguments: &[&str],
    extra: &str,
) -> PathBuf {
    let root = f._root.path().join("sources").join("kit");
    write(
        &root.join("aura-plugin.toml"),
        &format!("schema_version = 1\nname = \"kit\"\nversion = \"1\"\n{limits}\n"),
    );
    let command = match command {
        Some(path) => path.display().to_string().replace('\\', "\\\\"),
        None => {
            std::fs::create_dir_all(root.join("bin")).expect("bin");
            std::fs::copy(&f.server_exe, root.join("bin").join("server.exe")).expect("server");
            "${PLUGIN_ROOT}/bin/server.exe".to_owned()
        }
    };
    let arguments = arguments.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(", ");
    write(
        &root.join("mcp.json"),
        &format!(
            r#"{{"servers": [{{"name": "files", "transport": {{"type": "stdio", "command": "{command}", "arguments": [{arguments}]{extra}}}}}]}}"#
        ),
    );
    root
}

/// The plugin runtime MCP sees (it holds only a weak link; the context keeps it alive).
/// Waits for the background discovery a plugin's trust starts; the MCP page is never opened.
fn listed(f: &Fixture, id: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while f.runtime.catalog(id).is_none() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

pub(super) struct Plugins {
    pub(super) context: PluginContext,
    runtime: Arc<PluginRuntime>,
}

impl Plugins {
    pub(super) fn attach(f: &Fixture) -> Self {
        let runtime = PluginRuntime::new(Arc::clone(&f.settings), Some(Arc::clone(&f.runtime)));
        f.runtime.attach_plugins(&runtime);
        Self { context: PluginContext::new(Arc::clone(&runtime)), runtime }
    }

    pub(super) fn revision(&self) -> u64 {
        self.context.dispatch("agent.plugins.list", json!({})).expect("list")["revision"]
            .as_u64()
            .expect("revision")
    }

    pub(super) fn install(&self, source: &Path) {
        self.context
            .dispatch(
                "agent.plugins.import",
                json!({"path": source.display().to_string(), "expected_revision": self.revision()}),
            )
            .expect("imported");
        let preview = self
            .context
            .dispatch("agent.plugins.preview", json!({"name": "kit"}))
            .expect("preview");
        self.context
            .dispatch(
                "agent.plugins.trust",
                json!({"name": "kit", "expected_fingerprint": preview["fingerprint"], "expected_revision": self.revision()}),
            )
            .expect("trusted");
    }

    fn change(&self, method: &str) -> anyhow::Result<Value> {
        self.context.dispatch(method, json!({"name": "kit", "expected_revision": self.revision()}))
    }
}

#[test]
fn a_trusted_plugins_server_is_listed_and_answers_as_the_users_do() {
    let f = Fixture::new();
    let plugins = Plugins::attach(&f);
    plugins.install(&source(&f, "", None, &[], ""));
    let view = f.view("kit-files");
    assert_eq!(view["plugin"], "kit", "{view}");
    assert!(view["problems"].as_array().is_some_and(Vec::is_empty), "{view}");
    assert_eq!(view["config"]["trusted_fingerprint"], view["fingerprint"]);
    let connection = f.runtime.connection(SessionId::new(), &f.config("kit-files")).expect("runs");
    let answered = connection
        .request("tools/call", call("echo", json!({"text": "插件"})), Duration::from_secs(10), None)
        .expect("answered");
    assert_eq!(answered["content"][0]["text"], "插件");
    assert_eq!(f.settings.mcp_registry().servers.len(), 0, "never written to mcp.json");
    plugins.change("agent.plugins.untrust").expect("untrusted");
    let listed = f.dispatch("agent.mcp.list", json!({})).expect("list");
    assert!(listed["servers"].as_array().is_some_and(Vec::is_empty), "gone with its plugin");
}

#[test]
fn a_trusted_plugins_server_lists_its_tools_without_the_mcp_page() {
    let f = Fixture::new();
    let plugins = Plugins::attach(&f);
    plugins.install(&source(&f, "", None, &[], ""));
    assert!(listed(&f, "kit-files"), "discovered when the plugin was trusted");
    let catalog = f.runtime.catalog("kit-files").expect("catalog");
    assert!(catalog.catalog.tools.iter().any(|tool| tool.canonical_name == "mcp.kit-files.echo"));
}

#[test]
fn a_plugins_server_is_untrusted_and_removed_only_with_its_plugin() {
    let f = Fixture::new();
    let plugins = Plugins::attach(&f);
    plugins.install(&source(&f, "", None, &[], ""));
    for method in ["agent.mcp.untrust", "agent.mcp.remove"] {
        let refused =
            f.dispatch(method, json!({"id": "kit-files", "expected_revision": f.revision()}));
        assert!(refused.expect_err(method).to_string().starts_with("mcp.from_plugin"));
    }
    let copy = f.server("kit-files", &[]);
    let taken = f.dispatch(
        "agent.mcp.save",
        json!({"server": copy, "replaces": null, "expected_revision": f.revision()}),
    );
    assert!(taken.expect_err("taken").to_string().contains("来自插件 kit"));
}

#[test]
fn a_changed_outside_program_needs_its_server_trusted_again_and_the_plugin_keeps_that() {
    let f = Fixture::new();
    let plugins = Plugins::attach(&f);
    plugins.install(&source(&f, "", Some(&f.server_exe), &[], ""));
    assert_eq!(
        f.view("kit-files")["config"]["trusted_fingerprint"],
        f.view("kit-files")["fingerprint"]
    );
    // Listing started discovery for the trusted server: let it settle and stop it, so its
    // program can change the way the user updates it while nothing runs.
    f.settled("kit-files");
    f.runtime.stop_server("kit-files");
    let mut bytes = std::fs::read(&f.server_exe).expect("program");
    bytes.extend_from_slice(b"updated");
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Err(error) = std::fs::write(&f.server_exe, &bytes) {
        assert!(Instant::now() < deadline, "the stopped program is released: {error}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let changed = f.view("kit-files");
    assert_eq!(changed["state"], "config_changed", "{changed}");
    f.dispatch(
        "agent.mcp.trust",
        json!({"id": "kit-files", "expected_fingerprint": changed["fingerprint"], "expected_revision": f.revision()}),
    )
    .expect("trusted again");
    let saved = f.settings.plugin_registry().plugin("kit").cloned().expect("saved");
    assert_eq!(
        saved.trusted_parts.servers.get("kit-files").map(String::as_str),
        changed["fingerprint"].as_str()
    );
    assert_ne!(f.view("kit-files")["state"], "config_changed");
}

#[test]
fn a_plugins_secret_survives_a_sweep_and_goes_with_the_plugin() {
    let f = Fixture::new();
    let plugins = Plugins::attach(&f);
    plugins.install(&source(
        &f,
        "",
        None,
        &["--env-tool"],
        r#", "secret_environment": ["FIXTURE_TOKEN"]"#,
    ));
    let saved = f.settings.plugin_registry().plugin("kit").cloned().expect("saved");
    let secret = saved.secret("kit-files", "FIXTURE_TOKEN").expect("a credential for the secret");
    f.runtime
        .secrets()
        .set(
            "kit-files",
            Some(secret),
            &aura_windows_platform::SecretBytes::new(b"plugin-secret".to_vec()),
        )
        .expect("set");
    // As the secret route does once the value is saved.
    f.runtime.secret_written("kit-files");
    assert_eq!(f.secrets.len(), 1);
    f.runtime.sweep_credentials();
    assert_eq!(f.secrets.len(), 1, "named by plugins.json, kept");
    let connection = f.runtime.connection(SessionId::new(), &f.config("kit-files")).expect("runs");
    let seen = connection
        .request(
            "tools/call",
            call("env", json!({"name": "FIXTURE_TOKEN"})),
            Duration::from_secs(10),
            None,
        )
        .expect("answered");
    assert_eq!(seen["content"][0]["text"], "FIXTURE_TOKEN: set, 13 characters");
    assert!(listed(&f, "kit-files"), "its tools were listed");
    plugins.change("agent.plugins.uninstall").expect("uninstalled");
    assert_eq!(f.secrets.len(), 0, "the plugin's secret went with it");
    assert!(f.runtime.catalog("kit-files").is_none(), "and so did its tool catalog");
}

#[test]
fn a_plugins_server_runs_under_the_plugins_memory_limit() {
    let f = Fixture::new();
    let plugins = Plugins::attach(&f);
    plugins.install(&source(&f, "[limits]\nmemory_mb = 96", None, &["--allocate-tool"], ""));
    let session = SessionId::new();
    let connection = f.runtime.connection(session, &f.config("kit-files")).expect("runs");
    let small = connection
        .request("tools/call", call("allocate", json!({"mb": 16})), Duration::from_secs(20), None)
        .expect("within the limit");
    assert_eq!(small["content"][0]["text"], "held 16 MiB");
    let large = connection.request(
        "tools/call",
        call("allocate", json!({"mb": 512})),
        Duration::from_secs(20),
        None,
    );
    let Err(RequestFailure::Lost(reason)) = large else {
        panic!("the server ends at its limit: {large:?}");
    };
    assert!(reason.contains("内存上限"), "{reason}");
}

#[test]
fn a_plugin_is_uninstalled_or_updated_while_its_servers_tools_are_still_being_listed() {
    let f = Fixture::new();
    let plugins = Plugins::attach(&f);
    // Slow to answer: the listing its trust starts still runs the program in the plugin's folder
    // when the folder is moved.
    let slow = ["--initialize-delay-ms", "2000"];
    let listing = Duration::from_millis(500);
    plugins.install(&source(&f, "", None, &slow, ""));
    std::thread::sleep(listing);
    plugins.change("agent.plugins.uninstall").expect("uninstalled once the listing let go");
    assert!(!plugins.runtime.folder("kit").exists());

    plugins.install(&source(&f, "", None, &slow, ""));
    std::thread::sleep(listing);
    let newer = source(&f, "", None, &["--initialize-delay-ms", "2000", "--env-tool"], "");
    plugins
        .context
        .dispatch(
            "agent.plugins.import",
            json!({"path": newer.display().to_string(), "expected_revision": plugins.revision()}),
        )
        .expect("imported as an update");
    let listed = plugins.context.dispatch("agent.plugins.list", json!({})).expect("list");
    let update = &listed["plugins"][0]["pending_update"];
    plugins
        .context
        .dispatch(
            "agent.plugins.trust",
            json!({"name": "kit", "pending": true, "expected_fingerprint": update["fingerprint"], "expected_revision": plugins.revision()}),
        )
        .expect("updated once the listing let go");
}
