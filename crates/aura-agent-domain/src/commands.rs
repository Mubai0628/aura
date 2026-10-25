//! Aura Slash commands. The model can never invoke one: the catalog is read
//! only by the user's composer and settings, and every command acts through the
//! user routes that already exist (stop, compaction, start, settings, panels).
use aura_domain_common::WorkspaceId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    /// Runs the moment it is chosen: opens a picker, a panel or a settings page.
    Immediate,
    /// Becomes a block in the composer; sending runs the command, and the text
    /// typed after it is the command's argument, never a separate message.
    Block,
    /// A user's prompt command: choosing it inserts its template as ordinary,
    /// editable message text. It carries no authority of its own.
    Prompt,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    Builtin,
    /// The user's own commands, in Aura's configuration directory.
    User,
    /// Commands in the selected workspace's `.aura/commands`.
    Workspace,
    /// Commands a trusted, enabled plugin brings; they follow the plugin.
    Plugin,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandEntry {
    /// Typed after `/`: lowercase letters, digits and `-`.
    pub name: String,
    pub title: String,
    pub description: String,
    pub kind: CommandKind,
    pub source: CommandSource,
    /// The plugin that brings it, for a plugin's command.
    #[serde(default)]
    pub plugin: Option<String>,
    /// What the text after the command means, when the command takes any.
    pub argument_hint: Option<String>,
    /// Prompt commands only. `$ARGUMENTS` marks where the typed text goes.
    pub template: Option<String>,
    /// False for a user command the user turned off. Built-ins are always enabled.
    pub enabled: bool,
    /// Why a user command cannot be offered (invalid, or shadowed by another).
    pub issue: Option<String>,
    /// Where a user command's files are, for the settings page.
    pub location: Option<String>,
    /// The version of a user command's files; removal or replacement names it.
    pub revision: Option<String>,
}

impl CommandEntry {
    /// Offered in the composer: enabled and without an issue.
    #[must_use]
    pub fn runnable(&self) -> bool {
        self.enabled && self.issue.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandCatalogRequest {
    /// The workspace whose own commands are included; built-ins and the user's
    /// commands are listed without one.
    pub workspace_id: Option<WorkspaceId>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandCatalog {
    pub commands: Vec<CommandEntry>,
    /// Where the user's own commands live (it may not exist yet).
    pub user_directory: String,
    /// The selected workspace's `.aura/commands` (it may not exist yet).
    pub workspace_directory: Option<String>,
}

/// Settings → 命令: turn one of the user's or the workspace's commands on or off.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandSetEnabledRequest {
    pub workspace_id: Option<WorkspaceId>,
    pub name: String,
    pub source: CommandSource,
    pub enabled: bool,
}

/// Settings → 命令: remove one of the user's own commands at the revision shown. Workspace
/// commands are project files and are not removed from settings.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandDeleteRequest {
    pub workspace_id: Option<WorkspaceId>,
    pub name: String,
    pub revision: String,
}

/// Open a command's folder, or the folder commands are added to, in the file manager.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandDirectoryRequest {
    pub workspace_id: Option<WorkspaceId>,
    pub source: CommandSource,
    /// None opens the folder commands of this source are added to.
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandDirectoryOpened {
    pub completed: bool,
    pub message: String,
}

/// The built-in commands, in menu order. `/plan` and `/goal` choose the task mode, in place of the
/// `+` menu; slash text not chosen from the menu is still sent as typed.
#[must_use]
pub fn builtin_commands() -> Vec<CommandEntry> {
    const BUILTINS: &[(&str, &str, &str, CommandKind, Option<&str>)] = &[
        ("plan", "Plan 模式", "先梳理方案，由你决定何时实施", CommandKind::Immediate, None),
        ("goal", "Goal 模式", "按目标与预算持续执行", CommandKind::Immediate, None),
        (
            "init",
            "初始化工作区",
            "建立或维护根目录的 AGENTS.md：固定五个章节，每条命令都有仓库依据或执行验证",
            CommandKind::Block,
            Some("补充说明（可选）"),
        ),
        ("model", "切换模型", "打开当前会话的模型选择", CommandKind::Immediate, None),
        ("permission", "切换权限", "打开当前会话的权限选择", CommandKind::Immediate, None),
        (
            "stop",
            "停止当前任务",
            "停止正在运行的任务，与“停止当前任务”按钮相同",
            CommandKind::Immediate,
            None,
        ),
        (
            "compact",
            "压缩上下文",
            "登记一次手动压缩，在任务的安全点执行；后面的文字作为压缩重点",
            CommandKind::Block,
            Some("压缩重点（可选）"),
        ),
        ("context", "上下文", "打开右侧的提示词检查器", CommandKind::Immediate, None),
        ("tools", "工具设置", "打开 设置 → 工具", CommandKind::Immediate, None),
        ("memory", "记忆设置", "打开 设置 → 记忆", CommandKind::Immediate, None),
        ("files", "文件", "打开右侧的文件面板", CommandKind::Immediate, None),
        ("diff", "审查改动", "打开右侧的审查面板", CommandKind::Immediate, None),
        ("terminal", "终端", "打开右侧的终端", CommandKind::Immediate, None),
        ("process", "进程", "打开右侧的进程面板", CommandKind::Immediate, None),
        ("help", "命令帮助", "列出全部命令与说明", CommandKind::Immediate, None),
    ];
    BUILTINS
        .iter()
        .map(|(name, title, description, kind, hint)| CommandEntry {
            name: (*name).into(),
            title: (*title).into(),
            description: (*description).into(),
            kind: *kind,
            source: CommandSource::Builtin,
            plugin: None,
            argument_hint: hint.map(Into::into),
            template: None,
            enabled: true,
            issue: None,
            location: None,
            revision: None,
        })
        .collect()
}

/// A command name as typed after `/`.
#[must_use]
pub fn valid_command_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid_unique_and_choose_the_task_modes() {
        let commands = builtin_commands();
        let mut names = commands.iter().map(|c| c.name.as_str()).collect::<Vec<_>>();
        assert!(names.iter().all(|name| valid_command_name(name)));
        // The task modes come first, chosen at once.
        assert_eq!(&names[..2], ["plan", "goal"]);
        assert!(commands[..2].iter().all(|c| c.kind == CommandKind::Immediate));
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), commands.len());
        for other in ["skills", "mcp", "browser"] {
            assert!(!names.contains(&other), "/{other} is not offered");
        }
        assert!(commands.iter().all(CommandEntry::runnable));
    }

    #[test]
    fn command_names_are_plain_lowercase_words() {
        for good in ["init", "review-pr", "a1"] {
            assert!(valid_command_name(good), "{good}");
        }
        for bad in ["", "Init", "1st", "re view", "a_b", "名字", &"x".repeat(41)] {
            assert!(!valid_command_name(bad), "{bad}");
        }
    }
}
