//! The Slash command routes: the catalog for the selected workspace, built in
//! `aura-extension-service`, and the settings actions on the user's and the workspace's commands.
mod manage;
#[cfg(test)]
mod tests;
use aura_agent_domain::{CommandCatalog, CommandCatalogRequest};
use aura_domain_common::WorkspaceId;
use std::path::PathBuf;
use std::sync::Arc;

/// What the command routes need, owned so their file reads run off the reactor.
pub(crate) struct CommandContext {
    user_root: PathBuf,
    settings: Arc<aura_config::SettingsFacade>,
    workspace: Option<Arc<aura_workspace::WorkspaceRuntime>>,
    plugins: Option<Arc<aura_extension_service::plugins::PluginRuntime>>,
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
        let workspace =
            workspace_id.map(|id| self.workspace_root(id).map(|root| (id, root))).transpose()?;
        Ok(aura_extension_service::commands::command_catalog(
            &self.user_root,
            &self.settings.disabled_commands().keys,
            workspace.as_ref().map(|(id, root)| (*id, root.as_path())),
            self.plugins.as_deref(),
        ))
    }
}
