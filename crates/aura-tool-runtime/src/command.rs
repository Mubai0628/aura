use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use aura_domain_common::SandboxExecutionId;
use aura_permission_domain::SandboxHostEvent;
use aura_tool_contract::{
    CommandCredentialEnvironmentSlotV1, CommandEnvironmentEntryV1, CommandEnvironmentValueV1,
    CommandWorkingDirectoryV1, DeclarativeCommandV1, EnvironmentVariableNameV1,
    PackageRelativePath, PackageResourceClaimKeyV1, STDIO_PROTOCOL_VERSION_V1,
    ToolRuntimeDescriptorV1, validate_command_environment_v1,
};
use serde_json::Value;
use thiserror::Error;

use crate::{ToolArgumentResolutionErrorV1, render_command_arguments_v1, required_scalar_text};

#[derive(Debug, Error)]
pub enum PreparedCommandErrorV1 {
    #[error("the declarative command contract is invalid")]
    ContractInvalid,
    #[error("a command argument or environment value cannot be resolved")]
    Argument(#[from] ToolArgumentResolutionErrorV1),
    #[error("the requested working directory claim is unavailable")]
    WorkingDirectoryUnavailable,
    #[error("the trusted PowerShell executable is unavailable")]
    PowershellUnavailable,
    #[error("a trusted Core environment value is invalid")]
    CoreEnvironmentInvalid,
    #[error("the Tool runtime cannot use the short-lived Sandbox process path")]
    UnsupportedRuntime,
}

/// One non-secret environment value frozen before P04 evaluation. Attempt
/// TEMP/TMP and Credential plaintext are deliberately materialized only after
/// an Allow decision and therefore cannot appear here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedEnvironmentEntryV1 {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparedCommandRuntimeKindV1 {
    Argv,
    PowershellScript,
    AuraStdio,
}

#[derive(Clone, Debug)]
pub struct PreparedCommandLaunchV1 {
    pub runtime_kind: PreparedCommandRuntimeKindV1,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    pub environment: Vec<PreparedEnvironmentEntryV1>,
    pub credential_slot: Option<CommandCredentialEnvironmentSlotV1>,
    pub success_exit_codes: Vec<i32>,
    pub stdio_protocol_version: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandHostTerminalEventV1 {
    Exited { exit_code: u32 },
    Cancelled,
    TimedOut,
    PolicyViolation { code: String },
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CommandHostEventErrorV1 {
    #[error("Sandbox Host event identity does not match the authorized command execution")]
    IdentityMismatch,
    #[error("Sandbox Host command output chunk sequence is not contiguous")]
    OutputSequenceInvalid,
    #[error("Sandbox Host emitted multiple command terminal events")]
    TerminalConflict,
    #[error("Sandbox Host emitted command output after its execution terminal")]
    OutputAfterTerminal,
}

/// Core-side evidence collector for one short-lived declarative command. Host
/// chunk boundaries are transport facts; stdout and stderr remain separate and
/// their sequence spaces are independently monotonic.
pub struct CommandSandboxHostTrackerV1 {
    sandbox_execution_id: SandboxExecutionId,
    next_stdout_sequence: u64,
    next_stderr_sequence: u64,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    terminal_event: Option<CommandHostTerminalEventV1>,
    cleanup_failed: Option<String>,
}

impl CommandSandboxHostTrackerV1 {
    #[must_use]
    pub const fn new(sandbox_execution_id: SandboxExecutionId) -> Self {
        Self {
            sandbox_execution_id,
            next_stdout_sequence: 0,
            next_stderr_sequence: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
            terminal_event: None,
            cleanup_failed: None,
        }
    }

    pub fn accept_host_event(
        &mut self,
        event: SandboxHostEvent,
    ) -> Result<(), CommandHostEventErrorV1> {
        if command_event_execution_id(&event) != self.sandbox_execution_id {
            return Err(CommandHostEventErrorV1::IdentityMismatch);
        }
        match event {
            SandboxHostEvent::StdoutChunk { sequence, bytes, .. } => {
                self.accept_output_chunk(true, sequence, &bytes)
            }
            SandboxHostEvent::StderrChunk { sequence, bytes, .. } => {
                self.accept_output_chunk(false, sequence, &bytes)
            }
            SandboxHostEvent::Exit { exit_code, .. } => {
                self.set_terminal(CommandHostTerminalEventV1::Exited { exit_code })
            }
            SandboxHostEvent::Cancelled { .. } => {
                self.set_terminal(CommandHostTerminalEventV1::Cancelled)
            }
            SandboxHostEvent::Timeout { .. } => {
                self.set_terminal(CommandHostTerminalEventV1::TimedOut)
            }
            SandboxHostEvent::PolicyViolation { code, .. } => {
                self.set_terminal(CommandHostTerminalEventV1::PolicyViolation { code })
            }
            SandboxHostEvent::CleanupFailed { code, .. } => {
                self.cleanup_failed = Some(code);
                Ok(())
            }
            SandboxHostEvent::Started { .. }
            | SandboxHostEvent::ProcessStarted { .. }
            | SandboxHostEvent::CleanupCompleted { .. } => Ok(()),
        }
    }

    #[must_use]
    pub const fn sandbox_execution_id(&self) -> SandboxExecutionId {
        self.sandbox_execution_id
    }

    #[must_use]
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    #[must_use]
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    #[must_use]
    pub const fn terminal_event(&self) -> Option<&CommandHostTerminalEventV1> {
        self.terminal_event.as_ref()
    }

    #[must_use]
    pub fn cleanup_failed(&self) -> Option<&str> {
        self.cleanup_failed.as_deref()
    }

    fn accept_output_chunk(
        &mut self,
        stdout: bool,
        sequence: u64,
        bytes: &[u8],
    ) -> Result<(), CommandHostEventErrorV1> {
        if self.terminal_event.is_some() {
            return Err(CommandHostEventErrorV1::OutputAfterTerminal);
        }
        let expected =
            if stdout { &mut self.next_stdout_sequence } else { &mut self.next_stderr_sequence };
        if sequence != *expected {
            return Err(CommandHostEventErrorV1::OutputSequenceInvalid);
        }
        *expected =
            expected.checked_add(1).ok_or(CommandHostEventErrorV1::OutputSequenceInvalid)?;
        if stdout {
            self.stdout.extend_from_slice(bytes);
        } else {
            self.stderr.extend_from_slice(bytes);
        }
        Ok(())
    }

    fn set_terminal(
        &mut self,
        terminal: CommandHostTerminalEventV1,
    ) -> Result<(), CommandHostEventErrorV1> {
        if self.terminal_event.replace(terminal).is_some() {
            return Err(CommandHostEventErrorV1::TerminalConflict);
        }
        Ok(())
    }
}

fn command_event_execution_id(event: &SandboxHostEvent) -> SandboxExecutionId {
    match event {
        SandboxHostEvent::Started { execution_id }
        | SandboxHostEvent::ProcessStarted { execution_id, .. }
        | SandboxHostEvent::StdoutChunk { execution_id, .. }
        | SandboxHostEvent::StderrChunk { execution_id, .. }
        | SandboxHostEvent::Exit { execution_id, .. }
        | SandboxHostEvent::Cancelled { execution_id }
        | SandboxHostEvent::Timeout { execution_id }
        | SandboxHostEvent::PolicyViolation { execution_id, .. }
        | SandboxHostEvent::CleanupCompleted { execution_id }
        | SandboxHostEvent::CleanupFailed { execution_id, .. } => *execution_id,
    }
}

pub struct CommandRuntimeContextV1<'a> {
    pub package_root: &'a Path,
    pub workspace_root: &'a Path,
    pub resolved_directory_claims: BTreeMap<PackageResourceClaimKeyV1, PathBuf>,
    pub instance_configuration: &'a Value,
    pub system_root: &'a str,
    pub windows_directory: &'a str,
    pub trusted_powershell_executable: Option<&'a Path>,
}

pub fn prepare_tool_sandbox_runtime_v1(
    runtime: &ToolRuntimeDescriptorV1,
    arguments: &Value,
    context: &CommandRuntimeContextV1<'_>,
) -> Result<PreparedCommandLaunchV1, PreparedCommandErrorV1> {
    match runtime {
        ToolRuntimeDescriptorV1::DeclarativeCommand { command } => {
            prepare_declarative_command_v1(command, arguments, context)
        }
        ToolRuntimeDescriptorV1::AuraStdio {
            executable,
            arguments: mapping,
            working_directory,
            environment,
            credential_slot,
        } => {
            validate_command_environment_v1(environment, credential_slot.as_ref())
                .map_err(|_| PreparedCommandErrorV1::ContractInvalid)?;
            Ok(PreparedCommandLaunchV1 {
                runtime_kind: PreparedCommandRuntimeKindV1::AuraStdio,
                executable: package_path(context.package_root, executable),
                arguments: render_command_arguments_v1(arguments, mapping)?,
                working_directory: resolve_working_directory(working_directory, context)?,
                environment: prepare_environment(environment, arguments, context)?,
                credential_slot: credential_slot.clone(),
                success_exit_codes: Vec::new(),
                stdio_protocol_version: Some(STDIO_PROTOCOL_VERSION_V1),
            })
        }
        ToolRuntimeDescriptorV1::DeclarativeHttp { .. }
        | ToolRuntimeDescriptorV1::ExternalSource { .. } => {
            Err(PreparedCommandErrorV1::UnsupportedRuntime)
        }
    }
}

pub fn prepare_declarative_command_v1(
    command: &DeclarativeCommandV1,
    arguments: &Value,
    context: &CommandRuntimeContextV1<'_>,
) -> Result<PreparedCommandLaunchV1, PreparedCommandErrorV1> {
    command.validate().map_err(|_| PreparedCommandErrorV1::ContractInvalid)?;
    match command {
        DeclarativeCommandV1::Argv {
            executable,
            arguments: mapping,
            working_directory,
            environment,
            credential_slot,
            success_exit_codes,
        } => Ok(PreparedCommandLaunchV1 {
            runtime_kind: PreparedCommandRuntimeKindV1::Argv,
            executable: package_path(context.package_root, executable),
            arguments: render_command_arguments_v1(arguments, mapping)?,
            working_directory: resolve_working_directory(working_directory, context)?,
            environment: prepare_environment(environment, arguments, context)?,
            credential_slot: credential_slot.clone(),
            success_exit_codes: success_exit_codes.clone(),
            stdio_protocol_version: None,
        }),
        DeclarativeCommandV1::PowershellScript {
            script,
            arguments: mapping,
            working_directory,
            environment,
            credential_slot,
            success_exit_codes,
        } => {
            let powershell = context
                .trusted_powershell_executable
                .ok_or(PreparedCommandErrorV1::PowershellUnavailable)?;
            let script = package_path(context.package_root, script);
            let mut rendered = vec![
                "-NoProfile".to_owned(),
                "-NonInteractive".to_owned(),
                "-File".to_owned(),
                script.to_string_lossy().into_owned(),
            ];
            rendered.extend(render_command_arguments_v1(arguments, mapping)?);
            Ok(PreparedCommandLaunchV1 {
                runtime_kind: PreparedCommandRuntimeKindV1::PowershellScript,
                executable: powershell.to_path_buf(),
                arguments: rendered,
                working_directory: resolve_working_directory(working_directory, context)?,
                environment: prepare_environment(environment, arguments, context)?,
                credential_slot: credential_slot.clone(),
                success_exit_codes: success_exit_codes.clone(),
                stdio_protocol_version: None,
            })
        }
    }
}

fn prepare_environment(
    declared: &[CommandEnvironmentEntryV1],
    arguments: &Value,
    context: &CommandRuntimeContextV1<'_>,
) -> Result<Vec<PreparedEnvironmentEntryV1>, PreparedCommandErrorV1> {
    if context.system_root.is_empty()
        || context.windows_directory.is_empty()
        || context.system_root.contains('\0')
        || context.windows_directory.contains('\0')
    {
        return Err(PreparedCommandErrorV1::CoreEnvironmentInvalid);
    }
    let mut values = BTreeMap::<String, PreparedEnvironmentEntryV1>::new();
    for (name, value) in [
        ("SYSTEMROOT", context.system_root.to_owned()),
        ("WINDIR", context.windows_directory.to_owned()),
    ] {
        values.insert(name.to_owned(), PreparedEnvironmentEntryV1 { name: name.to_owned(), value });
    }
    for entry in declared {
        let value = match &entry.value {
            CommandEnvironmentValueV1::Literal { value } => value.as_str().to_owned(),
            CommandEnvironmentValueV1::Scalar { source } => {
                required_scalar_text(arguments, source)?
            }
            CommandEnvironmentValueV1::InstanceConfiguration { key } => required_scalar_text(
                context.instance_configuration,
                &format!("/{}", escape_json_pointer_segment(key.as_str()))
                    .parse()
                    .map_err(|_| PreparedCommandErrorV1::ContractInvalid)?,
            )?,
        };
        insert_environment(&mut values, &entry.name, value)?;
    }
    Ok(values.into_values().collect())
}

fn insert_environment(
    values: &mut BTreeMap<String, PreparedEnvironmentEntryV1>,
    name: &EnvironmentVariableNameV1,
    value: String,
) -> Result<(), PreparedCommandErrorV1> {
    let key = name.as_str().to_ascii_uppercase();
    if values.contains_key(&key) {
        return Err(PreparedCommandErrorV1::ContractInvalid);
    }
    values.insert(key, PreparedEnvironmentEntryV1 { name: name.as_str().to_owned(), value });
    Ok(())
}

fn resolve_working_directory(
    working_directory: &CommandWorkingDirectoryV1,
    context: &CommandRuntimeContextV1<'_>,
) -> Result<PathBuf, PreparedCommandErrorV1> {
    match working_directory {
        CommandWorkingDirectoryV1::PackageRoot => Ok(context.package_root.to_path_buf()),
        CommandWorkingDirectoryV1::WorkspaceRoot => Ok(context.workspace_root.to_path_buf()),
        CommandWorkingDirectoryV1::ResolvedDirectoryClaim { claim_key } => context
            .resolved_directory_claims
            .get(claim_key)
            .cloned()
            .ok_or(PreparedCommandErrorV1::WorkingDirectoryUnavailable),
    }
}

fn package_path(root: &Path, relative: &PackageRelativePath) -> PathBuf {
    relative.as_str().split('/').fold(root.to_path_buf(), |path, segment| path.join(segment))
}

fn escape_json_pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use aura_tool_contract::{
        CommandArgumentV1, CommandCredentialEnvironmentSlotV1, CommandEnvironmentValueV1,
        CommandFlagV1, CommandLiteralV1, CredentialSlotKey, EnvironmentLiteralValueV1,
        EnvironmentVariableNameV1, InstanceConfigurationKeyV1,
    };
    use serde_json::json;

    use super::*;

    fn context<'a>(
        root: &'a Path,
        workspace: &'a Path,
        instance: &'a Value,
        powershell: Option<&'a Path>,
    ) -> CommandRuntimeContextV1<'a> {
        CommandRuntimeContextV1 {
            package_root: root,
            workspace_root: workspace,
            resolved_directory_claims: BTreeMap::new(),
            instance_configuration: instance,
            system_root: r"C:\Windows",
            windows_directory: r"C:\Windows",
            trusted_powershell_executable: powershell,
        }
    }

    #[test]
    fn argv_plan_contains_only_pre_authorization_non_secret_environment() {
        let command = DeclarativeCommandV1::Argv {
            executable: PackageRelativePath::new("bin/tool.exe").expect("path"),
            arguments: vec![
                CommandArgumentV1::Literal {
                    value: CommandLiteralV1::new("--query").expect("literal"),
                },
                CommandArgumentV1::Scalar { source: "/query".parse().expect("pointer") },
                CommandArgumentV1::ConditionalFlag {
                    source: "/verbose".parse().expect("pointer"),
                    flag: CommandFlagV1::new("--verbose").expect("flag"),
                },
            ],
            working_directory: CommandWorkingDirectoryV1::PackageRoot,
            environment: vec![CommandEnvironmentEntryV1 {
                name: EnvironmentVariableNameV1::new("REGION").expect("name"),
                value: CommandEnvironmentValueV1::InstanceConfiguration {
                    key: InstanceConfigurationKeyV1::new("region").expect("key"),
                },
            }],
            credential_slot: Some(CommandCredentialEnvironmentSlotV1 {
                slot_key: CredentialSlotKey::new("api-key").expect("slot"),
                environment_variable: EnvironmentVariableNameV1::new("TOOL_API_KEY").expect("name"),
            }),
            success_exit_codes: vec![0],
        };
        let plan = prepare_declarative_command_v1(
            &command,
            &json!({"query": "hello", "verbose": true}),
            &context(
                Path::new(r"C:\Store\fixture"),
                Path::new(r"C:\Workspace"),
                &json!({"region": "local"}),
                None,
            ),
        )
        .expect("prepare");
        assert_eq!(plan.arguments, ["--query", "hello", "--verbose"]);
        assert_eq!(plan.environment.len(), 3);
        let debug = format!("{:?}", plan.environment);
        assert!(!debug.contains("canary-secret"));
        assert!(
            plan.environment.iter().all(|entry| !matches!(
                entry.name.as_str(),
                "PATH" | "TEMP" | "TMP" | "TOOL_API_KEY"
            ))
        );
        assert_eq!(
            plan.credential_slot.as_ref().map(|slot| slot.slot_key.as_str()),
            Some("api-key")
        );
        assert_eq!(plan.stdio_protocol_version, None);
    }

    #[test]
    fn powershell_is_fixed_to_no_profile_noninteractive_file() {
        let command = DeclarativeCommandV1::PowershellScript {
            script: PackageRelativePath::new("run.ps1").expect("script"),
            arguments: vec![],
            working_directory: CommandWorkingDirectoryV1::WorkspaceRoot,
            environment: vec![],
            credential_slot: None,
            success_exit_codes: vec![0],
        };
        let plan = prepare_declarative_command_v1(
            &command,
            &json!({}),
            &context(
                Path::new(r"C:\Store\fixture"),
                Path::new(r"C:\Workspace"),
                &json!({}),
                Some(Path::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")),
            ),
        )
        .expect("prepare");
        assert_eq!(&plan.arguments[..3], ["-NoProfile", "-NonInteractive", "-File"]);
        assert_eq!(plan.working_directory, PathBuf::from(r"C:\Workspace"));
    }

    #[test]
    fn aura_stdio_uses_duplex_protocol_without_materializing_attempt_values() {
        let runtime = ToolRuntimeDescriptorV1::AuraStdio {
            executable: PackageRelativePath::new("tool.exe").expect("path"),
            arguments: vec![],
            working_directory: CommandWorkingDirectoryV1::PackageRoot,
            environment: vec![CommandEnvironmentEntryV1 {
                name: EnvironmentVariableNameV1::new("VALUE").expect("name"),
                value: CommandEnvironmentValueV1::Literal {
                    value: EnvironmentLiteralValueV1::new("fixed").expect("value"),
                },
            }],
            credential_slot: Some(CommandCredentialEnvironmentSlotV1 {
                slot_key: CredentialSlotKey::new("required").expect("slot"),
                environment_variable: EnvironmentVariableNameV1::new("TOKEN").expect("name"),
            }),
        };
        let plan = prepare_tool_sandbox_runtime_v1(
            &runtime,
            &json!({}),
            &context(Path::new("package"), Path::new("workspace"), &json!({}), None),
        )
        .expect("prepare Aura Stdio");
        assert_eq!(plan.runtime_kind, PreparedCommandRuntimeKindV1::AuraStdio);
        assert_eq!(plan.stdio_protocol_version, Some(STDIO_PROTOCOL_VERSION_V1));
        assert!(plan.success_exit_codes.is_empty());
        assert!(plan.environment.iter().all(|entry| entry.name != "TOKEN"));
        assert!(plan.environment.iter().all(|entry| {
            !entry.name.as_str().eq_ignore_ascii_case("TEMP")
                && !entry.name.as_str().eq_ignore_ascii_case("TMP")
        }));
    }

    #[test]
    fn w7_host_adapter_command_collects_independent_streams_and_terminal() {
        let execution_id = SandboxExecutionId::new();
        let mut tracker = CommandSandboxHostTrackerV1::new(execution_id);
        tracker
            .accept_host_event(SandboxHostEvent::StdoutChunk {
                execution_id,
                sequence: 0,
                bytes: b"out-1".to_vec(),
            })
            .expect("stdout chunk");
        tracker
            .accept_host_event(SandboxHostEvent::StderrChunk {
                execution_id,
                sequence: 0,
                bytes: b"err".to_vec(),
            })
            .expect("stderr chunk");
        tracker
            .accept_host_event(SandboxHostEvent::StdoutChunk {
                execution_id,
                sequence: 1,
                bytes: b"-out-2".to_vec(),
            })
            .expect("second stdout chunk");
        tracker
            .accept_host_event(SandboxHostEvent::Exit { execution_id, exit_code: 7 })
            .expect("exit");

        assert_eq!(tracker.stdout(), b"out-1-out-2");
        assert_eq!(tracker.stderr(), b"err");
        assert_eq!(
            tracker.terminal_event(),
            Some(&CommandHostTerminalEventV1::Exited { exit_code: 7 })
        );
    }

    #[test]
    fn w7_host_adapter_command_rejects_identity_sequence_and_post_terminal_output() {
        let execution_id = SandboxExecutionId::new();
        let mut tracker = CommandSandboxHostTrackerV1::new(execution_id);
        assert_eq!(
            tracker.accept_host_event(SandboxHostEvent::Started {
                execution_id: SandboxExecutionId::new(),
            }),
            Err(CommandHostEventErrorV1::IdentityMismatch)
        );
        assert_eq!(
            tracker.accept_host_event(SandboxHostEvent::StdoutChunk {
                execution_id,
                sequence: 1,
                bytes: Vec::new(),
            }),
            Err(CommandHostEventErrorV1::OutputSequenceInvalid)
        );
        tracker
            .accept_host_event(SandboxHostEvent::Timeout { execution_id })
            .expect("timeout terminal");
        assert_eq!(
            tracker.accept_host_event(SandboxHostEvent::StderrChunk {
                execution_id,
                sequence: 0,
                bytes: Vec::new(),
            }),
            Err(CommandHostEventErrorV1::OutputAfterTerminal)
        );
        assert_eq!(
            tracker.accept_host_event(SandboxHostEvent::Cancelled { execution_id }),
            Err(CommandHostEventErrorV1::TerminalConflict)
        );
    }
}
