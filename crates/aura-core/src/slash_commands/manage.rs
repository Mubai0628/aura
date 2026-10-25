//! Settings actions: turning a user or workspace command on or off, and removing one of the
//! user's own commands at the revision the user saw.
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
        anyhow::ensure!(
            request.source != CommandSource::Plugin,
            "commands.from_plugin: 插件的命令随插件启用，请在 设置 → 插件 里停用插件"
        );
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
