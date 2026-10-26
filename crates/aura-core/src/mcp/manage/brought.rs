//! A plugin's MCP server in Settings → MCP: switched and configured like the user's own, with
//! what the user changed kept in the plugin's `plugins.json` entry. It is removed only with the
//! plugin, and its trust is the plugin's until a changed configuration is trusted on its own.
use super::McpContext;
use aura_mcp::{McpRegistryView, McpSaveRequest, McpServerConfig};
use std::collections::BTreeSet;
use std::sync::Arc;

impl McpContext {
    fn plugins(&self) -> anyhow::Result<Arc<crate::plugins::PluginRuntime>> {
        self.runtime
            .plugin_runtime()
            .ok_or_else(|| anyhow::anyhow!("mcp.from_plugin: 插件暂时不可用"))
    }

    pub(super) fn switch_brought(
        &self,
        plugin: &str,
        id: &str,
        enabled: bool,
    ) -> anyhow::Result<McpRegistryView> {
        self.plugins()?.switch_part(plugin, crate::plugins::PartKind::Server, id, enabled)?;
        let server = self.server(id)?;
        if server.enabled && server.trusted_fingerprint.is_some() {
            if self.runtime.catalog(id).is_none() {
                self.runtime.discover_later(server);
            }
        } else {
            self.runtime.stop_server(id);
        }
        Ok(self.view(false))
    }

    /// Saves the user's configuration in place of the plugin's. A configuration that runs
    /// something else no longer matches the trust and is trusted on its own.
    pub(super) fn save_brought(
        &self,
        plugin: &str,
        request: McpSaveRequest,
    ) -> anyhow::Result<McpRegistryView> {
        anyhow::ensure!(
            request.replaces.as_deref() == Some(request.server.id.as_str()),
            "mcp.id_fixed: 服务器 id 决定工具名，不能修改；请新建一个服务器"
        );
        let before = self.server(&request.server.id)?;
        self.plugins()?.change_server(plugin, request.server)?;
        self.settle_brought(plugin, &before)
    }

    /// Puts the plugin's own configuration back.
    pub(super) fn restore_brought(
        &self,
        plugin: &str,
        id: &str,
    ) -> anyhow::Result<McpRegistryView> {
        let before = self.server(id)?;
        self.plugins()?.restore_server(plugin, id)?;
        self.settle_brought(plugin, &before)
    }

    /// After the configuration changed: instances started from the old one stop, the secrets
    /// only the old one named go (never the plugin's own), and a sign-in made for another
    /// connection is ended.
    fn settle_brought(
        &self,
        plugin: &str,
        before: &McpServerConfig,
    ) -> anyhow::Result<McpRegistryView> {
        let after = self.server(&before.id)?;
        self.runtime.stop_server(&after.id);
        self.runtime.revise(&after);
        let own: BTreeSet<_> = self
            .runtime
            .settings()
            .plugin_registry()
            .plugin(plugin)
            .map(|setting| setting.secrets.iter().map(|secret| secret.secret_id).collect())
            .unwrap_or_default();
        let kept: BTreeSet<_> = after.transport.secret_ids().into_iter().collect();
        for id in before.transport.secret_ids() {
            if kept.contains(&id) || own.contains(&id) {
                continue;
            }
            if let Err(reason) = self.runtime.secrets().forget(id) {
                crate::append_core_log(
                    "mcp.secret_cleanup_failed",
                    serde_json::json!({"secret_id": id, "reason": reason}),
                );
            }
        }
        if before.transport != after.transport {
            self.runtime.sign_out(&after.id);
        }
        Ok(self.view(false))
    }
}
