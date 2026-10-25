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
    2000
}
fn page() -> usize {
    8192
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
    #[serde(default)]
    pub offset: u64,
    #[serde(default = "page")]
    #[schemars(range(min = 1, max = 65536))]
    #[schemars(with = "u32")]
    pub max_bytes: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentTerminalCloseInput {
    pub terminal_id: String,
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
