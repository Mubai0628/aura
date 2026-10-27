//! Execution observations, not certification of arbitrary script side effects.
use super::*;
use aura_domain_common::ToolCallId;

pub const RUNNER_WIRE_VERSION: &str = "aura-runner-lifecycle-v2";

/// Ordered direct-execution deltas, retained even if no terminal frame arrives.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerProgress {
    pub attempt: AttemptRef,
    pub sequence: u64,
    pub observed_at: UtcTimestamp,
    pub submitted_script: Option<String>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub output_truncated: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundState {
    Running,
    Exited,
    Interrupted,
    Unknown,
}

/// Appended after the immutable direct-execution result. Bytes are deltas, and
/// sequence numbers prevent duplicate or reordered output from replacing facts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerBackgroundUpdate {
    pub attempt: AttemptRef,
    pub sequence: u64,
    pub observed_at: UtcTimestamp,
    pub state: BackgroundState,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub output_truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShellQueryInput {
    /// The tool_call_id of a shell.powershell or shell.cmd result in this session (tcl_…); not
    /// the provider's call id, a tool name or an artifact id.
    pub tool_call_id: ToolCallId,
    /// Byte offset into stdout. Default 0.
    #[serde(default)]
    pub stdout_offset: u64,
    /// Byte offset into stderr. Default 0.
    #[serde(default)]
    pub stderr_offset: u64,
    /// Bytes per page. Default 65536.
    #[serde(default = "default_query_bytes")]
    pub max_bytes: u32,
}
fn default_query_bytes() -> u32 {
    65_536
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShellStopInput {
    /// The tool_call_id of a shell.powershell or shell.cmd result in this session (tcl_…).
    pub tool_call_id: ToolCallId,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellCallState {
    Waiting,
    Running,
    Completed,
    Failed,
    Interrupted,
    Indeterminate,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShellExecutionView {
    pub tool_call_id: ToolCallId,
    pub original_input: ShellExecuteInput,
    pub command: ManagedShellCommand,
    pub submitted_script: Option<String>,
    pub environment_version: String,
    pub shell_image: Option<ExecutionFileBinding>,
    pub state: ShellCallState,
    pub process_id: Option<u32>,
    pub started_at: Option<UtcTimestamp>,
    pub finished_at: Option<UtcTimestamp>,
    pub exit_code: Option<u32>,
    pub stop_reason: Option<RunnerStopReason>,
    pub diagnostic: Option<String>,
    /// Names only, from the launcher's actual filtered environment.
    #[serde(default)]
    pub filtered_environment_names: Vec<String>,
    pub background: Option<BackgroundState>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_total: u64,
    pub stderr_total: u64,
    pub stdout_next: Option<u64>,
    pub stderr_next: Option<u64>,
    pub output_truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShellQueryRequest {
    pub session_id: aura_domain_common::SessionId,
    pub input: ShellQueryInput,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShellStopRequest {
    pub session_id: aura_domain_common::SessionId,
    pub input: ShellStopInput,
}
