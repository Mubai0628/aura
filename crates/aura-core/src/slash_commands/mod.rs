//! The Slash command catalog: the built-in commands, then the user's and the selected
//! workspace's prompt commands, and the settings actions on them. Only the composer and
//! settings read the catalog; the model reaches workspace commands only through its own
//! approved tools and never Aura's configuration.
mod manage;
#[cfg(test)]
mod tests;
use aura_agent_domain::{CommandCatalog, CommandCatalogRequest, CommandSource, builtin_commands};
use aura_commands::WORKSPACE_COMMANDS_DIRECTORY;
use aura_domain_common::WorkspaceId;
use std::path::PathBuf;
use std::sync::Arc;

/// What the command routes need, owned so their file reads run off the reactor.
pub(crate) struct CommandContext {
    user_root: PathBuf,
    settings: Arc<aura_config::SettingsFacade>,
    workspace: Option<Arc<aura_workspace::WorkspaceRuntime>>,
    plugins: Option<Arc<crate::plugins::PluginRuntime>>,
}

impl CommandContext {
    pub(crate) fn of(services: &crate::RuntimeServices) -> Self {
        Self {
            user_root: services._settings.paths().config_root.join("commands"),
            settings: services._settings.clone(),
            workspace: services.p03().ok().map(|p03| p03.workspace.clone()),
            plugins: Some(services.plugins.clone()),
        }
    }

    /// Runs one `agent.commands.*` Core route.
    pub(crate) fn dispatch(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(match method {
            "agent.commands.catalog" => {
                let request: CommandCatalogRequest = serde_json::from_value(params)?;
                serde_json::to_value(self.catalog(request.workspace_id)?)?
            }
            "agent.commands.set_enabled" => {
                serde_json::to_value(self.set_enabled(serde_json::from_value(params)?)?)?
            }
            "agent.commands.delete" => {
                serde_json::to_value(self.delete(serde_json::from_value(params)?)?)?
            }
            _ => anyhow::bail!("commands.unknown_route"),
        })
    }

    /// The verified root of a registered workspace.
    pub(crate) fn workspace_root(&self, workspace_id: WorkspaceId) -> anyhow::Result<PathBuf> {
        let runtime =
            self.workspace.as_ref().ok_or_else(|| anyhow::anyhow!("workspace.unavailable"))?;
        let resolver = runtime.resolver(workspace_id)?;
        resolver.verify_root_identity()?;
        Ok(resolver.canonical_root().to_path_buf())
    }

    pub(crate) fn catalog(
        &self,
        workspace_id: Option<WorkspaceId>,
    ) -> anyhow::Result<CommandCatalog> {
        let user = aura_commands::scan(&self.user_root, CommandSource::User);
        let workspace = workspace_id
            .map(|id| -> anyhow::Result<_> {
                let root = self.workspace_root(id)?;
                let commands = aura_commands::scan_within(
                    &root,
                    WORKSPACE_COMMANDS_DIRECTORY,
                    CommandSource::Workspace,
                );
                Ok((id, root.join(".aura").join("commands"), commands))
            })
            .transpose()?;
        let disabled = self.settings.disabled_commands();
        let mut commands = aura_commands::merge(
            builtin_commands(),
            &user,
            workspace.as_ref().map(|(id, _, commands)| (*id, commands.as_slice())),
            &disabled.keys,
        );
        if let Some(plugins) = &self.plugins {
            aura_commands::add_plugin_commands(&mut commands, &plugin_commands(plugins));
        }
        Ok(CommandCatalog {
            commands,
            user_directory: aura_domain_common::display_path(&self.user_root),
            workspace_directory: workspace
                .map(|(_, directory, _)| aura_domain_common::display_path(&directory)),
        })
    }
}

/// Each ready plugin's commands, offered only at the revision they had when the plugin
/// was trusted.
fn plugin_commands(
    plugins: &crate::plugins::PluginRuntime,
) -> Vec<(String, Vec<aura_commands::LoadedCommand>)> {
    plugins
        .ready()
        .iter()
        .map(|plugin| {
            let trusted = &plugin.setting.trusted_parts.commands;
            let commands = plugin
                .package
                .commands
                .iter()
                .map(|offered| {
                    let mut command = offered.command.clone();
                    if command.issue.is_none()
                        && trusted.get(&command.name) != command.revision.as_ref()
                    {
                        command.issue = Some("内容在信任之后变了，请重新信任插件".into());
                    }
                    command
                })
                .collect();
            (plugin.package.manifest.name.clone(), commands)
        })
        .collect()
}
