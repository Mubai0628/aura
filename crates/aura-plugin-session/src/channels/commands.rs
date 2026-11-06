//! A command a chat's message starts with, read as the composer's own: only `/name` at the very
//! start, followed by white space or the end, and only a name the composer's catalog has or one
//! Aura has for chats; anything else is plain text for the conversation. Also the menu a platform
//! shows, what `/help` lists, and each command message counted once.
use super::approvals::RemoteDecision;
use aura_agent_domain::{CommandEntry, CommandKind, CommandSource, valid_command_name};
use aura_plugin_host::MenuCommand;
use aura_plugins::PluginCapability;
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write;
use std::time::{Duration, Instant};

/// The command messages remembered for one plugin's chats, and for how long, so a platform's
/// second push of one does not run it again.
pub const SEEN_COMMANDS_MAX: usize = 500;
pub const SEEN_COMMAND_TIME: Duration = Duration::from_secs(24 * 60 * 60);

/// `/name` and the text after it, as typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Typed<'a> {
    pub name: &'a str,
    pub arguments: &'a str,
}

/// One of Aura's own commands, as a chat uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remote {
    Plan,
    Goal,
    Init,
    Model,
    Permission,
    Stop,
    Compact,
    Help,
    Sessions,
    Switch,
    New,
    Status,
    /// A decision on the approval handed to the chat.
    Decide(RemoteDecision),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteCommand {
    Builtin(Remote),
    /// One of Aura's that needs what the plugin did not declare.
    Undeclared(PluginCapability),
    /// One of the composer's that opens a panel or a settings page on the computer.
    Desktop,
    /// A prompt command: its template becomes the message.
    Prompt {
        template: String,
    },
}

/// Where a command comes from: the composer's, or one only a chat has, of which the decisions on
/// approvals are listed only while the plugin's chats may decide them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Composer,
    Chat,
    Deciding,
}

/// What a command is in a chat: the composer's ones in the composer's order, then those only a
/// chat has. Each needs what its plugin must declare, beyond the `session.send` every chat has.
struct Spec {
    name: &'static str,
    remote: Remote,
    title: &'static str,
    description: &'static str,
    hint: Option<&'static str>,
    needs: PluginCapability,
    place: Place,
}

const fn spec(
    name: &'static str,
    remote: Remote,
    title: &'static str,
    description: &'static str,
    hint: Option<&'static str>,
    needs: PluginCapability,
    place: Place,
) -> Spec {
    Spec { name, remote, title, description, hint, needs, place }
}

const SEND: PluginCapability = PluginCapability::SessionSend;
const READ: PluginCapability = PluginCapability::SessionRead;
const CONTROL: PluginCapability = PluginCapability::SessionControl;

const SPECS: &[Spec] = &[
    spec(
        "plan",
        Remote::Plan,
        "Plan 模式",
        "按 Plan 模式开始一轮：先梳理方案，由你决定何时实施",
        Some("要做的事"),
        SEND,
        Place::Composer,
    ),
    spec(
        "goal",
        Remote::Goal,
        "Goal 模式",
        "按 Goal 模式开始一轮，持续执行到目标完成",
        Some("目标"),
        SEND,
        Place::Composer,
    ),
    spec(
        "init",
        Remote::Init,
        "初始化工作区",
        "建立或维护当前会话工作区根目录的 AGENTS.md",
        Some("补充说明（可选）"),
        SEND,
        Place::Composer,
    ),
    spec(
        "model",
        Remote::Model,
        "切换模型",
        "列出能用的模型；带编号就换当前会话的模型",
        Some("编号（可选）"),
        CONTROL,
        Place::Composer,
    ),
    spec(
        "permission",
        Remote::Permission,
        "切换权限",
        "列出三种权限模式；带编号就切换，放宽要打开远程批准",
        Some("编号（可选）"),
        CONTROL,
        Place::Composer,
    ),
    spec(
        "stop",
        Remote::Stop,
        "停止当前任务",
        "停止当前会话正在运行的任务",
        None,
        CONTROL,
        Place::Composer,
    ),
    spec(
        "compact",
        Remote::Compact,
        "压缩上下文",
        "压缩当前会话的上下文；后面的文字作为压缩重点",
        Some("压缩重点（可选）"),
        CONTROL,
        Place::Composer,
    ),
    spec("help", Remote::Help, "命令帮助", "列出这个聊天能用的命令", None, SEND, Place::Composer),
    spec("sessions", Remote::Sessions, "会话列表", "列出能进入的会话", None, READ, Place::Chat),
    spec(
        "switch",
        Remote::Switch,
        "切换会话",
        "把这个聊天换到会话列表里的那个会话",
        Some("编号"),
        CONTROL,
        Place::Chat,
    ),
    spec(
        "new",
        Remote::New,
        "新建会话",
        "新建一个会话，这个聊天换过去",
        Some("标题（可选）"),
        CONTROL,
        Place::Chat,
    ),
    spec(
        "status",
        Remote::Status,
        "会话状态",
        "当前会话的标题、工作区、模型、权限和运行情况",
        None,
        READ,
        Place::Chat,
    ),
    spec(
        "approve",
        Remote::Decide(RemoteDecision::Approve),
        "批准",
        "批准交给这个聊天的审批，仅此一次",
        Some("编号（可选）"),
        CONTROL,
        Place::Deciding,
    ),
    spec(
        "approve-session",
        Remote::Decide(RemoteDecision::ApproveSession),
        "本会话批准",
        "批准交给这个聊天的审批，本会话里同样的操作都允许",
        Some("编号（可选）"),
        CONTROL,
        Place::Deciding,
    ),
    spec(
        "deny",
        Remote::Decide(RemoteDecision::Deny),
        "拒绝",
        "拒绝交给这个聊天的审批",
        Some("编号（可选）"),
        CONTROL,
        Place::Deciding,
    ),
];

fn spec_of(name: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|spec| spec.name == name)
}

/// The command the message starts with, if it starts with one; `/name@bot` is Telegram's way in
/// groups of naming the bot the command is for.
#[must_use]
pub fn typed(text: &str) -> Option<Typed<'_>> {
    let rest = text.trim_start().strip_prefix('/')?;
    let end = rest
        .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        .unwrap_or(rest.len());
    let (name, mut after) = rest.split_at(end);
    if !valid_command_name(name) {
        return None;
    }
    if let Some(bot) = after.strip_prefix('@') {
        let end = bot.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(bot.len());
        if end == 0 {
            return None;
        }
        after = &bot[end..];
    }
    if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        return None;
    }
    Some(Typed { name, arguments: after.trim() })
}

/// What `name` is for a chat of a plugin that declares `declared`: one of those only a chat has,
/// else the composer's command of that name that can be used now. None when it is no command,
/// and the message is plain text.
#[must_use]
pub fn remote_command(
    name: &str,
    catalog: &[CommandEntry],
    declared: &[PluginCapability],
) -> Option<RemoteCommand> {
    let own = |spec: &Spec| {
        if declared.contains(&spec.needs) {
            RemoteCommand::Builtin(spec.remote)
        } else {
            RemoteCommand::Undeclared(spec.needs)
        }
    };
    if let Some(spec) = spec_of(name).filter(|spec| spec.place != Place::Composer) {
        return Some(own(spec));
    }
    let entry = catalog.iter().find(|entry| entry.runnable() && entry.name == name)?;
    match (entry.source, entry.kind) {
        (CommandSource::Builtin, _) => Some(spec_of(name).map_or(RemoteCommand::Desktop, own)),
        (_, CommandKind::Prompt) => {
            entry.template.clone().map(|template| RemoteCommand::Prompt { template })
        }
        _ => None,
    }
}

/// The message a prompt command sends: its template with the text after the command where it
/// says `$ARGUMENTS`, or after the template when it does not say.
#[must_use]
pub fn expanded(template: &str, arguments: &str) -> String {
    if template.contains("$ARGUMENTS") {
        return template.replace("$ARGUMENTS", arguments).trim_end().to_owned();
    }
    let template = template.trim_end();
    if arguments.is_empty() { template.to_owned() } else { format!("{template}\n\n{arguments}") }
}

/// Aura's commands a chat of a plugin that declares `declared` can use, in menu order; the
/// decisions on approvals only while its chats may decide them (`deciding`).
fn usable<'a>(
    catalog: &'a [CommandEntry],
    declared: &'a [PluginCapability],
    deciding: bool,
) -> impl Iterator<Item = &'static Spec> + 'a {
    SPECS.iter().filter(move |spec| {
        declared.contains(&spec.needs)
            && match spec.place {
                Place::Chat => true,
                Place::Deciding => deciding,
                Place::Composer => catalog.iter().any(|entry| {
                    entry.source == CommandSource::Builtin
                        && entry.runnable()
                        && entry.name == spec.name
                }),
            }
    })
}

/// The prompt commands a chat can use from `sources`: none named like Aura's chat commands.
fn prompts<'a>(
    catalog: &'a [CommandEntry],
    declared: &'a [PluginCapability],
    sources: &'a [CommandSource],
) -> impl Iterator<Item = &'a CommandEntry> + 'a {
    catalog.iter().filter(move |entry| {
        declared.contains(&SEND)
            && entry.kind == CommandKind::Prompt
            && sources.contains(&entry.source)
            && entry.runnable()
            && entry.template.is_some()
            && spec_of(&entry.name).is_none()
    })
}

/// The menu of a channel that shows one; `deciding` while the plugin's chats may decide approvals.
/// A workspace's commands change with the conversation and are only in `/help`.
#[must_use]
pub fn menu(
    catalog: &[CommandEntry],
    declared: &[PluginCapability],
    deciding: bool,
) -> Vec<MenuCommand> {
    let own = usable(catalog, declared, deciding).map(|spec| MenuCommand {
        name: spec.name.to_owned(),
        title: spec.title.to_owned(),
        description: spec.description.to_owned(),
        argument_hint: spec.hint.map(str::to_owned),
    });
    let sources = [CommandSource::User, CommandSource::Plugin];
    let prompted = prompts(catalog, declared, &sources).map(|entry| MenuCommand {
        name: entry.name.clone(),
        title: entry.title.clone(),
        description: entry.description.clone(),
        argument_hint: entry.argument_hint.clone(),
    });
    own.chain(prompted).collect()
}

/// What `/help` answers: every command the chat can use now, its conversation's workspace's too;
/// `deciding` while the plugin's chats may decide approvals.
#[must_use]
pub fn help_text(
    catalog: &[CommandEntry],
    declared: &[PluginCapability],
    deciding: bool,
) -> String {
    let mut text = String::from("这个聊天能用的命令：");
    let mut line = |name: &str, hint: Option<&str>, label: &str| {
        let _ = match hint {
            Some(hint) => write!(text, "\n/{name} {hint} — {label}"),
            None => write!(text, "\n/{name} — {label}"),
        };
    };
    for spec in usable(catalog, declared, deciding) {
        line(spec.name, spec.hint, spec.title);
    }
    let sources = [CommandSource::User, CommandSource::Plugin, CommandSource::Workspace];
    for entry in prompts(catalog, declared, &sources) {
        // An untitled command's title is its name.
        let label = if entry.title == entry.name { &entry.description } else { &entry.title };
        line(&entry.name, entry.argument_hint.as_deref(), label);
    }
    text
}

/// The command messages each plugin's chats sent, the oldest first.
#[derive(Default)]
pub struct SeenCommands {
    seen: BTreeMap<String, VecDeque<(String, Instant)>>,
}

impl SeenCommands {
    /// True the first time the plugin's chats sent the command message `key` within
    /// [`SEEN_COMMAND_TIME`]; it is remembered from then on, the plugin's oldest going first once
    /// there are [`SEEN_COMMANDS_MAX`].
    pub fn first(&mut self, plugin: &str, key: &str, now: Instant) -> bool {
        let seen = self.seen.entry(plugin.to_owned()).or_default();
        while seen.front().is_some_and(|(_, at)| now.duration_since(*at) >= SEEN_COMMAND_TIME) {
            seen.pop_front();
        }
        if seen.iter().any(|(known, _)| known == key) {
            return false;
        }
        if seen.len() >= SEEN_COMMANDS_MAX {
            seen.pop_front();
        }
        seen.push_back((key.to_owned(), now));
        true
    }

    /// The plugin's service no longer runs.
    pub fn forget(&mut self, plugin: &str) {
        self.seen.remove(plugin);
    }
}
