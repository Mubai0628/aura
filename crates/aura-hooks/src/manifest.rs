//! `hook.toml`: what a Hook is, which events it hears and which program runs.
use crate::event::{HookEvent, unavailable};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const HOOK_MANIFEST_FILE: &str = "hook.toml";
const MAX_MANIFEST_BYTES: usize = 16 * 1024;
const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const TIMEOUT_RANGE_MS: std::ops::RangeInclusive<u64> = 100..=60_000;

/// `user` Hooks fail open, `security` Hooks fail closed, unless the manifest says otherwise.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    #[default]
    User,
    Security,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookFailurePolicy {
    /// A failed Hook is recorded and treated as if it had answered `continue`.
    FailOpen,
    /// A failed Hook blocks a pre-event; on later events it stops the remaining Hooks.
    FailClosed,
}

/// The program, always inside the package: its executable, or a script run by the
/// PowerShell that ships with Aura (the same two forms tool packages use).
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookCommand {
    Executable { path: String, arguments: Vec<String> },
    PowershellScript { path: String, arguments: Vec<String> },
}

impl HookCommand {
    pub fn path(&self) -> &str {
        match self {
            Self::Executable { path, .. } | Self::PowershellScript { path, .. } => path,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HookDefinition {
    pub name: String,
    pub version: String,
    pub description: String,
    pub kind: HookKind,
    pub failure_policy: HookFailurePolicy,
    pub timeout_ms: u64,
    pub events: Vec<HookEvent>,
    pub command: HookCommand,
    /// Tool names (`fs.write`) or families (`shell.*`) for `tool.*` events; empty is every tool.
    pub tools: Vec<String>,
}

impl HookDefinition {
    pub fn hears(&self, event: HookEvent) -> bool {
        self.events.contains(&event)
    }

    /// Whether a `tool.*` event for this tool reaches the Hook.
    pub fn matches_tool(&self, tool: &str) -> bool {
        self.tools.is_empty()
            || self.tools.iter().any(|pattern| match pattern.strip_suffix(".*") {
                Some(family) => tool.strip_prefix(family).is_some_and(|rest| rest.starts_with('.')),
                None => pattern == tool,
            })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema_version: u32,
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    kind: HookKind,
    #[serde(default)]
    failure_policy: Option<HookFailurePolicy>,
    #[serde(default)]
    isolation: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    events: Vec<String>,
    command: RawCommand,
    #[serde(default, rename = "match")]
    matcher: Option<RawMatch>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCommand {
    #[serde(default)]
    executable: Option<String>,
    #[serde(default)]
    powershell_script: Option<String>,
    #[serde(default)]
    arguments: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMatch {
    #[serde(default)]
    tools: Vec<String>,
}

/// Every problem at once, in the user's language, so the settings page can show them all.
pub fn parse_manifest(text: &str) -> Result<HookDefinition, Vec<String>> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(vec![format!("{HOOK_MANIFEST_FILE} 超过 {MAX_MANIFEST_BYTES} 字节")]);
    }
    let raw: RawManifest = toml::from_str(text)
        .map_err(|error| vec![format!("{HOOK_MANIFEST_FILE} 格式不对：{}", error.message())])?;
    let mut problems = Vec::new();
    if raw.schema_version != 1 {
        problems.push(format!("schema_version 必须是 1（现在是 {}）", raw.schema_version));
    }
    if !valid_name(&raw.name) {
        problems.push("name 只能用小写字母、数字和 -，以字母开头，最长 64 个字符".into());
    }
    if raw.version.trim().is_empty() || raw.version.len() > 32 || has_control(&raw.version) {
        problems.push("version 不能为空，最长 32 个字符".into());
    }
    if raw.description.chars().count() > 500 || has_control(&raw.description) {
        problems.push("description 最长 500 个字符，不能含控制字符".into());
    }
    match raw.isolation.as_deref() {
        None | Some("compatibility") => {}
        Some("strict") => problems.push(
            "isolation = \"strict\" 需要受限沙箱，Aura 目前没有；首版只支持 compatibility（以你的身份在 Job 里运行）".into(),
        ),
        Some("native") => {
            problems.push("isolation = \"native\" 首版不支持；请用 compatibility".into())
        }
        Some(other) => problems.push(format!("isolation 不认识：{other}")),
    }
    let timeout_ms = raw.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    if !TIMEOUT_RANGE_MS.contains(&timeout_ms) {
        problems.push("timeout_ms 必须在 100 到 60000 之间".into());
    }
    let mut events = Vec::new();
    if raw.events.is_empty() {
        problems.push("events 至少要有一个事件".into());
    }
    for name in &raw.events {
        match (HookEvent::parse(name), unavailable(name)) {
            (Some(event), _) if events.contains(&event) => {
                problems.push(format!("事件 {name} 重复"))
            }
            (Some(event), _) => events.push(event),
            (None, Some(reason)) => problems.push(reason.into()),
            (None, None) => problems.push(format!("事件 {name} 不存在")),
        }
    }
    let command = match (raw.command.executable, raw.command.powershell_script) {
        (Some(path), None) => {
            if !path.to_ascii_lowercase().ends_with(".exe") {
                problems.push("command.executable 必须是包内的 .exe 文件".into());
            }
            HookCommand::Executable { path, arguments: raw.command.arguments }
        }
        (None, Some(path)) => {
            if !path.to_ascii_lowercase().ends_with(".ps1") {
                problems.push("command.powershell_script 必须是包内的 .ps1 文件".into());
            }
            HookCommand::PowershellScript { path, arguments: raw.command.arguments }
        }
        _ => {
            problems.push("command 里 executable 与 powershell_script 必须且只能有一个".into());
            HookCommand::Executable { path: String::new(), arguments: vec![] }
        }
    };
    if !command.path().is_empty() && !package_relative(command.path()) {
        problems.push(format!("命令路径 {} 必须是包内的相对路径", command.path()));
    }
    let arguments = match &command {
        HookCommand::Executable { arguments, .. }
        | HookCommand::PowershellScript { arguments, .. } => arguments,
    };
    if arguments.len() > 32 || arguments.iter().any(|a| a.len() > 1024 || a.contains('\0')) {
        problems.push("command.arguments 最多 32 个，每个最长 1024 字节".into());
    }
    let tools = raw.matcher.map(|m| m.tools).unwrap_or_default();
    if tools.len() > 64 || !tools.iter().all(|pattern| valid_tool_pattern(pattern)) {
        problems.push(
            "match.tools 只能写工具名（如 fs.write）或工具族（如 shell.*），最多 64 个".into(),
        );
    }
    if !tools.is_empty() && !events.iter().any(|event| event.tool_scoped()) {
        problems.push("match.tools 只对 tool.* 事件有意义，但没有订阅 tool.* 事件".into());
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(HookDefinition {
        name: raw.name,
        version: raw.version.trim().to_owned(),
        description: raw.description,
        failure_policy: raw.failure_policy.unwrap_or(match raw.kind {
            HookKind::User => HookFailurePolicy::FailOpen,
            HookKind::Security => HookFailurePolicy::FailClosed,
        }),
        kind: raw.kind,
        timeout_ms,
        events,
        command,
        tools,
    })
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn has_control(text: &str) -> bool {
    text.chars().any(char::is_control)
}

/// A relative path that stays inside the package: no root, drive, `..` or empty part.
pub(crate) fn package_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(':')
        && !path.starts_with(['/', '\\'])
        && path.split(['/', '\\']).all(|part| !part.is_empty() && part != "." && part != "..")
}

fn valid_tool_pattern(pattern: &str) -> bool {
    let name = pattern.strip_suffix(".*").unwrap_or(pattern);
    !name.is_empty()
        && name.len() <= 128
        && name.split('.').all(|part| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}
