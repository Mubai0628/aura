//! The command menu of each channel whose service reported that its platform shows one: what that
//! plugin's chats can use, from the composer's catalog. Each connected channel is sent its menu
//! once and then again whenever it changed, on the reads of the plugins.
use super::driver::PluginServices;
use crate::channels::menu;
use aura_extension_service::commands::command_catalog;
use aura_extension_service::plugins::ReadyPlugin;
use aura_plugin_host::ToService;

impl PluginServices {
    /// Sends each connected channel that shows a menu the one it should show now, when that is
    /// not the one it was sent last. It blocks: it reads the commands' folders.
    pub(super) fn send_menus(&self, ready: &[ReadyPlugin]) {
        let showing: Vec<(&ReadyPlugin, Vec<String>)> = ready
            .iter()
            .filter_map(|plugin| {
                let connected = self.bridge().channels(&plugin.package.manifest.name);
                let channels: Vec<String> = connected
                    .into_iter()
                    .filter(|channel| channel.command_menu)
                    .map(|channel| channel.id)
                    .collect();
                (!channels.is_empty()).then_some((plugin, channels))
            })
            .collect();
        if showing.is_empty() {
            return;
        }
        let Some(agent) = self.bridge().sessions().reading() else { return };
        let user_root = agent.settings.paths().config_root.join("commands");
        let disabled = agent.settings.disabled_commands();
        let catalog = command_catalog(&user_root, &disabled.keys, None, Some(&self.plugins));
        for (plugin, channels) in showing {
            let name = &plugin.package.manifest.name;
            let declared = &plugin.package.manifest.declared.capabilities;
            let commands = menu(&catalog.commands, declared, plugin.setting.remote.remote_approval);
            for channel in channels {
                if !self.bridge().menu_due(name, &channel, &commands) {
                    continue;
                }
                let sent =
                    ToService::Commands { channel: channel.clone(), commands: commands.clone() };
                if self.host().deliver(name, &sent).is_err() {
                    self.bridge().menu_lost(name, &channel);
                }
            }
        }
    }
}
