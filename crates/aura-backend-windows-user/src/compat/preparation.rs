use super::{
    CompatBackend,
    service::{COMPAT_BACKEND_ID, CompatExecutionBackend},
};
use crate::{controller::ScopeFacts, file_binding};
use anyhow::{Result, ensure};
use aura_domain_common::{Revision, ToolWireVersionV1, UtcTimestamp, jcs::canonical_sha256};
use aura_execution_contract::*;
use aura_security_contract::{BackendClass, EnforcementGuarantee, ResolvedCapability};
use aura_windows_user_contract::*;
use std::path::Path;

impl CompatExecutionBackend {
    pub(super) fn descriptor_now(&self) -> BackendDescriptor {
        let readiness = match self.check.get() {
            None => BackendReadiness::Unavailable { reason: "compat.preparing".into() },
            Some(Ok(_)) => BackendReadiness::Ready,
            Some(Err(reason)) => BackendReadiness::Unavailable { reason },
        };
        BackendDescriptor {
            id: BackendId(COMPAT_BACKEND_ID.into()),
            revision: Revision::INITIAL,
            runtime_instance_id: self.runtime,
            class: BackendClass::Native,
            capabilities: [ResolvedCapability::ProcessExecute, ResolvedCapability::OrdinaryNetwork]
                .into(),
            guarantees: [
                EnforcementGuarantee::ChildProcessControl,
                EnforcementGuarantee::BoundedOutput,
            ]
            .into(),
            compatibility: [
                ExecutionCompatibility::ProcessTreeCancellation,
                ExecutionCompatibility::LongRunningProcess,
                ExecutionCompatibility::NativeWindowsToolchain,
            ]
            .into(),
            known_limitations: Default::default(),
            readiness,
            protocol_fingerprint: canonical_sha256(&(COMPAT_BACKEND_ID, "compat-managed-shell-v1"))
                .expect("literal compat protocol"),
        }
    }
    pub(super) fn preparation(
        &self,
        engine: &CompatBackend,
        operation: &SecuredOperation,
        facts: ScopeFacts,
        valid_until: UtcTimestamp,
    ) -> Result<WindowsUserPreparation> {
        let command: ManagedShellCommand =
            serde_json::from_value(operation.content.action.clone())?;
        command.validate()?;
        ensure!(
            aura_security_contract::built_in_shell(
                &operation.content.source,
                &operation.content.action_kind
            ),
            "compat.builtin_shell_required"
        );
        ensure!(
            operation.content.action_kind
                == match command.shell {
                    ManagedShell::PowerShell => "shell.powershell",
                    ManagedShell::Cmd => "shell.cmd",
                },
            "compat.shell_changed"
        );
        ensure!(
            operation.content.credentials.is_empty(),
            "compat.credential_binding_not_supported"
        );
        ensure!(
            matches!(
                operation.content.network,
                OperationNetwork::None | OperationNetwork::BroadProcess
            ),
            "compat.network_declaration"
        );
        let image = match command.shell {
            ManagedShell::PowerShell => engine.powershell_path(),
            ManagedShell::Cmd => engine.cmd_path(),
        };
        let working_directory =
            file_binding::capture(Path::new(&command.working_directory), false)?;
        ensure!(working_directory.1.info().is_directory, "compat.directory_required");
        // This is normal CWD resolution; it does not claim directory or read-only confinement.
        ensure!(
            facts.read_roots.iter().any(|root| file_binding::verify(root).is_ok_and(|root| {
                aura_windows_platform::final_path_within(root.info(), working_directory.1.info())
            })),
            "compat.working_directory_changed"
        );
        Ok(WindowsUserPreparation {
            schema_version: ToolWireVersionV1,
            assurance_version: ASSURANCE_VERSION.into(),
            shell_environment_version: SHELL_ENVIRONMENT_VERSION.into(),
            installation_fingerprint: canonical_sha256(&(COMPAT_BACKEND_ID, image))?,
            mode: facts.mode,
            permission_revision: facts.revision,
            valid_until,
            account_sid: aura_windows_platform::current_user_token_identity()?.0,
            shell_image: file_binding::capture(image, true)?.0,
            working_directory: working_directory.0,
            read_roots: facts.read_roots,
            write_roots: facts.write_roots,
        })
    }
}
