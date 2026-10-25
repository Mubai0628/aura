//! The menu and settings list: built-ins first, then the user's and the workspace's
//! commands, each with the reason it is not offered when it is not.
use super::definition::LoadedCommand;
use aura_agent_domain::{CommandEntry, CommandSource};
use aura_domain_common::WorkspaceId;
use std::collections::BTreeSet;

/// The settings key for a command's on/off state. A workspace command is keyed by its
/// workspace, so turning it off in one project does not touch another.
pub fn disabled_key(source: CommandSource, workspace: Option<WorkspaceId>, name: &str) -> String {
    match (source, workspace) {
        (CommandSource::Workspace, Some(workspace)) => format!("workspace/{workspace}/{name}"),
        (CommandSource::Workspace, None) => format!("workspace/-/{name}"),
        (CommandSource::User, _) => format!("user/{name}"),
        (CommandSource::Builtin, _) => format!("builtin/{name}"),
        (CommandSource::Plugin, _) => format!("plugin/{name}"),
    }
}

/// A ready plugin's commands come after every other command. A name another command
/// already offers stays with that command; between plugins, the plugin listed first keeps it.
pub fn add_plugin_commands(
    entries: &mut Vec<CommandEntry>,
    plugins: &[(String, Vec<LoadedCommand>)],
) {
    for (plugin, commands) in plugins {
        for command in commands {
            let holder = entries
                .iter()
                .find(|entry| entry.runnable() && entry.name == command.name)
                .map(|entry| entry.plugin.clone());
            let issue = holder.map(|holder| match holder {
                Some(other) => format!("与插件 {other} 的 /{} 重名，未启用", command.name),
                None => format!("与已有的 /{} 重名，未启用", command.name),
            });
            let mut entry = command.entry(true, issue);
            entry.plugin = Some(plugin.clone());
            entries.push(entry);
        }
    }
}

/// Built-ins always win a name. A valid, enabled workspace command takes the name from a
/// user command in that workspace; a broken or disabled one does not.
pub fn merge(
    builtins: Vec<CommandEntry>,
    user: &[LoadedCommand],
    workspace: Option<(WorkspaceId, &[LoadedCommand])>,
    disabled: &BTreeSet<String>,
) -> Vec<CommandEntry> {
    let reserved = builtins.iter().map(|c| c.name.clone()).collect::<BTreeSet<_>>();
    let taken = |name: &str| reserved.contains(name).then(|| "与内置命令重名，未启用".to_owned());
    let mut entries = builtins;
    let mut overriding = BTreeSet::new();
    let mut workspace_entries = Vec::new();
    if let Some((workspace_id, commands)) = workspace {
        for command in commands {
            let enabled = !disabled.contains(&disabled_key(
                CommandSource::Workspace,
                Some(workspace_id),
                &command.name,
            ));
            let entry = command.entry(enabled, taken(&command.name));
            if entry.runnable() {
                overriding.insert(command.name.clone());
            }
            workspace_entries.push(entry);
        }
    }
    for command in user {
        let enabled = !disabled.contains(&disabled_key(CommandSource::User, None, &command.name));
        let issue = taken(&command.name).or_else(|| {
            overriding.contains(&command.name).then(|| "被当前工作区的同名命令覆盖".to_owned())
        });
        entries.push(command.entry(enabled, issue));
    }
    entries.extend(workspace_entries);
    entries
}
