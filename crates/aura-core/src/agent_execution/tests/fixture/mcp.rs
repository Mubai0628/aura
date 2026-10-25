//! Sessions whose registry holds the tools of trusted MCP servers. The servers are the
//! repository's test MCP server, configured and trusted through the real `agent.mcp.*` routes
//! of the Core under test; nothing reads the owner's configuration.
use super::*;
use serde_json::json;

/// The test server copied into the fixture root, so the trusted fingerprint names a file
/// this test owns.
fn server_exe(host: &crate::r03_tests::Fixture) -> std::path::PathBuf {
    let debug = std::env::current_exe()
        .expect("test image")
        .parent()
        .expect("deps")
        .parent()
        .expect("debug")
        .to_owned();
    let exe = host.root.path().join("mcp-servers").join("fixture-server.exe");
    std::fs::create_dir_all(exe.parent().expect("folder")).expect("server folder");
    std::fs::copy(debug.join("aura-mcp-fixture.exe"), &exe)
        .expect("build current aura-tool-fixtures --bins first");
    exe
}

async fn revision(runtime: &crate::RuntimeState) -> u64 {
    crate::r03_tests::success(runtime, "agent.mcp.list", json!({})).await["revision"]
        .as_u64()
        .expect("registry revision")
}

pub async fn mcp_view(runtime: &crate::RuntimeState, id: &str) -> serde_json::Value {
    crate::r03_tests::success(runtime, "agent.mcp.list", json!({})).await["servers"]
        .as_array()
        .expect("servers")
        .iter()
        .find(|server| server["config"]["id"] == id)
        .cloned()
        .expect("listed")
}

/// A stdio server `id` running the test server with `arguments`.
pub fn mcp_server_config(exe: &std::path::Path, id: &str, arguments: &[&str]) -> serde_json::Value {
    json!({
        "id": id,
        "display_name": format!("测试 {id}"),
        "transport": {"type": "stdio", "command": exe.display().to_string(), "arguments": arguments},
        "startup_timeout_ms": 10_000,
        "call_timeout_ms": 10_000,
    })
}

/// Saves `server`, trusts exactly the shown fingerprint and waits until discovery settles.
pub async fn trust_mcp_server(
    runtime: &crate::RuntimeState,
    server: serde_json::Value,
) -> serde_json::Value {
    let id = server["id"].as_str().expect("server id").to_owned();
    let id = id.as_str();
    let expected_revision = revision(runtime).await;
    crate::r03_tests::success(
        runtime,
        "agent.mcp.save",
        json!({"server": server, "replaces": null, "expected_revision": expected_revision}),
    )
    .await;
    let fingerprint = mcp_view(runtime, id).await["fingerprint"].clone();
    let expected_revision = revision(runtime).await;
    crate::r03_tests::success(
        runtime,
        "agent.mcp.trust",
        json!({"id": id, "expected_fingerprint": fingerprint, "expected_revision": expected_revision}),
    )
    .await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let view = mcp_view(runtime, id).await;
        if view["state"] != "starting" || std::time::Instant::now() >= deadline {
            assert_eq!(view["state"], "ready", "the test server lists its tools: {view}");
            return view;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

impl Fixture {
    /// Starts Core, trusts one test server per `(id, arguments)` and opens a session whose
    /// registry has their tools loaded, as after `tools.load`.
    pub async fn with_mcp(mode: PermissionMode, servers: &[(&str, &[&str])]) -> Self {
        Self::with_mcp_configs(mode, |exe| {
            servers.iter().map(|(id, arguments)| mcp_server_config(exe, id, arguments)).collect()
        })
        .await
    }

    /// [`Fixture::with_mcp`] with the server configurations `configure` builds around the
    /// copied test server.
    pub async fn with_mcp_configs(
        mode: PermissionMode,
        configure: impl FnOnce(&std::path::Path) -> Vec<serde_json::Value>,
    ) -> Self {
        let host = Arc::new(crate::r03_tests::Fixture::new());
        let runtime = Arc::new(host.start().await);
        let exe = server_exe(&host);
        for server in configure(&exe) {
            trust_mcp_server(&runtime, server).await;
        }
        let root = host.root.path().join("agent-workspace");
        std::fs::create_dir(&root).expect("Agent test setup and persisted assertion");
        let value = crate::r03_tests::success(
            &runtime,
            "workspace.create",
            json!({"selected_root":root,"display_name":"Agent"}),
        )
        .await;
        let workspace: aura_workspace_domain::WorkspaceSummary =
            serde_json::from_value(value["workspace"].clone())
                .expect("Agent test setup and persisted assertion");
        Self::create_in_workspace(host, runtime, root, workspace, mode, &[], Packages::Loaded)
    }
}
