//! What the settings page shows about one server: its configuration checked against this
//! computer, its trust, its sign-in and the last discovery.
use super::McpRuntime;
use super::launch;
use aura_mcp::{McpServerConfig, McpServerState, McpServerView};

impl McpRuntime {
    /// The settings page's view of one server, from its configuration, this computer and
    /// the last discovery.
    pub(crate) fn view(&self, server: &McpServerConfig) -> McpServerView {
        let mut checked = launch::check(server, &self.hashes);
        checked.problems.extend(self.missing_secret(server));
        let checked_fingerprint = checked.fingerprint.clone();
        let catalog = self.catalog(&server.id);
        let discovery = self
            .discoveries
            .lock()
            .ok()
            .and_then(|d| d.get(&server.id).map(|d| (d.running, d.last_error.clone())));
        let (running, last_error) = discovery.unwrap_or((false, None));
        let fresh = catalog.as_ref().filter(|catalog| {
            checked.fingerprint.as_deref() == Some(catalog.config_fingerprint.as_str())
        });
        let state = if !checked.problems.is_empty() {
            McpServerState::Invalid
        } else if server.trusted_fingerprint.is_none() {
            McpServerState::NotTrusted
        } else if server.trusted_fingerprint != checked.fingerprint {
            McpServerState::ConfigChanged
        } else if !server.enabled {
            McpServerState::Disabled
        } else if self.needs_sign_in(server, checked.fingerprint.as_deref()) {
            McpServerState::SignInRequired
        } else if running {
            McpServerState::Starting
        } else if fresh.is_some() && last_error.is_none() {
            McpServerState::Ready
        } else if last_error.is_some() {
            McpServerState::Failed
        } else {
            McpServerState::Starting
        };
        let (plugin, plugin_changed) = self
            .brought_by(&server.id)
            .map_or((None, false), |(plugin, changed)| (Some(plugin), changed));
        let last_error = super::plugin_hint::rewrite(plugin.as_deref(), server, last_error);
        McpServerView {
            config: server.clone(),
            plugin,
            plugin_changed,
            state,
            fingerprint: checked.fingerprint.clone(),
            resolved_command: checked.resolved,
            problems: checked.problems,
            server_name: fresh.map(|c| c.server.name.clone()),
            server_version: fresh.map(|c| c.server.version.clone()),
            protocol_version: fresh.map(|c| c.protocol_version.clone()),
            tool_count: fresh.map(|c| u32::try_from(c.catalog.tools.len()).unwrap_or(u32::MAX)),
            discovered_at: fresh.map(|c| c.discovered_at),
            last_error,
            running_instances: self.running_instances(&server.id),
            sign_in: self.sign_in_view(server, checked_fingerprint.as_deref()),
        }
    }
}
