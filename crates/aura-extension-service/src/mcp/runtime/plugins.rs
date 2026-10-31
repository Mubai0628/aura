//! MCP and plugins: the servers ready plugins bring, the limits their processes run
//! under, the credentials plugins keep, and what the plugin runtime asks of MCP when a plugin
//! is trusted, changes or goes.
use super::McpRuntime;
use aura_domain_common::CredentialId;
use aura_mcp::McpServerConfig;
use aura_windows_platform::JobLimits;
use std::collections::BTreeSet;
use std::sync::{Arc, Weak};

/// A server a ready plugin brings, as MCP runs it.
pub(crate) struct BroughtServer {
    pub config: McpServerConfig,
    pub plugin: String,
    pub limits: JobLimits,
    /// Run with the configuration the user saved in place of the plugin's.
    pub changed: bool,
}

impl McpRuntime {
    pub fn attach_plugins(&self, plugins: &Arc<crate::plugins::PluginRuntime>) {
        let _ = self.plugins.set(Arc::downgrade(plugins));
    }

    pub(crate) fn plugin_runtime(&self) -> Option<Arc<crate::plugins::PluginRuntime>> {
        self.plugins.get().and_then(Weak::upgrade)
    }

    /// The servers ready plugins bring, on and with the trust their plugin recorded for each. A
    /// server whose ID one of the user's servers (or an earlier plugin's) already has is left
    /// out; the plugin page says so.
    pub(crate) fn plugin_servers(&self, user: &[McpServerConfig]) -> Vec<BroughtServer> {
        let Some(plugins) = self.plugin_runtime() else { return Vec::new() };
        let mut taken: BTreeSet<String> = user.iter().map(|server| server.id.clone()).collect();
        let mut brought = Vec::new();
        for plugin in plugins.ready().iter() {
            let setting = &plugin.setting;
            for server in plugin.package.servers.iter().filter(|server| server.problems.is_empty())
            {
                if !taken.insert(server.id.clone()) {
                    continue;
                }
                let changed = setting.changes.servers.get(&server.id);
                let mut config = match changed {
                    Some(changed) => changed.clone(),
                    None => {
                        server.config(&|key| setting.secret(&server.id, key).unwrap_or_default())
                    }
                };
                config.enabled = !setting.changes.switched_off.servers.contains(&server.id);
                config.trusted_fingerprint = setting.trusted_parts.servers.get(&server.id).cloned();
                brought.push(BroughtServer {
                    config,
                    plugin: plugin.package.manifest.name.clone(),
                    limits: plugin.job_limits(),
                    changed: changed.is_some(),
                });
            }
        }
        brought
    }

    fn brought(&self, id: &str) -> Option<BroughtServer> {
        let user = self.settings.mcp_registry().servers;
        self.plugin_servers(&user).into_iter().find(|server| server.config.id == id)
    }

    /// The plugin that brings the server `id`, if a ready plugin does.
    pub(crate) fn plugin_of(&self, id: &str) -> Option<String> {
        self.brought(id).map(|server| server.plugin)
    }

    /// The plugin that brings the server `id` and whether the user saved its configuration.
    pub(crate) fn brought_by(&self, id: &str) -> Option<(String, bool)> {
        self.brought(id).map(|server| (server.plugin, server.changed))
    }

    pub(crate) fn plugin_limits(&self, id: &str) -> Option<JobLimits> {
        self.brought(id).map(|server| server.limits)
    }

    /// Every secret and every server ID `plugins.json` names, ready or not (the user's saved
    /// configurations included); `None` while it cannot be read (then nothing may be swept).
    pub(crate) fn plugin_credentials(&self) -> Option<(BTreeSet<CredentialId>, Vec<String>)> {
        let registry = self.settings.plugin_registry();
        if registry.issue.is_some() {
            return None;
        }
        let mut secrets = BTreeSet::new();
        let mut servers = BTreeSet::new();
        for plugin in &registry.plugins {
            for secret in &plugin.secrets {
                secrets.insert(secret.secret_id);
                servers.insert(secret.server.clone());
            }
            servers.extend(plugin.trusted_parts.servers.keys().cloned());
            for (id, changed) in &plugin.changes.servers {
                secrets.extend(changed.transport.secret_ids());
                servers.insert(id.clone());
            }
        }
        Some((secrets, servers.into_iter().collect()))
    }

    /// The configuration fingerprint `server` would be trusted at now (the program it resolves to
    /// and that program's content), or why it cannot be trusted.
    pub(crate) fn current_fingerprint(&self, server: &McpServerConfig) -> Result<String, String> {
        let checked = crate::mcp::launch::check(server, &self.hashes);
        if !checked.problems.is_empty() {
            return Err(checked.problems.join("；"));
        }
        checked.fingerprint.ok_or_else(|| "无法确定服务器会运行的内容".to_owned())
    }

    /// Forgets servers that are gone (their instances, tool catalog and sign-in) and the given
    /// secrets; returns what could not be removed.
    pub(crate) fn forget_servers(
        &self,
        servers: &[String],
        secrets: &[CredentialId],
    ) -> Vec<String> {
        let mut failures = Vec::new();
        for secret in secrets {
            if let Err(reason) = self.secrets.forget(*secret) {
                failures.push(reason);
            }
        }
        for server in servers {
            if let Err(reason) = self.forget(server) {
                failures.push(reason);
            }
            if let Err(reason) = self.oauth.sign_out(server) {
                failures.push(reason);
            }
        }
        failures
    }
}
