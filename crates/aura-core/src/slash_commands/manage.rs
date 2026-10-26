//! Settings actions: turning a user, workspace or plugin command on or off, and removing one of
//! the user's own commands at the revision the user saw. A plugin's command is removed only with
//! its plugin.
use super::CommandContext;
use aura_agent_domain::{
    CommandCatalog, CommandDeleteRequest, CommandSetEnabledRequest, CommandSource,
    valid_command_name,
};

impl CommandContext {
    pub(super) fn set_enabled(
        &self,
        request: CommandSetEnabledRequest,
    ) -> anyhow::Result<CommandCatalog> {
        anyhow::ensure!(
            request.source != CommandSource::Builtin,
            "commands.builtin_always_enabled: 内置命令不能停用"
        );
        if request.source == CommandSource::Plugin {
            let current = self.catalog(request.workspace_id)?;
            let plugin = current
                .commands
                .iter()
                .find(|command| command.source == request.source && command.name == request.name)
                .and_then(|command| command.plugin.clone())
                .ok_or_else(|| {
                    anyhow::anyhow!("commands.not_found: /{} 不存在，请刷新后再试", request.name)
                })?;
            let plugins = self
                .plugins
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("commands.from_plugin: 插件暂时不可用"))?;
            let command = crate::plugins::PartKind::Command;
            plugins.switch_part(&plugin, command, &request.name, request.enabled)?;
            return self.catalog(request.workspace_id);
        }
        anyhow::ensure!(
            request.source != CommandSource::Workspace || request.workspace_id.is_some(),
            "commands.workspace_required: 请先选择工作区"
        );
        let current = self.catalog(request.workspace_id)?;
        anyhow::ensure!(
            valid_command_name(&request.name)
                && current
                    .commands
                    .iter()
                    .any(|command| command.source == request.source && command.name == request.name),
            "commands.not_found: /{} 不存在，请刷新后再试",
            request.name
        );
        let key = aura_commands::disabled_key(request.source, request.workspace_id, &request.name);
        self.settings.set_command_enabled(&key, request.enabled)?;
        self.catalog(request.workspace_id)
    }

    pub(super) fn delete(&self, request: CommandDeleteRequest) -> anyhow::Result<CommandCatalog> {
        aura_commands::delete_user_command(&self.user_root, &request.name, &request.revision)?;
        // A removed command keeps no on/off state behind.
        let key = aura_commands::disabled_key(CommandSource::User, None, &request.name);
        if self.settings.disabled_commands().keys.contains(&key) {
            self.settings.set_command_enabled(&key, true)?;
        }
        self.catalog(request.workspace_id)
    }
}
