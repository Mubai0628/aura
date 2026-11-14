//! Model-facing terminal inputs contain no desktop authority or credential bytes.
use crate::{ManagedShell, TerminalEnd};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentTerminalShell {
    #[default]
    Powershell,
    Cmd,
}
impl From<AgentTerminalShell> for ManagedShell {
    fn from(value: AgentTerminalShell) -> Self {
        match value {
            AgentTerminalShell::Powershell => Self::PowerShell,
            AgentTerminalShell::Cmd => Self::Cmd,
        }
    }
}
fn directory() -> String {
    ".".into()
}
fn wait() -> u64 {
    15000
}
fn lines() -> u32 {
    200
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentTerminalOpenInput {
    #[serde(default)]
    pub shell: AgentTerminalShell,
    #[serde(default = "directory")]
    pub working_directory: String,
    #[serde(default)]
    pub requires_network: bool,
    #[serde(default)]
    #[schemars(length(max = 120))]
    pub title: String,
    /// Default none. Credentials of the user's environment this terminal needs, by variable name
    /// (names with TOKEN, KEY, SECRET, PASSWORD, PASS, PWD or the like, or holding a
    /// user:password@ address). Only these are passed; their values and every other
    /// credential's are hidden in the output.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_environment: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentTerminalWriteInput {
    pub terminal_id: String,
    #[serde(default)]
    #[schemars(length(max = 65536))]
    pub input: String,
    #[serde(default)]
    pub interrupt: bool,
    #[serde(default = "wait")]
    #[schemars(range(max = 30000))]
    pub wait_ms: u64,
    #[serde(default)]
    #[schemars(length(max = 1024))]
    pub until_text: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentTerminalReadInput {
    pub terminal_id: String,
    /// The first line to read; the terminal's first line with text is line 1. Without it the
    /// newest lines are read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub line: Option<u64>,
    #[serde(default = "lines")]
    #[schemars(range(min = 1, max = 2000))]
    pub max_lines: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentTerminalCloseInput {
    pub terminal_id: String,
}
/// A page of the lines an Agent terminal shows, numbered from its first line with text.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentTerminalLinesV1 {
    pub terminal_id: String,
    /// The lines as the terminal shows them, joined with line breaks.
    pub lines: String,
    /// The number of the first line in `lines`.
    pub first_line: u64,
    /// The number of the terminal's newest line.
    pub last_line: u64,
    /// The `line` that reads on after this page; absent when the page reaches the newest line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_line: Option<u64>,
    /// The oldest line still kept; earlier ones are gone.
    pub earliest_line: u64,
    pub running: bool,
    pub end: Option<TerminalEnd>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentTerminalResultV1 {
    pub terminal_id: String,
    pub output: String,
    pub start_offset: u64,
    pub next_offset: u64,
    pub truncated: bool,
    pub running: bool,
    pub end: Option<TerminalEnd>,
}
