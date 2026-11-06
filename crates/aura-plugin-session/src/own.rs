//! A plugin's own prompt commands, run by its panel or its background service: found in the
//! composer's catalog among the commands the plugin brings, as the composer offers them now.
use aura_agent_domain::{CommandEntry, CommandKind, CommandSource, valid_command_name};

/// The template of `plugin`'s own prompt command `name`. A name the plugin brings no command of
/// is unknown, even when another source has one; one it brings that is switched off or cannot be
/// offered now is refused with why.
pub fn own_prompt(catalog: &[CommandEntry], plugin: &str, name: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        valid_command_name(name),
        "plugin_session.command_unknown: 命令名只能用小写字母、数字和 -，以字母开头"
    );
    let own = catalog.iter().find(|entry| {
        entry.source == CommandSource::Plugin
            && entry.plugin.as_deref() == Some(plugin)
            && entry.name == name
    });
    let Some(entry) = own else {
        anyhow::bail!("plugin_session.command_unknown: 插件 {plugin} 没有命令 /{name}");
    };
    if let Some(issue) = &entry.issue {
        anyhow::bail!("plugin_session.command_unusable: /{name} 现在不能用：{issue}");
    }
    anyhow::ensure!(entry.enabled, "plugin_session.command_unusable: /{name} 已关掉，打开后才能用");
    match (entry.kind, &entry.template) {
        (CommandKind::Prompt, Some(template)) => Ok(template.clone()),
        _ => anyhow::bail!("plugin_session.command_unusable: /{name} 不是提示命令"),
    }
}
