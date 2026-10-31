//! Settings → MCP: list, check, save, trust, untrust, switch, remove, test and describe, and put a
//! plugin server's own configuration back. Every change names the registry revision it was made
//! against; trust is given only at the fingerprint the user was shown, and never comes from a save.
mod brought;

use super::McpRuntime;
use aura_config::SettingsFacadeError;
use aura_mcp::{
    McpDescribeRequest, McpListRequest, McpPreview, McpPreviewRequest, McpRefreshRequest,
    McpRegistryView, McpSaveRequest, McpServerConfig, McpServerDetail, McpServerRequest,
    McpServerState, McpSetEnabledRequest, McpSignInBegun, McpSignInRequest, McpSignOutRequest,
    McpToolView, McpTrustRequest,
};
use std::sync::Arc;

pub struct McpContext {
    runtime: Arc<McpRuntime>,
}

fn find<'a>(
    servers: &'a mut [McpServerConfig],
    id: &str,
) -> anyhow::Result<&'a mut McpServerConfig> {
    servers
        .iter_mut()
        .find(|server| server.id == id)
        .ok_or_else(|| anyhow::anyhow!("mcp.not_found: 这个服务器不在列表里，请刷新"))
}

impl McpContext {
    pub fn new(runtime: Arc<McpRuntime>) -> Self {
        Self { runtime }
    }

    /// Runs one `agent.mcp.*` Core route.
    pub fn dispatch(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(match method {
            "agent.mcp.list" => {
                let _: McpListRequest = serde_json::from_value(params)?;
                serde_json::to_value(self.view(true))?
            }
            "agent.mcp.preview" => {
                let request: McpPreviewRequest = serde_json::from_value(params)?;
                let checked = super::launch::check(&request.server, &self.runtime.hashes);
                serde_json::to_value(McpPreview {
                    fingerprint: checked.fingerprint,
                    resolved_command: checked.resolved,
                    problems: checked.problems,
                })?
            }
            "agent.mcp.save" => serde_json::to_value(self.save(serde_json::from_value(params)?)?)?,
            "agent.mcp.trust" => {
                serde_json::to_value(self.trust(serde_json::from_value(params)?)?)?
            }
            "agent.mcp.untrust" => {
                let request: McpServerRequest = serde_json::from_value(params)?;
                self.not_from_plugin(&request.id)?;
                let view = self.change(&request.id, request.expected_revision, |server| {
                    server.trusted_fingerprint = None;
                })?;
                // An untrusted server keeps no sign-in.
                self.runtime.sign_out(&request.id);
                serde_json::to_value(view)?
            }
            "agent.mcp.oauth.sign_in" => {
                let request: McpSignInRequest = serde_json::from_value(params)?;
                serde_json::to_value(self.sign_in(&self.server(&request.id)?)?)?
            }
            "agent.mcp.oauth.sign_out" => {
                let request: McpSignOutRequest = serde_json::from_value(params)?;
                let server = self.server(&request.id)?;
                self.runtime.sign_out(&server.id);
                serde_json::to_value(self.view(false))?
            }
            "agent.mcp.set_enabled" => {
                let request: McpSetEnabledRequest = serde_json::from_value(params)?;
                if let Some(plugin) = self.runtime.plugin_of(&request.id) {
                    let view = self.switch_brought(&plugin, &request.id, request.enabled)?;
                    return Ok(serde_json::to_value(view)?);
                }
                serde_json::to_value(self.change(
                    &request.id,
                    request.expected_revision,
                    |server| {
                        server.enabled = request.enabled;
                    },
                )?)?
            }
            "agent.mcp.remove" => {
                let request: McpServerRequest = serde_json::from_value(params)?;
                self.not_from_plugin(&request.id)?;
                let registry = self.registry()?;
                let mut servers = registry.servers;
                let removed = servers.iter().find(|server| server.id == request.id).cloned();
                servers.retain(|server| server.id != request.id);
                anyhow::ensure!(removed.is_some(), "mcp.not_found: 这个服务器不在列表里，请刷新");
                self.write(request.expected_revision, &servers)?;
                self.runtime.forget(&request.id).map_err(|error| anyhow::anyhow!(error))?;
                self.runtime.release_secrets(removed.as_ref(), None);
                self.runtime.sign_out(&request.id);
                serde_json::to_value(self.view(false))?
            }
            "agent.mcp.restore_plugin_default" => {
                let request: McpServerRequest = serde_json::from_value(params)?;
                let plugin = self.runtime.plugin_of(&request.id).ok_or_else(|| {
                    anyhow::anyhow!("mcp.not_from_plugin: 这个服务器不是插件带来的")
                })?;
                serde_json::to_value(self.restore_brought(&plugin, &request.id)?)?
            }
            "agent.mcp.refresh" => {
                let request: McpRefreshRequest = serde_json::from_value(params)?;
                let server = self.server(&request.id)?;
                // The result shows in the server's state and errors; a failure is not a
                // route failure.
                let _ = self.runtime.discover(&server);
                serde_json::to_value(self.detail(&server))?
            }
            "agent.mcp.describe" => {
                let request: McpDescribeRequest = serde_json::from_value(params)?;
                serde_json::to_value(self.detail(&self.server(&request.id)?))?
            }
            other => anyhow::bail!("mcp.unknown_route: {other}"),
        })
    }

    /// The user's own servers, as `mcp.json` holds them: the only ones a change writes.
    fn registry(&self) -> anyhow::Result<aura_config::McpRegistry> {
        let registry = self.runtime.settings().mcp_registry();
        anyhow::ensure!(
            registry.issue.is_none(),
            "mcp.registry_unavailable: {}",
            registry.issue.clone().unwrap_or_default()
        );
        Ok(registry)
    }

    /// Any server, the user's or one a ready plugin brings.
    fn server(&self, id: &str) -> anyhow::Result<McpServerConfig> {
        let mut servers = self.runtime.configured().servers;
        Ok(find(&mut servers, id)?.clone())
    }

    /// A plugin's server is trusted and removed with its plugin.
    fn not_from_plugin(&self, id: &str) -> anyhow::Result<()> {
        match self.runtime.plugin_of(id) {
            Some(plugin) => anyhow::bail!(
                "mcp.from_plugin: 这个服务器来自插件 {plugin}，它的信任和移除都随插件"
            ),
            None => Ok(()),
        }
    }

    /// Every server with its state. Listing also starts discovery for a trusted, enabled
    /// server Aura has not listed yet (for example after its configuration was restored).
    fn view(&self, discover_missing: bool) -> McpRegistryView {
        let registry = self.runtime.configured();
        let servers = registry
            .servers
            .iter()
            .map(|server| {
                let view = self.runtime.view(server);
                if discover_missing
                    && view.state == McpServerState::Starting
                    && view.running_instances == 0
                    && view.tool_count.is_none()
                {
                    self.runtime.discover_later(server.clone());
                }
                view
            })
            .collect();
        McpRegistryView { revision: registry.revision, servers }
    }

    fn detail(&self, server: &McpServerConfig) -> McpServerDetail {
        let view = self.runtime.view(server);
        let catalog = self.runtime.catalog(&server.id).filter(|catalog| {
            view.fingerprint.as_deref() == Some(catalog.config_fingerprint.as_str())
        });
        McpServerDetail {
            tools: catalog
                .as_ref()
                .map(|catalog| {
                    catalog
                        .catalog
                        .tools
                        .iter()
                        .map(|tool| McpToolView {
                            canonical_name: tool.canonical_name.clone(),
                            wire_name: tool.wire_name.clone(),
                            title: tool.title.clone(),
                            description: tool.description.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            skipped_tools: catalog.map(|catalog| catalog.catalog.skipped).unwrap_or_default(),
            stderr: self.runtime.stderr(&server.id),
            server: view,
        }
    }

    fn save(&self, request: McpSaveRequest) -> anyhow::Result<McpRegistryView> {
        let problems = aura_mcp::validate_server(&request.server);
        anyhow::ensure!(problems.is_empty(), "mcp.invalid_server: {}", problems.join("；"));
        let plugin = self.runtime.plugin_of(&request.server.id);
        // A plugin's server may also name the secrets its plugin keeps for it.
        let own: Vec<_> = plugin
            .as_ref()
            .and_then(|plugin| self.runtime.settings().plugin_registry().plugin(plugin).cloned())
            .map(|setting| {
                setting
                    .secrets
                    .iter()
                    .filter(|secret| secret.server == request.server.id)
                    .map(|secret| secret.secret_id)
                    .collect()
            })
            .unwrap_or_default();
        for secret in request.server.transport.secret_ids() {
            anyhow::ensure!(
                own.contains(&secret)
                    || self.runtime.secrets().belongs_to(&request.server.id, secret),
                "mcp.secret_unknown: 密钥不存在或不属于这个服务器，请重新填写密钥"
            );
        }
        if let Some(plugin) = plugin {
            anyhow::ensure!(
                request.replaces.is_some(),
                "mcp.duplicate_id: 已有 id 为 {} 的服务器（来自插件 {plugin}）",
                request.server.id
            );
            return self.save_brought(&plugin, request);
        }
        if let Some(id) = &request.replaces {
            self.not_from_plugin(id)?;
        }
        let registry = self.registry()?;
        let mut servers = registry.servers;
        let replaced = request
            .replaces
            .as_ref()
            .and_then(|id| servers.iter().find(|server| server.id == *id).cloned());
        let mut saved = request.server;
        match &request.replaces {
            None => {
                anyhow::ensure!(
                    servers.iter().all(|server| server.id != saved.id),
                    "mcp.duplicate_id: 已有 id 为 {} 的服务器",
                    saved.id
                );
                saved.trusted_fingerprint = None;
                saved.enabled = false;
                servers.push(saved.clone());
            }
            Some(id) => {
                anyhow::ensure!(
                    *id == saved.id,
                    "mcp.id_fixed: 服务器 id 决定工具名，不能修改；请新建一个服务器"
                );
                let existing = find(&mut servers, id)?;
                saved.trusted_fingerprint = existing.trusted_fingerprint.clone();
                saved.enabled = existing.enabled;
                *existing = saved.clone();
                // A changed configuration no longer matches its trust: running instances
                // were started from the old one.
                self.runtime.stop_server(id);
            }
        }
        self.write(request.expected_revision, &servers)?;
        if request.replaces.is_some() {
            self.runtime.revise(&saved);
        }
        self.runtime.release_secrets(replaced.as_ref(), Some(&saved));
        // A sign-in belongs to the configuration it was made for.
        if replaced.as_ref().is_some_and(|before| before.transport != saved.transport) {
            self.runtime.sign_out(&saved.id);
        }
        Ok(self.view(false))
    }

    fn trust(&self, request: McpTrustRequest) -> anyhow::Result<McpRegistryView> {
        if let Some(plugin) = self.runtime.plugin_of(&request.id) {
            return self.trust_brought(&plugin, &request);
        }
        let registry = self.registry()?;
        let mut servers = registry.servers;
        let server = find(&mut servers, &request.id)?;
        let checked = super::launch::check(server, &self.runtime.hashes);
        anyhow::ensure!(
            checked.problems.is_empty(),
            "mcp.invalid_server: {}",
            checked.problems.join("；")
        );
        anyhow::ensure!(
            checked.fingerprint.as_deref() == Some(request.expected_fingerprint.as_str()),
            "mcp.config_changed: 服务器的配置或程序在你查看之后变了，请重新查看再信任"
        );
        server.trusted_fingerprint = checked.fingerprint;
        server.enabled = true;
        let trusted = server.clone();
        self.write(request.expected_revision, &servers)?;
        self.runtime.stop_server(&trusted.id);
        self.runtime.discover_later(trusted);
        Ok(self.view(false))
    }

    /// Trusts a plugin's server again at its current configuration (after, say, the program it
    /// runs was updated); the plugin keeps the trust.
    fn trust_brought(
        &self,
        plugin: &str,
        request: &McpTrustRequest,
    ) -> anyhow::Result<McpRegistryView> {
        let server = self.server(&request.id)?;
        let fingerprint = self
            .runtime
            .current_fingerprint(&server)
            .map_err(|reason| anyhow::anyhow!("mcp.invalid_server: {reason}"))?;
        anyhow::ensure!(
            fingerprint == request.expected_fingerprint,
            "mcp.config_changed: 服务器的配置或程序在你查看之后变了，请重新查看再信任"
        );
        let plugins = self
            .runtime
            .plugin_runtime()
            .ok_or_else(|| anyhow::anyhow!("mcp.from_plugin: 插件暂时不可用"))?;
        plugins.trust_server(plugin, &request.id, fingerprint)?;
        self.runtime.stop_server(&request.id);
        if let Ok(trusted) = self.server(&request.id) {
            self.runtime.discover_later(trusted);
        }
        Ok(self.view(false))
    }

    /// Starts an OAuth sign-in for a trusted, enabled and unchanged server; the tools are
    /// listed again once the browser's return completed it.
    fn sign_in(&self, server: &McpServerConfig) -> anyhow::Result<McpSignInBegun> {
        let fingerprint = self
            .runtime
            .ready_for_sign_in(server)
            .map_err(|reason| anyhow::anyhow!("mcp.not_ready: {reason}"))?;
        let (runtime, later) = (Arc::clone(&self.runtime), server.clone());
        let begun = self
            .runtime
            .oauth()
            .begin(server, &fingerprint, move || runtime.discover_later(later))
            .map_err(|reason| anyhow::anyhow!("mcp.sign_in_failed: {reason}"))?;
        Ok(McpSignInBegun {
            authorization_url: begun.authorization_url,
            authorization_server: begun.authorization_server,
        })
    }

    fn change(
        &self,
        id: &str,
        expected_revision: u64,
        edit: impl FnOnce(&mut McpServerConfig),
    ) -> anyhow::Result<McpRegistryView> {
        let registry = self.registry()?;
        let mut servers = registry.servers;
        let server = find(&mut servers, id)?;
        edit(server);
        let changed = server.clone();
        self.write(expected_revision, &servers)?;
        if changed.enabled && changed.trusted_fingerprint.is_some() {
            if self.runtime.catalog(id).is_none() {
                self.runtime.discover_later(changed);
            }
        } else {
            self.runtime.stop_server(id);
        }
        Ok(self.view(false))
    }

    fn write(&self, expected_revision: u64, servers: &[McpServerConfig]) -> anyhow::Result<()> {
        match self.runtime.settings().replace_mcp_registry(expected_revision, servers) {
            Ok(_) => Ok(()),
            Err(SettingsFacadeError::RevisionConflict { .. }) => {
                anyhow::bail!("mcp.revision_changed: 服务器列表已被修改，请刷新后再操作")
            }
            Err(error) => Err(error.into()),
        }
    }
}
