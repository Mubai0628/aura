//! What a sub-agent may be given and the calls it is refused whatever its table says. It has
//! every tool of the conversation that starts it but the main agent's own, unless the main agent
//! narrowed the list; what a call may then do is its conversation's permission mode and rules.
use crate::Refusal;
use std::path::{Path, PathBuf};

/// The Skill tools: a narrowed sub-agent has them whenever the starting conversation does,
/// whatever list the main agent asked for. Skills are guidance and grant nothing.
pub const SKILL_TOOLS: &[&str] = &["skills.load", "skills.search"];

/// The tool every sub-agent has, and no conversation of the user's.
pub const REPORT_TOOL: &str = "agent.report";

/// The tools only the main agent has.
pub const COORDINATOR_TOOLS: &[&str] = &[
    "agents.approve",
    "agents.discard",
    "agents.merge",
    "agents.result",
    "agents.send",
    "agents.spawn",
    "agents.status",
    "agents.stop",
    "agents.wait",
];

/// Whether only the main agent has `tool`: it starts or steers sub-agents; it ends, plans or
/// reconciles the user's turn; it asks the user, which a sub-agent does through its report; or it
/// drives the user's own Chrome or desktop, which one agent uses at a time.
#[must_use]
pub fn main_agent_only(tool: &str) -> bool {
    ["agents.", "chrome.", "computer.", "task_lineage.", "goal.", "plugin.chrome-control."]
        .iter()
        .any(|prefix| tool.starts_with(prefix))
        || [
            "normal.complete",
            "mode.disposition",
            "plan.complete",
            "workspace_init.complete",
            "session.reply",
            "query.user_question",
            "terminal.request_sensitive_input",
            "browser.request_user",
        ]
        .contains(&tool)
}

/// The tools a new sub-agent is narrowed to: none when the main agent named none, and it has every
/// tool a sub-agent may have. Otherwise the ones it named, each a tool the starting conversation
/// has and a sub-agent may have, and the [`SKILL_TOOLS`]. A tool outside that set is refused,
/// never added.
pub fn granted_tools(
    requested: Option<&[String]>,
    parent_tools: &[String],
) -> Result<Option<Vec<String>>, Refusal> {
    let Some(requested) = requested.filter(|requested| !requested.is_empty()) else {
        return Ok(None);
    };
    let available = |tool: &str| {
        tool != REPORT_TOOL && !main_agent_only(tool) && parent_tools.iter().any(|own| own == tool)
    };
    let mut granted = Vec::new();
    for tool in requested {
        if !available(tool) {
            return Err(Refusal::new(
                "agents.tool_not_available",
                format!(
                    "{tool} 不能给子 Agent：这个会话没有它，或者它只给主 Agent。不填 tools 时子 Agent 有主 Agent 专用以外的全部工具。"
                ),
            ));
        }
        if !granted.contains(tool) {
            granted.push(tool.clone());
        }
    }
    for tool in SKILL_TOOLS {
        if available(tool) && !granted.iter().any(|own| own == tool) {
            granted.push((*tool).to_owned());
        }
    }
    granted.sort();
    Ok(Some(granted))
}

/// A call a sub-agent may not make whatever its tool table holds, judged by its name and
/// arguments: a tool only the main agent has, another conversation's history, or a tool outside
/// the list the main agent `narrowed` it to. The paths it reaches are judged where they are
/// resolved.
#[must_use]
pub fn refused_call(
    tool: &str,
    arguments: &serde_json::Value,
    narrowed: Option<&[String]>,
) -> Option<Refusal> {
    if tool == REPORT_TOOL {
        return None;
    }
    if tool.starts_with("agents.") {
        return Some(Refusal::new("agents.not_for_subagents", "子 Agent 不能派或管理子 Agent。"));
    }
    if main_agent_only(tool) {
        return Some(Refusal::new(
            "agents.main_agent_only",
            format!("{tool} 只给主 Agent。要问用户或要主 Agent 做的事，写进报告。"),
        ));
    }
    if tool.starts_with("history.") && arguments.get("conversation").is_some() {
        return Some(Refusal::new(
            "agents.other_conversation",
            "子 Agent 不能读别的会话；需要的事实请在报告里向主 Agent 要。",
        ));
    }
    if let Some(tools) = narrowed
        && !tools.iter().any(|granted| granted == tool)
    {
        return Some(Refusal::new(
            "agents.tool_not_granted",
            format!("这个子 Agent 没有 {tool}。可用的工具：{}。", tools.join("、")),
        ));
    }
    None
}

/// The refusal of a path in Aura's own data.
#[must_use]
pub fn data_refusal() -> Refusal {
    Refusal::new(
        "agents.aura_data",
        "子 Agent 不能读写 Aura 的数据位置（会话、记忆和设置都在那里）。",
    )
}

/// The refusal of a path in another workspace: the main one or another sub-agent's worktree.
#[must_use]
pub fn other_workspace_refusal() -> Refusal {
    Refusal::new(
        "agents.other_workspace",
        "子 Agent 只在自己的工作区里干活；主工作区和别的子 Agent 的工作副本由主 Agent 合并和处理。",
    )
}

/// The data folder a command names, after `%NAME%`, `$env:NAME`, `${env:NAME}` and a leading `~`
/// are replaced by their values. A command that reaches a folder without naming it is not seen.
#[must_use]
pub fn names_aura_data(
    script: &str,
    folders: &[PathBuf],
    variable: &dyn Fn(&str) -> Option<String>,
) -> Option<PathBuf> {
    let expanded = normalized(&expand(script, variable));
    folders.iter().find(|folder| contains_folder(&expanded, folder)).cloned()
}

fn contains_folder(text: &str, folder: &Path) -> bool {
    let needle = normalized(&folder.to_string_lossy());
    let needle = needle.trim_end_matches('\\');
    if needle.is_empty() {
        return false;
    }
    text.match_indices(needle).any(|(start, _)| {
        text[start + needle.len()..]
            .chars()
            .next()
            .is_none_or(|next| !(next.is_alphanumeric() || matches!(next, '-' | '_' | '.')))
    })
}

fn normalized(text: &str) -> String {
    let text = text.strip_prefix(r"\\?\").unwrap_or(text);
    let mut out = String::with_capacity(text.len());
    let mut previous = '\0';
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = if c == '/' { '\\' } else { c };
        if !(c == '\\' && previous == '\\') {
            out.push(c);
        }
        previous = c;
    }
    out
}

fn expand(script: &str, variable: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(script.len());
    let mut rest = script;
    while let Some(c) = rest.chars().next() {
        let replaced = match c {
            '%' => rest[1..].find('%').and_then(|end| {
                let name = &rest[1..=end];
                variable(name).map(|value| (value, end + 2))
            }),
            '$' => env_reference(rest).and_then(|(name, used)| variable(name).map(|v| (v, used))),
            '~' if out.is_empty() || out.ends_with([' ', '"', '\'', '(', '=', ',']) => {
                variable("USERPROFILE").map(|value| (value, 1))
            }
            _ => None,
        };
        match replaced {
            Some((value, used)) => {
                out.push_str(&value);
                rest = &rest[used..];
            }
            None => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// `$env:NAME` or `${env:NAME}`: the name and how many bytes the reference takes.
fn env_reference(text: &str) -> Option<(&str, usize)> {
    let starts = |prefix: &str| {
        text.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    if starts("${env:") {
        let end = text.find('}')?;
        return Some((&text[6..end], end + 1));
    }
    if starts("$env:") {
        let length = text[5..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(text.len() - 5);
        return (length > 0).then(|| (&text[5..5 + length], 5 + length));
    }
    None
}
