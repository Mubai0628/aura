//! The Slash command catalog the composer and a plugin's chats share: the built-in commands, then
//! the user's and a workspace's prompt commands, then the ready plugins' at the revision each was
//! trusted with. Only the composer, settings and a chat's own commands read it; the model reaches
//! workspace commands only through its own approved tools and never Aura's configuration.
use crate::plugins::PluginRuntime;
use aura_agent_domain::{CommandCatalog, CommandEntry, CommandSource, builtin_commands};
use aura_commands::{LoadedCommand, WORKSPACE_COMMANDS_DIRECTORY};
use aura_domain_common::WorkspaceId;
use std::collections::BTreeSet;
use std::path::Path;

/// The catalog: the user's commands under `user_root`, on or off as the keys in `disabled` say;
/// the commands of the workspace at the verified root given; and the ready plugins' commands.
/// The files are read here, so a caller on the reactor runs it off the reactor.
#[must_use]
pub fn command_catalog(
    user_root: &Path,
    disabled: &BTreeSet<String>,
    workspace: Option<(WorkspaceId, &Path)>,
    plugins: Option<&PluginRuntime>,
) -> CommandCatalog {
    let user = aura_commands::scan(user_root, CommandSource::User);
    let workspace = workspace.map(|(id, root)| {
        let commands = aura_commands::scan_within(
            root,
            WORKSPACE_COMMANDS_DIRECTORY,
            CommandSource::Workspace,
        );
        (id, root.join(".aura").join("commands"), commands)
    });
    let mut commands = aura_commands::merge(
        builtin_commands(),
        &user,
        workspace.as_ref().map(|(id, _, commands)| (*id, commands.as_slice())),
        disabled,
    );
    if let Some(plugins) = plugins {
        aura_commands::add_plugin_commands(&mut commands, &plugin_commands(plugins));
        switch_off_plugin_commands(&mut commands, plugins);
    }
    CommandCatalog {
        commands,
        user_directory: aura_domain_common::display_path(user_root),
        workspace_directory: workspace
            .map(|(_, directory, _)| aura_domain_common::display_path(&directory)),
    }
}

/// The plugin commands the user switched off stay listed, off.
fn switch_off_plugin_commands(commands: &mut [CommandEntry], plugins: &PluginRuntime) {
    let ready = plugins.ready();
    for entry in commands.iter_mut().filter(|entry| entry.source == CommandSource::Plugin) {
        let switched_off = ready.iter().any(|plugin| {
            entry.plugin.as_ref() == Some(&plugin.package.manifest.name)
                && plugin.setting.changes.switched_off.commands.contains(&entry.name)
        });
        if switched_off {
            entry.enabled = false;
        }
    }
}

/// Each ready plugin's commands, offered only at the revision they had when the plugin
/// was trusted.
fn plugin_commands(plugins: &PluginRuntime) -> Vec<(String, Vec<LoadedCommand>)> {
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
