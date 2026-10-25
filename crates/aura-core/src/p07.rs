use std::{os::windows::ffi::OsStrExt, path::Path};

use aura_domain_common::ToolExecutionAttemptId;
use aura_domain_common::jcs::canonical_sha256;
use aura_domain_common::{ContentHash, ContentHashAlgorithm, SecurityOperationId, UtcTimestamp};
use aura_permission_domain::{
    AuthorizedToolSandboxExecutionV1, NativeSandboxMode, NetworkAccessRequirement,
    NonSecretSandboxRuntimeFactsV1, NonSecretToolRuntimeFactsV1, OperationSecurityPlan,
    PermissionDecision, PermissionGrant, SandboxExecutionPlan, SandboxFilesystemAccess,
    SandboxFilesystemGrant, ToolRuntimeAuthorizationErrorV1, ToolSandboxDispatchBindingV1,
    TurnDenyRecord, W7ToolOperationFingerprintPreimageV1,
};
use aura_permission_engine::{DefaultPermissionEvaluator, PermissionEvaluation};
use aura_persistence::ExactToolExecutionAuthorityV1;
use aura_sandbox::{
    DefaultSandboxPlanner, InMemorySandboxOperationResolver, SandboxCommandDescriptor,
    SandboxError, SandboxPlanner, plan_hash,
};
use aura_security_contract::TrustState;
use aura_security_contract::{
    InvocationIdentity, PermissionAllowAuthorizationBasisV1, PermissionSnapshot,
};
use aura_tool_contract::{
    CoreInvocationIdentityV1, NormalizedToolCallInputStateV1, ToolRuntimeOutputAuthorizationV1,
    ToolSourceKindV1,
};
use aura_tool_scheduler::{
    AdmittedToolBatchV1, PlannedToolBatchV1, PlannedToolRuntimeCallV1,
    security_source_for_admitted_tool_call_v1,
};
use thiserror::Error;

/// Inputs already frozen by Batch Admission, execution planning, Registry
/// authority loading, and P04 session-state creation. No Runtime attempt exists
/// while this value is evaluated.
pub(crate) struct ToolSandboxAssemblyInputV1<'a> {
    pub admitted_batch: &'a AdmittedToolBatchV1,
    pub planned_batch: &'a PlannedToolBatchV1,
    pub planned_call: &'a PlannedToolRuntimeCallV1,
    pub authority: &'a ExactToolExecutionAuthorityV1,
    pub operation_id: SecurityOperationId,
    pub operation_identity: InvocationIdentity,
    pub runtime_facts: NonSecretSandboxRuntimeFactsV1,
    pub command: SandboxCommandDescriptor,
    pub selected_mode: NativeSandboxMode,
    pub network_access: NetworkAccessRequirement,
    pub permission_snapshot: PermissionSnapshot,
    pub trust_state: &'a TrustState,
    pub permission_grants: &'a [PermissionGrant],
    pub turn_denies: &'a [TurnDenyRecord],
    pub available_sandbox_modes: &'a [NativeSandboxMode],
    pub network_control_available: bool,
    pub allow_unrestricted_inline_scripts_in_full_access: bool,
    pub evaluated_at: UtcTimestamp,
}

pub(crate) struct ToolSandboxAssemblyEvaluationV1 {
    pub decision: PermissionDecision,
    pub authorized_operation: Option<AuthorizedToolSandboxOperationV1>,
}

/// Opaque Core-owned result of one exact P04 Allow. Callers cannot substitute
/// another Descriptor, source, decision, or Registry binding during
/// materialization.
pub(crate) struct AuthorizedToolSandboxOperationV1 {
    operation_id: SecurityOperationId,
    operation: W7ToolOperationFingerprintPreimageV1,
    permission_snapshot: PermissionSnapshot,
    authorization_basis: PermissionAllowAuthorizationBasisV1,
    base_plan: SandboxExecutionPlan,
}

impl AuthorizedToolSandboxOperationV1 {
    #[cfg(any(test, feature = "p08-contract-driver"))]
    pub(crate) fn operation_scopes(&self) -> &[aura_permission_domain::ResolvedScope] {
        &self.operation.scopes
    }
}

#[derive(Clone, Default)]
pub(crate) struct ToolSandboxAssemblyServiceV1 {
    resolver: InMemorySandboxOperationResolver,
}

impl ToolSandboxAssemblyServiceV1 {
    pub fn evaluate(
        &self,
        input: ToolSandboxAssemblyInputV1<'_>,
    ) -> Result<ToolSandboxAssemblyEvaluationV1, ToolSandboxAssemblyErrorV1> {
        validate_authority_chain(&input)?;
        validate_command_matches_runtime(&input.command, &input.runtime_facts)?;

        let source = security_source_for_admitted_tool_call_v1(
            &input.planned_call.validated_call,
            &input.authority.registry,
        )
        .map_err(|_| ToolSandboxAssemblyErrorV1::AuthorityMismatch)?;
        let aura_permission_domain::SecuritySource::Tool { tool } = &source else {
            return Err(ToolSandboxAssemblyErrorV1::AuthorityMismatch);
        };
        let source_security_fingerprint = source_security_fingerprint(input.authority, tool)?;
        validate_trust_binding(tool.source_kind, &source_security_fingerprint, input.trust_state)?;

        let NormalizedToolCallInputStateV1::Ready { purpose, arguments, arguments_hash } =
            &input.planned_call.validated_call.call.input
        else {
            return Err(ToolSandboxAssemblyErrorV1::InputNotReady);
        };
        let operation = W7ToolOperationFingerprintPreimageV1::new(
            input.admitted_batch.batch.invocation_identity.clone(),
            input.admitted_batch.batch.tool_batch_id,
            input.planned_call.validated_call.call.tool_call_id,
            (**tool).clone(),
            source_security_fingerprint.clone(),
            input.planned_batch.execution_plan.execution_plan_hash.clone(),
            input.authority.definition.capabilities.clone(),
            input.planned_call.permission_scopes.clone(),
            input.planned_call.risk_flags.clone(),
            purpose.clone(),
            arguments.clone(),
            arguments_hash.clone(),
            NonSecretToolRuntimeFactsV1::Sandbox { facts: Box::new(input.runtime_facts.clone()) },
            Some(input.selected_mode),
            input.network_access,
        )?;

        let mut operation_identity = input.operation_identity.clone();
        operation_identity.permission_snapshot_id = Some(input.permission_snapshot.id);
        let security_plan = OperationSecurityPlan {
            operation_id: input.operation_id,
            identity: operation_identity,
            source,
            capabilities: operation.capabilities.clone(),
            scopes: operation.scopes.clone(),
            risk_flags: operation.risks.clone(),
            purpose: Some(operation.purpose.as_str().to_owned()),
            source_fingerprint: Some(source_security_fingerprint),
            requested_sandbox: Some(input.selected_mode),
            network_access: input.network_access,
            network_plan: None,
            operation_fingerprint: operation.fingerprint()?.as_content_hash(),
        };
        let evaluated =
            DefaultPermissionEvaluator.evaluate_with_authorization_basis(&PermissionEvaluation {
                plan: &security_plan,
                snapshot: input.permission_snapshot.clone(),
                source_enabled: input.authority.instance.enabled,
                trust_state: input.trust_state,
                grants: input.permission_grants,
                turn_denies: input.turn_denies,
                available_sandbox_modes: input.available_sandbox_modes,
                network_control_available: input.network_control_available,
                allow_unrestricted_inline_scripts_in_full_access: input
                    .allow_unrestricted_inline_scripts_in_full_access,
                now: input.evaluated_at,
            });

        let PermissionDecision::Allow { snapshot } = &evaluated.decision else {
            return Ok(ToolSandboxAssemblyEvaluationV1 {
                decision: evaluated.decision,
                authorized_operation: None,
            });
        };
        let permission_snapshot = snapshot.clone();
        let authorization_basis = evaluated
            .allow_authorization_basis
            .ok_or(ToolSandboxAssemblyErrorV1::AuthorizationBasisMissing)?;

        // Registration happens only after the exact operation facts have been
        // evaluated. Re-registering identical facts is idempotent; replacement
        // for the same operation_id is rejected by the resolver.
        self.resolver.register(input.operation_id, input.command)?;
        let base_plan = DefaultSandboxPlanner::new(self.resolver.clone())
            .plan(&security_plan, &evaluated.decision)?;
        validate_base_plan(&base_plan, &operation, &permission_snapshot)?;

        Ok(ToolSandboxAssemblyEvaluationV1 {
            decision: evaluated.decision,
            authorized_operation: Some(AuthorizedToolSandboxOperationV1 {
                operation_id: input.operation_id,
                operation,
                permission_snapshot,
                authorization_basis,
                base_plan,
            }),
        })
    }
}

impl AuthorizedToolSandboxOperationV1 {
    /// Binds the pre-approved non-secret launch to a preallocated `tea_` and
    /// attempt-local TEMP/TMP path. The directory itself is created only after
    /// the caller atomically commits this binding with Started and
    /// DispatchPrepared.
    pub fn materialize_dispatch(
        self,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        attempt_temp_directory: &Path,
        prepared_at: UtcTimestamp,
    ) -> Result<AuthorizedToolSandboxExecutionV1, ToolSandboxAssemblyErrorV1> {
        let mut plan = self.base_plan;
        if plan
            .environment
            .iter()
            .any(|entry| aura_permission_domain::is_attempt_local_environment_name(&entry.name))
        {
            return Err(ToolSandboxAssemblyErrorV1::RuntimeFactsMismatch);
        }
        // One binding, shared with the conformance adapter: TEMP/TMP keep the Tool
        // runtime's scratch inside this attempt's directory, and LOCALAPPDATA is read
        // by the AppContainer launcher from the block it is handed. Never the
        // launching user's profile.
        let attempt_directory = attempt_temp_directory.to_string_lossy();
        plan.environment
            .extend(aura_permission_domain::attempt_local_environment(&attempt_directory));
        let stdio = matches!(
            &self.operation.runtime,
            NonSecretToolRuntimeFactsV1::Sandbox { facts }
                if facts.runtime_kind == aura_permission_domain::ToolPreparedRuntimeKindV1::AuraStdio
        );
        let attempt_output_root =
            stdio.then(|| canonical_path(&attempt_temp_directory.join("output")));
        let runtime_output_authorization = if let Some(output_root) = &attempt_output_root {
            if plan.filesystem_grants.iter().any(|grant| grant.path == *output_root) {
                return Err(ToolSandboxAssemblyErrorV1::RuntimeFactsMismatch);
            }
            plan.filesystem_grants.push(SandboxFilesystemGrant {
                path: output_root.clone(),
                identity: None,
                access: SandboxFilesystemAccess::ReadWrite,
                recursive: true,
            });
            Some(ToolRuntimeOutputAuthorizationV1::new(
                self.operation.tool_call_id,
                tool_execution_attempt_id,
            ))
        } else {
            None
        };
        plan.plan_hash = plan_hash(&plan)?;
        let binding = ToolSandboxDispatchBindingV1::new(
            self.operation_id,
            plan.identity.clone(),
            self.operation.fingerprint()?,
            self.permission_snapshot,
            self.authorization_basis,
            self.operation.tool_batch_id,
            self.operation.tool_call_id,
            tool_execution_attempt_id,
            plan.execution_id,
            self.operation.execution_plan_hash.clone(),
            self.operation.runtime_binding_hash.clone(),
            self.operation.source.clone(),
            self.operation.source_security_fingerprint.clone(),
            plan.mode,
            plan.network_access,
            plan.network_enforcement,
            plan.plan_hash.clone(),
            runtime_output_authorization,
            attempt_output_root,
            prepared_at,
        )?;
        AuthorizedToolSandboxExecutionV1::new(self.operation, binding, plan)
            .map_err(ToolSandboxAssemblyErrorV1::from)
    }
}

fn canonical_path(path: &Path) -> aura_security_contract::CanonicalPath {
    aura_security_contract::CanonicalPath {
        display: path.display().to_string(),
        utf16: path.as_os_str().encode_wide().collect(),
    }
}

fn validate_authority_chain(
    input: &ToolSandboxAssemblyInputV1<'_>,
) -> Result<(), ToolSandboxAssemblyErrorV1> {
    validate_common_authority_chain(
        input.admitted_batch,
        input.planned_batch,
        input.planned_call,
        input.authority,
        &input.operation_identity,
        &input.permission_snapshot,
        input.runtime_facts.timeout_ms,
        true,
    )
}

// Keep every exact admission, plan, authority, permission, and Runtime fact visible at
// this shared fail-closed validation boundary.
#[allow(clippy::too_many_arguments)]
pub(super) fn validate_common_authority_chain(
    admitted_batch: &AdmittedToolBatchV1,
    planned_batch: &PlannedToolBatchV1,
    planned_call: &PlannedToolRuntimeCallV1,
    authority: &ExactToolExecutionAuthorityV1,
    operation_identity: &InvocationIdentity,
    permission_snapshot: &PermissionSnapshot,
    timeout_ms: u64,
    requires_package_runtime_scope: bool,
) -> Result<(), ToolSandboxAssemblyErrorV1> {
    admitted_batch
        .batch
        .validate_hash()
        .map_err(|_| ToolSandboxAssemblyErrorV1::AuthorityMismatch)?;
    planned_batch
        .execution_plan
        .validate()
        .map_err(|_| ToolSandboxAssemblyErrorV1::AuthorityMismatch)?;
    let call = &planned_call.validated_call;
    let invocation = &admitted_batch.batch.invocation_identity;
    if planned_batch.execution_plan.tool_batch_id != admitted_batch.batch.tool_batch_id
        || planned_batch.execution_plan.registry_snapshot_ref
            != admitted_batch.batch.registry_snapshot_ref
        || call
            .call
            .definition_ref
            .as_ref()
            .ok_or(ToolSandboxAssemblyErrorV1::AuthorityMismatch)?
            .registry_snapshot_ref
            != admitted_batch.batch.registry_snapshot_ref
        || !admitted_batch.validated_calls.contains(call)
        || !planned_batch.execution_plan.calls.iter().any(|planned| {
            planned.tool_call_id == call.call.tool_call_id
                && planned.timeout_ms == timeout_ms
                && planned.entity_revision == authority.instance.entity_revision
        })
        || authority.registry.snapshot_ref != admitted_batch.batch.registry_snapshot_ref
        || authority.definition != call.definition
        || authority.instance.tool_instance_id != call.binding.tool_instance_id
        || authority.frozen.registry_snapshot_ref != admitted_batch.batch.registry_snapshot_ref
        || authority.frozen.tool_id != call.definition.tool_id
        || authority.frozen.tool_version != call.definition.tool_version
        || authority.frozen.tool_instance_id != call.binding.tool_instance_id
        || !identity_matches_invocation(operation_identity, invocation)
        || operation_identity.workspace_id != Some(permission_snapshot.workspace_id)
        || permission_snapshot.session_id != invocation.session_id
    {
        return Err(ToolSandboxAssemblyErrorV1::AuthorityMismatch);
    }
    if requires_package_runtime_scope && authority.package.is_some() {
        let package_scope = authority
            .package_runtime_scope_for_admitted_call(call)
            .map_err(|_| ToolSandboxAssemblyErrorV1::AuthorityMismatch)?;
        if !planned_call.permission_scopes.contains(&package_scope) {
            return Err(ToolSandboxAssemblyErrorV1::AuthorityMismatch);
        }
    }
    Ok(())
}

fn identity_matches_invocation(
    identity: &InvocationIdentity,
    invocation: &CoreInvocationIdentityV1,
) -> bool {
    identity.session_id == Some(invocation.session_id)
        && identity.turn_task_id == Some(invocation.turn_task_id)
        && identity.agent_run_id == Some(invocation.agent_run_id)
}

pub(super) fn source_security_fingerprint(
    authority: &ExactToolExecutionAuthorityV1,
    source: &aura_permission_domain::ToolSecuritySourceRefV1,
) -> Result<ContentHash, ToolSandboxAssemblyErrorV1> {
    match source.source_kind {
        ToolSourceKindV1::BuiltIn => {
            let hash = canonical_sha256(&source.source_contract_fingerprint)
                .map_err(|_| ToolSandboxAssemblyErrorV1::AuthorityMismatch)?;
            ContentHash::new(ContentHashAlgorithm::Sha256, hash.hex())
                .map_err(|_| ToolSandboxAssemblyErrorV1::AuthorityMismatch)
        }
        ToolSourceKindV1::UserCreated
        | ToolSourceKindV1::ModelCreated
        | ToolSourceKindV1::Imported => {
            let package =
                authority.package.as_ref().ok_or(ToolSandboxAssemblyErrorV1::AuthorityMismatch)?;
            ContentHash::new(
                ContentHashAlgorithm::Blake3,
                package.package_security_fingerprint().0.hex(),
            )
            .map_err(|_| ToolSandboxAssemblyErrorV1::AuthorityMismatch)
        }
        // MCP/Plugin keep their existing adapter-owned process routes until
        // their typed non-Tool lifecycle references and execution evidence are
        // implemented by P11/P14.
        ToolSourceKindV1::Mcp | ToolSourceKindV1::Plugin => {
            Err(ToolSandboxAssemblyErrorV1::UnsupportedSource)
        }
    }
}

pub(super) fn validate_trust_binding(
    source_kind: ToolSourceKindV1,
    source_fingerprint: &ContentHash,
    trust_state: &TrustState,
) -> Result<(), ToolSandboxAssemblyErrorV1> {
    match (source_kind, trust_state) {
        (ToolSourceKindV1::BuiltIn, TrustState::SystemTrusted) => Ok(()),
        (
            ToolSourceKindV1::UserCreated
            | ToolSourceKindV1::ModelCreated
            | ToolSourceKindV1::Imported,
            TrustState::Trusted { fingerprint, .. },
        ) if fingerprint == source_fingerprint => Ok(()),
        (_, TrustState::Untrusted | TrustState::Revoked) => Ok(()),
        _ => Err(ToolSandboxAssemblyErrorV1::TrustFingerprintMismatch),
    }
}

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

fn validate_base_plan(
    plan: &SandboxExecutionPlan,
    operation: &W7ToolOperationFingerprintPreimageV1,
    snapshot: &PermissionSnapshot,
) -> Result<(), ToolSandboxAssemblyErrorV1> {
    if plan.permission_snapshot_id != snapshot.id
        || plan.mode != operation.sandbox_mode.ok_or(ToolSandboxAssemblyErrorV1::ModeMismatch)?
        || plan.network_access != operation.network_access
        || plan.plan_hash != plan_hash(plan)?
    {
        return Err(ToolSandboxAssemblyErrorV1::ModeMismatch);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub(crate) enum ToolSandboxAssemblyErrorV1 {
    #[error("the admitted call, execution plan, Registry authority, or P04 identity differs")]
    AuthorityMismatch,
    #[error("the normalized Tool input is not ready for execution")]
    InputNotReady,
    #[error("the selected Tool source is not dispatched by the W7 Package runtime adapter")]
    UnsupportedSource,
    #[error("the P04 trust state does not bind the exact Tool source fingerprint")]
    TrustFingerprintMismatch,
    #[error("the Sandbox command descriptor differs from the approved non-secret Runtime facts")]
    RuntimeFactsMismatch,
    #[error("P04 returned Allow without an exact authorization basis")]
    AuthorizationBasisMissing,
    #[error("the selected Sandbox mode or network enforcement changed")]
    ModeMismatch,
    #[error(transparent)]
    Authorization(#[from] ToolRuntimeAuthorizationErrorV1),
    #[error(transparent)]
    Sandbox(#[from] SandboxError),
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
