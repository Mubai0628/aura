//! Sessions whose registry holds the real installed package tools.
use super::*;

/// How a session's registry offers the installed package tools and the tools of
/// trusted MCP servers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Packages {
    None,
    /// Candidates only: found by `tools.search`, loaded by `tools.load`.
    Discoverable,
    /// Candidates loaded for the session, as after `tools.load`.
    Loaded,
}

impl Fixture {
    /// Starts Core, runs `install` against its real package service, then opens a
    /// session whose registry has every installed package tool loaded.
    pub async fn with_packages<T>(
        mode: PermissionMode,
        names: &[&str],
        install: impl FnOnce(&crate::RuntimeServices) -> T,
    ) -> (Self, T) {
        Self::with_package_exposure(mode, names, Packages::Loaded, install).await
    }

    pub async fn with_package_exposure<T>(
        mode: PermissionMode,
        names: &[&str],
        exposure: Packages,
        install: impl FnOnce(&crate::RuntimeServices) -> T,
    ) -> (Self, T) {
        let host = Arc::new(crate::r03_tests::Fixture::new());
        let runtime = Arc::new(host.start().await);
        let installed = install(services(&runtime));
        let root = host.root.path().join("agent-workspace");
        std::fs::create_dir(&root).expect("Agent test setup and persisted assertion");
        let value = crate::r03_tests::success(
            &runtime,
            "workspace.create",
            serde_json::json!({"selected_root":root,"display_name":"Agent"}),
        )
        .await;
        let workspace: aura_workspace_domain::WorkspaceSummary =
            serde_json::from_value(value["workspace"].clone())
                .expect("Agent test setup and persisted assertion");
        (
            Self::create_in_workspace(host, runtime, root, workspace, mode, names, exposure),
            installed,
        )
    }

    /// Another session in this Core whose registry loads the packages installed now.
    pub fn package_session(&self, mode: PermissionMode, names: &[&str]) -> Self {
        let workspace = self
            .service()
            .persistence
            .workspace(self.turn.workspace_id)
            .expect("the session shares the existing real workspace");
        Self::create_in_workspace(
            Arc::clone(&self._host),
            Arc::clone(&self.runtime),
            self.root.clone(),
            workspace,
            mode,
            names,
            Packages::Loaded,
        )
    }

    /// A built-in tool's id, or a package tool's id from this step's registry.
    pub fn tool_id(&self, name: &str) -> aura_domain_common::ToolId {
        if let Some(entry) = aura_tool_registry::agent_release::release()
            .expect("Agent test setup and persisted assertion")
            .entry(name)
        {
            return entry.definition.tool_id;
        }
        self.registry()
            .catalog_entries
            .iter()
            .find(|entry| entry.canonical_name.as_str() == name)
            .unwrap_or_else(|| panic!("{name} is neither built in nor in this registry"))
            .tool_id
    }
}
