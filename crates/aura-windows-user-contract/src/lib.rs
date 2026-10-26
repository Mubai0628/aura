#![forbid(unsafe_code)]
//! Windows implementation protocol. These DTOs do not confer authority: the
//! Runner accepts them only over its mutually process-bound Core connection.
use aura_domain_common::{
    ContentHash, ToolWireVersionV1, UtcTimestamp, jcs::CanonicalSha256HashV1,
};
use aura_execution_contract::{AttemptRef, DispatchRequest};
use aura_security_contract::PermissionMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
mod acceptance;
pub use acceptance::*;
mod maintenance;
pub use maintenance::*;
mod lifecycle;
pub use lifecycle::*;
mod shell_result;
pub use shell_result::*;
mod terminal;
pub use terminal::*;
mod terminal_opening;
pub use terminal_opening::*;
mod agent_terminal;
pub use agent_terminal::*;
mod terminal_state;
pub use terminal_state::*;
mod terminal_sensitive;
pub use terminal_sensitive::*;
mod managed_process;
pub use managed_process::*;
#[cfg(test)]
mod acceptance_tests;

pub const ASSURANCE_VERSION: &str = "windows-user-direct-network-v1";
pub const INSTALLATION_VERSION: &str = "windows-user-installation-v2";
pub const SHELL_ENVIRONMENT_VERSION: &str = "aura-shell-environment-v1";
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedShell {
    PowerShell,
    Cmd,
}

// The selected Built-in Tool fixes the shell. Identity, backend and network
// authority are never supplied by the model. Core binds the working directory.
// Only `script` is required; every other field has the default its description states.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShellExecuteInput {
    /// Default false. Set true when the script needs the network; read-only mode
    /// then asks for network approval. It declares a need and never
    /// grants access by itself.
    #[serde(default)]
    pub requires_network: bool,
    /// The script to run.
    pub script: String,
    /// Default: the workspace root. A path relative to it.
    #[serde(default = "workspace_root")]
    pub working_directory: String,
    /// Text for standard input. Default empty.
    #[serde(default)]
    pub stdin: String,
    /// Default 120000. After this many milliseconds Aura stops the whole process tree.
    /// The tool time limit in Tool settings (default 10 minutes, at most 30 minutes) also
    /// ends the call, so a value above that limit does not help. At most 86400000.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    /// Default 65536. Bytes of stdout and stderr kept, up to 1048576.
    #[serde(default = "default_max_output_bytes")]
    pub max_output_bytes: u64,
}

fn workspace_root() -> String {
    ".".to_owned()
}
/// The Shell timeout a call gets when it does not name one.
pub const DEFAULT_SHELL_TIMEOUT_MS: u64 = 120_000;
fn default_timeout_ms() -> u64 {
    DEFAULT_SHELL_TIMEOUT_MS
}
/// The output a call keeps when it does not name a limit.
pub const DEFAULT_SHELL_MAX_OUTPUT_BYTES: u64 = 65_536;
fn default_max_output_bytes() -> u64 {
    DEFAULT_SHELL_MAX_OUTPUT_BYTES
}
impl ShellExecuteInput {
    pub fn command(&self, shell: ManagedShell) -> Result<ManagedShellCommand, RunnerProtocolError> {
        let command = ManagedShellCommand {
            shell,
            script: self.script.clone(),
            working_directory: self.working_directory.clone(),
            stdin: self.stdin.clone(),
            timeout_ms: self.timeout_ms,
            max_output_bytes: self.max_output_bytes,
        };
        command.validate()?;
        Ok(command)
    }
}

/// Model-visible command input. Runtime location, PATH, arguments, credentials,
/// account identity and channel are deliberately not request fields.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedShellCommand {
    pub shell: ManagedShell,
    pub script: String,
    pub working_directory: String,
    pub stdin: String,
    pub timeout_ms: u64,
    pub max_output_bytes: u64,
}
impl ManagedShellCommand {
    pub fn validate(&self) -> Result<(), RunnerProtocolError> {
        if self.script.trim().is_empty()
            || self.script.len() > 256 * 1024
            || self.script.contains('\0')
            || self.working_directory.is_empty()
            || self.working_directory.contains('\0')
            || self.stdin.len() > 4 * 1024 * 1024
            || self.timeout_ms == 0
            || self.timeout_ms > 24 * 60 * 60 * 1000
            || self.max_output_bytes == 0
            || self.max_output_bytes > 1024 * 1024
        {
            return Err(RunnerProtocolError::InvalidCommand);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionFileBinding {
    pub path: String,
    /// Decimal string: Windows volume IDs may exceed the exact JSON/JCS integer range.
    pub volume_serial: String,
    pub file_id: [u8; 16],
    pub sha256: Option<ContentHash>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecutionAccount {
    pub name: String,
    pub sid: String,
    pub credential_sha256: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstallationManifest {
    pub schema_version: ToolWireVersionV1,
    pub installation_version: String,
    pub assurance_version: String,
    pub revision: aura_domain_common::Revision,
    pub installation_id: String,
    pub owner_sid: String,
    pub resource_record_hash: ContentHash,
    pub maintenance_config_sha256: ContentHash,
    pub core_image: ExecutionFileBinding,
    pub runner_image: ExecutionFileBinding,
    pub cmd_image: ExecutionFileBinding,
    pub powershell_image: ExecutionFileBinding,
    pub offline: ExecutionAccount,
    pub online: Option<ExecutionAccount>,
}

pub const SYSTEM_CMD_CONVERSION_VERSION: &str = "windows-user-system-cmd-conversion-v1";
pub const SYSTEM_CMD_APPLY_VERSION: &str = "windows-user-system-cmd-apply-v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemCmdConversionState {
    Pending,
    Applied,
}

/// Owner-audited replacement of the OS CMD image only. Core, Runner, and pwsh
/// stay exact; inspect remains fail-closed until this record is applied.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SystemCmdConversion {
    pub schema: String,
    pub installation_id: String,
    pub from_revision: aura_domain_common::Revision,
    pub to_revision: aura_domain_common::Revision,
    pub from_cmd_image: ExecutionFileBinding,
    pub to_cmd_image: ExecutionFileBinding,
    pub from_fingerprint: CanonicalSha256HashV1,
    pub to_fingerprint: CanonicalSha256HashV1,
    pub core_sha256: ContentHash,
    pub runner_sha256: ContentHash,
    pub powershell_sha256: ContentHash,
    pub state: SystemCmdConversionState,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemCmdApplyPlan {
    pub schema: String,
    pub manifest: InstallationManifest,
    pub conversion: Option<SystemCmdConversion>,
    /// Uppercase SHA-256 of the exact execution.json bytes this plan writes.
    pub manifest_sha256: String,
}

/// Prepared by the Core-side backend, included in the immutable Plan hash.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsUserPreparation {
    pub schema_version: ToolWireVersionV1,
    pub assurance_version: String,
    pub installation_fingerprint: CanonicalSha256HashV1,
    pub shell_environment_version: String,
    pub mode: PermissionMode,
    pub permission_revision: u64,
    pub valid_until: UtcTimestamp,
    pub account_sid: String,
    pub shell_image: ExecutionFileBinding,
    pub working_directory: ExecutionFileBinding,
    pub read_roots: Vec<ExecutionFileBinding>,
    pub write_roots: Vec<ExecutionFileBinding>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerDispatchEnvelope {
    pub schema_version: ToolWireVersionV1,
    pub dispatch: DispatchRequest,
    pub expires_at: UtcTimestamp,
    pub attempt_write_sid: String,
    pub scratch_directory: ExecutionFileBinding,
}
impl RunnerDispatchEnvelope {
    pub fn validate(
        &self,
        now: UtcTimestamp,
    ) -> Result<WindowsUserPreparation, RunnerProtocolError> {
        let request = &self.dispatch;
        request.operation.validate().map_err(|_| RunnerProtocolError::InvalidBinding)?;
        request.plan.validate().map_err(|_| RunnerProtocolError::InvalidBinding)?;
        if self.expires_at <= now
            || request.binding.attempt.operation_id != request.operation.operation_id
            || request.binding.operation_fingerprint != request.operation.fingerprint
            || request.plan.prepared.operation_fingerprint != request.operation.fingerprint
            || request.binding.plan_fingerprint != request.plan.fingerprint
        {
            return Err(RunnerProtocolError::InvalidBinding);
        }
        let command: ManagedShellCommand =
            serde_json::from_value(request.operation.content.action.clone())
                .map_err(|_| RunnerProtocolError::InvalidCommand)?;
        command.validate()?;
        let preparation: WindowsUserPreparation =
            serde_json::from_value(request.plan.prepared.preparation.clone())
                .map_err(|_| RunnerProtocolError::InvalidBinding)?;
        if preparation.assurance_version != ASSURANCE_VERSION
            || preparation.valid_until < self.expires_at
            || preparation.permission_revision == 0
            || preparation.shell_environment_version != SHELL_ENVIRONMENT_VERSION
            || (preparation.mode == PermissionMode::ReadOnly && !preparation.write_roots.is_empty())
            || !matches!(
                request.operation.content.action_kind.as_str(),
                "shell.powershell" | "shell.cmd"
            )
            || (command.shell == ManagedShell::PowerShell)
                != (request.operation.content.action_kind == "shell.powershell")
        {
            return Err(RunnerProtocolError::InvalidBinding);
        }
        Ok(preparation)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerStopReason {
    Exited,
    Cancelled,
    TimedOut,
    OutputLimit,
    CoreDisconnected,
    Unknown,
}

/// Raw execution facts, not an Agent result or an Artifact receipt. Core owns
/// publication. Generic script side effects are unassessed, not execution loss.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerObservation {
    pub attempt: AttemptRef,
    pub binding_fingerprint: CanonicalSha256HashV1,
    pub account_sid: String,
    pub process_id: u32,
    pub shell_image: ExecutionFileBinding,
    pub started_at: UtcTimestamp,
    pub finished_at: UtcTimestamp,
    pub exit_code: Option<u32>,
    pub stop_reason: RunnerStopReason,
    pub process_tree_stopped: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Exact executed contents, including Aura's fixed Shell bootstrap.
    #[serde(default)]
    pub submitted_script: String,
    #[serde(default)]
    pub output_truncated: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "message", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreRunnerMessage {
    Execute { envelope: Box<RunnerDispatchEnvelope> },
    Cancel { attempt: AttemptRef },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "message", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunnerCoreMessage {
    Ready,
    Started { attempt: AttemptRef, process_id: u32 },
    Progress { update: RunnerProgress },
    Completed { observation: Box<RunnerObservation> },
    Background { update: RunnerBackgroundUpdate },
    Rejected { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum RunnerProtocolError {
    #[error("runner.invalid_command")]
    InvalidCommand,
    #[error("runner.invalid_or_expired_binding")]
    InvalidBinding,
}
