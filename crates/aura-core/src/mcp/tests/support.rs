//! A temporary settings root and V2 store, and the repository's test MCP server copied to
//! TEMP. Nothing here reads the owner's configuration, starts a real server or calls a model.
use super::super::*;
use aura_config::{AuraPaths, SettingsFacade};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

pub(super) struct Fixture {
    pub _root: tempfile::TempDir,
    pub settings: Arc<SettingsFacade>,
    pub runtime: Arc<McpRuntime>,
    pub context: McpContext,
    pub server_exe: PathBuf,
    /// Where this fixture's MCP secrets live: memory, never the Credential Manager.
    pub secrets: Arc<crate::mcp::credentials::MemorySecretStore>,
    journal: Arc<crate::broker::BrokerJournal>,
    /// Drives Core's network gateway for HTTP servers; dropped last.
    pub _network: Option<tokio::runtime::Runtime>,
}

type Network = (tokio::runtime::Runtime, Arc<aura_network_control::CoreHttpClientGateway>);

impl Fixture {
    pub fn new() -> Self {
        Self::build(None)
    }

    /// With Core's network gateway as in the product; `dns`, when given, replaces the system
    /// resolver (and must then name every host the test reaches).
    pub fn with_network(dns: Vec<aura_network_control::TestFixtureDnsRecord>) -> Self {
        let driver = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("network runtime");
        let mut options = aura_network_control::NetworkControlOptions::default();
        if !dns.is_empty() {
            options = options.with_test_fixture_dns(dns).expect("fixture DNS");
        }
        let gateway = Arc::new(aura_network_control::CoreHttpClientGateway::new(
            Arc::new(aura_network_control::InMemoryNetworkGrantStore::default()),
            None,
            options,
        ));
        Self::build(Some((driver, gateway)))
    }

    fn build(network: Option<Network>) -> Self {
        let root = tempfile::tempdir().expect("temporary MCP fixture");
        let paths = AuraPaths::from_roots(root.path().join("config"), root.path().join("local"));
        let settings = SettingsFacade::open(paths).expect("temporary settings");
        let journal = Arc::new(
            crate::broker::BrokerJournal::open(&root.path().join("journal")).expect("journal"),
        );
        let (driver, network) = match network {
            Some((driver, gateway)) => {
                let runtime = driver.handle().clone();
                (Some(driver), Some(McpNetwork { gateway, runtime }))
            }
            None => (None, None),
        };
        let secrets = Arc::new(crate::mcp::credentials::MemorySecretStore::default());
        let store: Arc<dyn crate::mcp::credentials::SecretStore> = secrets.clone();
        let runtime = McpRuntime::new(Arc::clone(&settings), Arc::clone(&journal), network, store);
        let context = McpContext::new(Arc::clone(&runtime));
        let debug = std::env::current_exe()
            .expect("test image")
            .parent()
            .expect("deps")
            .parent()
            .expect("debug")
            .to_owned();
        let server_exe = root.path().join("servers").join("fixture-server.exe");
        std::fs::create_dir_all(server_exe.parent().expect("folder")).expect("server folder");
        std::fs::copy(debug.join("aura-mcp-fixture.exe"), &server_exe)
            .expect("build current aura-tool-fixtures --bins first");
        Self {
            _root: root,
            settings,
            runtime,
            context,
            server_exe,
            secrets,
            journal,
            _network: driver,
        }
    }

    /// The V2 store the runtime keeps its MCP records in.
    pub fn runtime_journal(&self) -> Arc<crate::broker::BrokerJournal> {
        Arc::clone(&self.journal)
    }

    /// The servers whose tools the agent lists, read the way the agent reads them: inside its
    /// own access to the store, whose lock is not reentrant. Waits at most 20 seconds.
    pub fn listed_inside_store(&self) -> Vec<String> {
        let (journal, runtime) = (self.runtime_journal(), Arc::clone(&self.runtime));
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let store = journal.lock().expect("journal");
            let tools = crate::agent_execution::mcp_tools::usable_tools(
                &runtime,
                store.database.connection(),
            );
            let _ = sender.send(tools.into_iter().map(|tool| tool.server.id).collect::<Vec<_>>());
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("the agent's listing never waits for the store it holds")
    }

    pub fn dispatch(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.context.dispatch(method, params)
    }

    pub fn revision(&self) -> u64 {
        self.dispatch("agent.mcp.list", json!({})).expect("list")["revision"]
            .as_u64()
            .expect("revision")
    }

    /// A stdio server running the fixture with `arguments`.
    pub fn server(&self, id: &str, arguments: &[&str]) -> Value {
        json!({
            "id": id,
            "display_name": format!("测试 {id}"),
            "transport": {
                "type": "stdio",
                "command": self.server_exe.display().to_string(),
                "arguments": arguments,
            },
            "startup_timeout_ms": 10_000,
            "call_timeout_ms": 10_000,
        })
    }

    /// A Streamable HTTP server at `url` with `headers` (name, value).
    pub fn http_server(&self, id: &str, url: &str, headers: &[(&str, &str)]) -> Value {
        let headers: Vec<Value> =
            headers.iter().map(|(name, value)| json!({"name": name, "value": value})).collect();
        json!({
            "id": id,
            "display_name": format!("测试 {id}"),
            "transport": {"type": "http", "url": url, "headers": headers},
            "startup_timeout_ms": 10_000,
            "call_timeout_ms": 10_000,
        })
    }

    pub fn save(&self, server: Value) -> Value {
        self.dispatch(
            "agent.mcp.save",
            json!({"server": server, "replaces": null, "expected_revision": self.revision()}),
        )
        .expect("saved")
    }

    pub fn view(&self, id: &str) -> Value {
        self.dispatch("agent.mcp.list", json!({})).expect("list")["servers"]
            .as_array()
            .expect("servers")
            .iter()
            .find(|server| server["config"]["id"] == id)
            .cloned()
            .expect("listed")
    }

    /// Trust exactly the shown fingerprint, then wait for discovery to settle.
    pub fn trust(&self, id: &str) -> Value {
        let fingerprint = self.view(id)["fingerprint"].clone();
        self.dispatch(
            "agent.mcp.trust",
            json!({"id": id, "expected_fingerprint": fingerprint, "expected_revision": self.revision()}),
        )
        .expect("trusted");
        self.settled(id)
    }

    pub fn settled(&self, id: &str) -> Value {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let view = self.view(id);
            if view["state"] != "starting" || std::time::Instant::now() >= deadline {
                return view;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    pub fn config(&self, id: &str) -> aura_mcp::McpServerConfig {
        serde_json::from_value(self.view(id)["config"].clone()).expect("config")
    }
}
