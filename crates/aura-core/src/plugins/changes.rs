//! The user's changes to what a plugin brings: a part switched off, or one of its MCP servers saved
//! with a configuration of the user's own in place of the plugin's. They live in the plugin's
//! `plugins.json` entry, stay across its updates for the parts the new version still has, and go
//! with the plugin; a part is never removed on its own.
use super::lifecycle::replaced;
use super::runtime::PluginRuntime;
use aura_mcp::McpServerConfig;
use aura_plugins::{PluginPackage, PluginParts, PluginSetting};
use std::collections::{BTreeMap, BTreeSet};

/// The kind of part a switch names, by the name `trusted_parts` uses for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PartKind {
    /// A Skill folder, relative to the plugin's `skills`.
    Skill,
    /// A command name.
    Command,
    /// A Hook folder under `hooks`.
    Hook,
    /// An MCP server ID.
    Server,
}

fn of_kind(parts: &mut PluginParts, kind: PartKind) -> &mut BTreeSet<String> {
    match kind {
        PartKind::Skill => &mut parts.skills,
        PartKind::Command => &mut parts.commands,
        PartKind::Hook => &mut parts.hooks,
        PartKind::Server => &mut parts.servers,
    }
}

impl PluginRuntime {
    /// Switches one part the plugin brings on or off.
    pub(crate) fn switch_part(
        &self,
        plugin: &str,
        kind: PartKind,
        part: &str,
        on: bool,
    ) -> anyhow::Result<()> {
        self.change_setting(plugin, |setting| {
            let off = of_kind(&mut setting.changes.switched_off, kind);
            if on {
                off.remove(part);
            } else {
                off.insert(part.to_owned());
            }
        })
    }

    /// Saves the user's configuration for one of the plugin's servers. Its trust and its switch
    /// stay the plugin's (`trusted_parts`, `switched_off`).
    pub(crate) fn change_server(
        &self,
        plugin: &str,
        mut config: McpServerConfig,
    ) -> anyhow::Result<()> {
        config.trusted_fingerprint = None;
        config.enabled = true;
        self.change_setting(plugin, |setting| {
            setting.changes.servers.insert(config.id.clone(), config);
        })
    }

    /// Puts the plugin's own configuration back for one of its servers; returns the one the user
    /// had saved.
    pub(crate) fn restore_server(
        &self,
        plugin: &str,
        server: &str,
    ) -> anyhow::Result<Option<McpServerConfig>> {
        let mut restored = None;
        self.change_setting(plugin, |setting| {
            restored = setting.changes.servers.remove(server);
        })?;
        Ok(restored)
    }

    fn change_setting(
        &self,
        plugin: &str,
        change: impl FnOnce(&mut PluginSetting),
    ) -> anyhow::Result<()> {
        let _changing = self.changing.lock().map_err(|_| anyhow::anyhow!("plugins.busy"))?;
        let registry = self.settings().plugin_registry();
        anyhow::ensure!(
            registry.issue.is_none(),
            "plugins.registry_unavailable: {}",
            registry.issue.clone().unwrap_or_default()
        );
        let mut setting = registry
            .plugin(plugin)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("plugins.not_found: 找不到插件 {plugin}，请刷新"))?;
        change(&mut setting);
        self.write_registry(registry.revision, &replaced(&registry, setting))?;
        self.changed();
        Ok(())
    }

    /// After the plugin was trusted or updated: the changes to parts the package no longer has go,
    /// and a server the user configured keeps the trust given to that configuration only while it
    /// still resolves to what was trusted; otherwise it is trusted on its own again.
    pub(crate) fn settle_changes(
        &self,
        package: &PluginPackage,
        setting: &mut PluginSetting,
        trusted_before: &BTreeMap<String, String>,
    ) {
        let skills: BTreeSet<_> = package.skills.iter().map(|found| &found.relative).collect();
        let commands: BTreeSet<_> =
            package.commands.iter().map(|offered| &offered.command.name).collect();
        let hooks: BTreeSet<_> = package.hooks.iter().map(|hook| &hook.folder).collect();
        let servers: BTreeSet<_> = package.servers.iter().map(|server| &server.id).collect();
        let changes = &mut setting.changes;
        changes.switched_off.skills.retain(|part| skills.contains(part));
        changes.switched_off.commands.retain(|part| commands.contains(part));
        changes.switched_off.hooks.retain(|part| hooks.contains(part));
        changes.switched_off.servers.retain(|part| servers.contains(part));
        changes.servers.retain(|id, _| servers.contains(id));
        for (id, config) in &changes.servers {
            let trusted = trusted_before.get(id).filter(|trusted| {
                self.mcp.as_ref().and_then(|mcp| mcp.current_fingerprint(config).ok()).as_ref()
                    == Some(*trusted)
            });
            match trusted {
                Some(trusted) => {
                    setting.trusted_parts.servers.insert(id.clone(), trusted.clone());
                }
                None => {
                    setting.trusted_parts.servers.remove(id);
                }
            }
        }
    }
}

/// The servers the user configured whose own configuration the waiting version changes or no
/// longer has, by display name.
pub(super) fn changed_servers(
    installed: Option<&PluginPackage>,
    waiting: &PluginPackage,
    setting: Option<&PluginSetting>,
) -> Vec<String> {
    let Some(setting) = setting else { return Vec::new() };
    let own = |package: &PluginPackage, id: &str| {
        package
            .servers
            .iter()
            .find(|server| server.id == id)
            .map(|server| server.config(&|key| setting.secret(id, key).unwrap_or_default()))
    };
    setting
        .changes
        .servers
        .iter()
        .filter_map(|(id, saved)| {
            let before = installed.and_then(|package| own(package, id));
            let after = own(waiting, id);
            (before != after).then(|| {
                after.or(before).map_or_else(|| saved.display_name.clone(), |c| c.display_name)
            })
        })
        .collect()
}
