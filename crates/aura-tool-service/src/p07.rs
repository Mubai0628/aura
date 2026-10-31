use aura_permission_domain::{
    NonSecretSandboxRuntimeFactsV1, NonSecretToolRuntimeFactsV1, ToolRuntimeAuthorizationErrorV1,
};
use aura_sandbox::SandboxCommandDescriptor;
use thiserror::Error;

fn validate_command_matches_runtime(
    command: &SandboxCommandDescriptor,
    runtime: &NonSecretSandboxRuntimeFactsV1,
) -> Result<(), ToolSandboxAssemblyErrorV1> {
    NonSecretToolRuntimeFactsV1::Sandbox { facts: Box::new(runtime.clone()) }.validate()?;
    let credential_environment = runtime.credential.as_ref().and_then(|credential| {
        if let aura_permission_domain::ToolCredentialPlacementV1::Environment {
            environment_variable,
        } = &credential.placement
        {
            Some(environment_variable.as_str())
        } else {
            None
        }
    });
    if command.executable != runtime.executable
        || command.args != runtime.arguments
        || command.working_directory != runtime.working_directory
        || command.environment != runtime.environment
        || command.filesystem_grants != runtime.filesystem_grants
        || !command.network_grant_ids.is_empty()
        || command.timeout_ms != runtime.timeout_ms
        || command.output_limits != runtime.output_limits
        || credential_environment.is_some_and(|name| {
            command.environment.iter().any(|entry| entry.name.eq_ignore_ascii_case(name))
        })
    {
        return Err(ToolSandboxAssemblyErrorV1::RuntimeFactsMismatch);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ToolSandboxAssemblyErrorV1 {
    #[error("the Sandbox command descriptor differs from the approved non-secret Runtime facts")]
    RuntimeFactsMismatch,
    #[error(transparent)]
    Authorization(#[from] ToolRuntimeAuthorizationErrorV1),
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{CancellationId, ContentHash, FileIdentity, NetworkGrantId};
    use aura_permission_domain::{
        CanonicalExecutableRef, ProcessOutputLimits, SandboxFilesystemAccess,
        SandboxFilesystemGrant, WindowsOsString,
    };
    use aura_security_contract::CanonicalPath;

    use super::*;

    fn runtime_and_command() -> (NonSecretSandboxRuntimeFactsV1, SandboxCommandDescriptor) {
        let executable = CanonicalExecutableRef {
            path: canonical_path(r"C:\Aura Fixture\tool.exe"),
            identity: FileIdentity {
                volume_serial_number: "fixture-volume".to_owned(),
                file_id: "fixture-file".to_owned(),
            },
            content_hash: ContentHash::sha256(b"fixture executable"),
        };
        let grant = SandboxFilesystemGrant {
            path: canonical_path(r"C:\Aura Fixture"),
            identity: None,
            access: SandboxFilesystemAccess::Read,
            recursive: true,
        };
        let output_limits =
            ProcessOutputLimits { stdout_bytes: 1_024, stderr_bytes: 1_024, combined_bytes: 2_048 };
        let runtime = NonSecretSandboxRuntimeFactsV1 {
            runtime_kind: aura_permission_domain::ToolPreparedRuntimeKindV1::DeclarativeCommandArgv,
            executable: executable.clone(),
            arguments: vec![os_string("--fixture")],
            working_directory: canonical_path(r"C:\Aura Fixture"),
            environment: vec![],
            filesystem_grants: vec![grant.clone()],
            credential: None,
            success_exit_codes: vec![0],
            stdio_protocol_version: None,
            aura_stdio_bootstrap: None,
            aura_stdio_output: None,
            timeout_ms: 5_000,
            output_limits: output_limits.clone(),
        };
        let command = SandboxCommandDescriptor {
            executable,
            args: vec![os_string("--fixture")],
            working_directory: canonical_path(r"C:\Aura Fixture"),
            environment: vec![],
            filesystem_grants: vec![grant],
            network_grant_ids: vec![],
            timeout_ms: 5_000,
            output_limits,
            cancellation_id: CancellationId::new(),
        };
        (runtime, command)
    }

    fn canonical_path(value: &str) -> CanonicalPath {
        CanonicalPath { display: value.to_owned(), utf16: value.encode_utf16().collect() }
    }

    fn os_string(value: &str) -> WindowsOsString {
        WindowsOsString { utf16: value.encode_utf16().collect() }
    }

    #[test]
    fn assembly_binds_the_complete_non_secret_launch_descriptor() {
        let (runtime, command) = runtime_and_command();
        validate_command_matches_runtime(&command, &runtime).expect("matching command");

        let mut changed = command.clone();
        changed.filesystem_grants[0].access = SandboxFilesystemAccess::ReadWrite;
        assert!(matches!(
            validate_command_matches_runtime(&changed, &runtime),
            Err(ToolSandboxAssemblyErrorV1::RuntimeFactsMismatch)
        ));

        let mut changed = command;
        changed.network_grant_ids.push(NetworkGrantId::new());
        assert!(matches!(
            validate_command_matches_runtime(&changed, &runtime),
            Err(ToolSandboxAssemblyErrorV1::RuntimeFactsMismatch)
        ));
    }
}
