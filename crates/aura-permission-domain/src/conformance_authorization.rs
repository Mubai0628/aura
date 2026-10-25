use aura_domain_common::jcs::{CanonicalHashV1, CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{
    CommandInvocationId, ContentHash, CorrelationId, PermissionSnapshotId, SandboxExecutionId,
    SecurityOperationId, UtcTimestamp,
};
use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
use aura_tool_contract::{
    AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1, CredentialSlotKey, EnvironmentVariableNameV1,
    ToolPackageFileSha256V1,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use thiserror::Error;

use crate::{
    CanonicalExecutableRef, ConformanceSandboxProfileV1, NativeSandboxMode,
    NetworkAccessRequirement, NetworkEnforcementKind, ResolvedScope, SandboxExecutionPlan,
    SandboxFilesystemAccess, SandboxFilesystemGrant, SanitizedEnvironmentEntry,
    ToolConformanceRuntimeScopeV1, ToolConformanceSecuritySourceRefV1, WindowsOsString,
};
use aura_security_contract::{CanonicalPath, InvocationActor};

pub const TOOL_CONFORMANCE_ALLOW_ONCE_EVIDENCE_VERSION_V1: u32 = 1;
pub const TOOL_CONFORMANCE_SANDBOX_DISPATCH_BINDING_VERSION_V1: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolConformanceApprovalDecisionV1 {
    AllowOnce,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolConformanceSyntheticCredentialPolicyV1 {
    EphemeralPerRunNoPersistenceV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolConformancePreparedRuntimeKindV1 {
    DeclarativeCommandArgv,
    DeclarativeCommandPowershellScript,
    AuraStdio,
}

/// Non-secret binding for the single synthetic Credential that W3 may inject
/// into a Command/AuraStdio child. It has no CredentialId and cannot be
/// interpreted as a reusable user Credential.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolConformanceSyntheticCredentialUseV1 {
    pub slot_key: CredentialSlotKey,
    pub environment_variable: EnvironmentVariableNameV1,
    pub credential_binding_hash: CanonicalSha256HashV1,
    pub secret_sha256: ToolPackageFileSha256V1,
}

/// Exact non-secret launch facts approved for one uninstalled W3 process.
/// TEMP/TMP and the optional output root are run-local paths and are included
/// in this hash-bound value; synthetic Credential plaintext is never present.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NonSecretToolConformanceSandboxRuntimeFactsV1 {
    pub runtime_kind: ToolConformancePreparedRuntimeKindV1,
    pub tool_arguments: Value,
    pub tool_arguments_hash: CanonicalHashV1,
    pub executable: CanonicalExecutableRef,
    pub arguments: Vec<WindowsOsString>,
    pub working_directory: CanonicalPath,
    pub environment: Vec<SanitizedEnvironmentEntry>,
    pub filesystem_grants: Vec<SandboxFilesystemGrant>,
    pub synthetic_credential: Option<ToolConformanceSyntheticCredentialUseV1>,
    pub success_exit_codes: Vec<i32>,
    pub stdio_protocol_version: Option<u32>,
    pub output_root: Option<CanonicalPath>,
    pub timeout_ms: u64,
    pub output_limits: crate::ProcessOutputLimits,
}

impl NonSecretToolConformanceSandboxRuntimeFactsV1 {
    pub fn validate(&self) -> Result<(), ToolConformanceAuthorizationErrorV1> {
        let stdio = self.runtime_kind == ToolConformancePreparedRuntimeKindV1::AuraStdio;
        if self.timeout_ms == 0
            || self.output_limits.stdout_bytes == 0
            || self.output_limits.stderr_bytes == 0
            || self.output_limits.combined_bytes == 0
            || stdio != self.stdio_protocol_version.is_some()
            || stdio != self.output_root.is_some()
            || (stdio && !self.success_exit_codes.is_empty())
            || (!stdio && self.success_exit_codes.is_empty())
            || has_duplicate_environment(&self.environment)
            || has_forbidden_runtime_environment(
                &self.environment,
                self.synthetic_credential.as_ref(),
            )
            || self.filesystem_grants.is_empty()
            || canonical_sha256(&self.tool_arguments).ok() != Some(self.tool_arguments_hash.clone())
            || self.output_root.as_ref().is_some_and(|root| {
                !self.filesystem_grants.iter().any(|grant| {
                    grant.path == *root
                        && grant.access == SandboxFilesystemAccess::ReadWrite
                        && grant.recursive
                })
            })
        {
            return Err(ToolConformanceAuthorizationErrorV1::RuntimeFactsInvalid);
        }
        Ok(())
    }

    pub fn canonical_hash(&self) -> Result<CanonicalHashV1, ToolConformanceAuthorizationErrorV1> {
        self.validate()?;
        canonical_sha256(self).map_err(|_| ToolConformanceAuthorizationErrorV1::Canonicalization)
    }
}

/// P04-owned proof that one authenticated settings interaction approved one
/// exact W3 run. The proof is deliberately not a PermissionGrant and cannot
/// be reused by a Workspace, Session, installed Tool, or later `tcr_`.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolConformanceAllowOnceEvidenceV1 {
    pub evidence_version: u32,
    pub decision: ToolConformanceApprovalDecisionV1,
    pub security_operation_id: SecurityOperationId,
    pub source: ToolConformanceSecuritySourceRefV1,
    pub runtime_scope: ToolConformanceRuntimeScopeV1,
    pub sandbox_profile: ConformanceSandboxProfileV1,
    pub synthetic_credential_policy: ToolConformanceSyntheticCredentialPolicyV1,
    pub synthetic_credential_bindings_hash: CanonicalSha256HashV1,
    pub runtime_binding_hash: CanonicalHashV1,
    pub sandbox_plan_hash: ContentHash,
    pub tool_call_id: ToolCallId,
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub sandbox_execution_id: SandboxExecutionId,
    pub authorization_snapshot_id: PermissionSnapshotId,
    pub authenticated_user_identity_hash: ContentHash,
    pub command_invocation_id: CommandInvocationId,
    pub correlation_id: CorrelationId,
    pub approved_at: UtcTimestamp,
    pub evidence_hash: CanonicalSha256HashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolConformanceAllowOnceEvidenceHashMaterialV1<'a> {
    evidence_version: u32,
    decision: ToolConformanceApprovalDecisionV1,
    security_operation_id: SecurityOperationId,
    source: &'a ToolConformanceSecuritySourceRefV1,
    runtime_scope: &'a ToolConformanceRuntimeScopeV1,
    sandbox_profile: ConformanceSandboxProfileV1,
    synthetic_credential_policy: ToolConformanceSyntheticCredentialPolicyV1,
    synthetic_credential_bindings_hash: &'a CanonicalSha256HashV1,
    runtime_binding_hash: &'a CanonicalHashV1,
    sandbox_plan_hash: &'a ContentHash,
    tool_call_id: ToolCallId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    sandbox_execution_id: SandboxExecutionId,
    authorization_snapshot_id: PermissionSnapshotId,
    authenticated_user_identity_hash: &'a ContentHash,
    command_invocation_id: CommandInvocationId,
    correlation_id: CorrelationId,
    approved_at: UtcTimestamp,
}

impl ToolConformanceAllowOnceEvidenceV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        security_operation_id: SecurityOperationId,
        source: ToolConformanceSecuritySourceRefV1,
        synthetic_credential_bindings_hash: CanonicalSha256HashV1,
        runtime_binding_hash: CanonicalHashV1,
        sandbox_plan_hash: ContentHash,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        sandbox_execution_id: SandboxExecutionId,
        authorization_snapshot_id: PermissionSnapshotId,
        authenticated_user_identity_hash: ContentHash,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        approved_at: UtcTimestamp,
    ) -> Result<Self, ToolConformanceAuthorizationErrorV1> {
        let runtime_scope = ToolConformanceRuntimeScopeV1 { conformance: source.clone() };
        let mut value = Self {
            evidence_version: TOOL_CONFORMANCE_ALLOW_ONCE_EVIDENCE_VERSION_V1,
            decision: ToolConformanceApprovalDecisionV1::AllowOnce,
            security_operation_id,
            source,
            runtime_scope,
            sandbox_profile: ConformanceSandboxProfileV1::HermeticNoNetworkV1,
            synthetic_credential_policy:
                ToolConformanceSyntheticCredentialPolicyV1::EphemeralPerRunNoPersistenceV1,
            synthetic_credential_bindings_hash,
            runtime_binding_hash,
            sandbox_plan_hash,
            tool_call_id,
            tool_execution_attempt_id,
            sandbox_execution_id,
            authorization_snapshot_id,
            authenticated_user_identity_hash,
            command_invocation_id,
            correlation_id,
            approved_at,
            evidence_hash: CanonicalSha256HashV1::sha256(&[]),
        };
        value.evidence_hash = value.compute_hash()?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ToolConformanceAuthorizationErrorV1> {
        if self.evidence_version != TOOL_CONFORMANCE_ALLOW_ONCE_EVIDENCE_VERSION_V1
            || self.decision != ToolConformanceApprovalDecisionV1::AllowOnce
            || self.runtime_scope.conformance != self.source
            || self.sandbox_profile != ConformanceSandboxProfileV1::HermeticNoNetworkV1
            || self.synthetic_credential_policy
                != ToolConformanceSyntheticCredentialPolicyV1::EphemeralPerRunNoPersistenceV1
            || self.evidence_hash != self.compute_hash()?
        {
            return Err(ToolConformanceAuthorizationErrorV1::AllowOnceEvidenceInvalid);
        }
        Ok(())
    }

    fn compute_hash(&self) -> Result<CanonicalSha256HashV1, ToolConformanceAuthorizationErrorV1> {
        canonical_sha256(&ToolConformanceAllowOnceEvidenceHashMaterialV1 {
            evidence_version: self.evidence_version,
            decision: self.decision,
            security_operation_id: self.security_operation_id,
            source: &self.source,
            runtime_scope: &self.runtime_scope,
            sandbox_profile: self.sandbox_profile,
            synthetic_credential_policy: self.synthetic_credential_policy,
            synthetic_credential_bindings_hash: &self.synthetic_credential_bindings_hash,
            runtime_binding_hash: &self.runtime_binding_hash,
            sandbox_plan_hash: &self.sandbox_plan_hash,
            tool_call_id: self.tool_call_id,
            tool_execution_attempt_id: self.tool_execution_attempt_id,
            sandbox_execution_id: self.sandbox_execution_id,
            authorization_snapshot_id: self.authorization_snapshot_id,
            authenticated_user_identity_hash: &self.authenticated_user_identity_hash,
            command_invocation_id: self.command_invocation_id,
            correlation_id: self.correlation_id,
            approved_at: self.approved_at,
        })
        .map_err(|_| ToolConformanceAuthorizationErrorV1::Canonicalization)
    }
}

impl<'de> Deserialize<'de> for ToolConformanceAllowOnceEvidenceV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            evidence_version: u32,
            decision: ToolConformanceApprovalDecisionV1,
            security_operation_id: SecurityOperationId,
            source: ToolConformanceSecuritySourceRefV1,
            runtime_scope: ToolConformanceRuntimeScopeV1,
            sandbox_profile: ConformanceSandboxProfileV1,
            synthetic_credential_policy: ToolConformanceSyntheticCredentialPolicyV1,
            synthetic_credential_bindings_hash: CanonicalSha256HashV1,
            runtime_binding_hash: CanonicalHashV1,
            sandbox_plan_hash: ContentHash,
            tool_call_id: ToolCallId,
            tool_execution_attempt_id: ToolExecutionAttemptId,
            sandbox_execution_id: SandboxExecutionId,
            authorization_snapshot_id: PermissionSnapshotId,
            authenticated_user_identity_hash: ContentHash,
            command_invocation_id: CommandInvocationId,
            correlation_id: CorrelationId,
            approved_at: UtcTimestamp,
            evidence_hash: CanonicalSha256HashV1,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            evidence_version: wire.evidence_version,
            decision: wire.decision,
            security_operation_id: wire.security_operation_id,
            source: wire.source,
            runtime_scope: wire.runtime_scope,
            sandbox_profile: wire.sandbox_profile,
            synthetic_credential_policy: wire.synthetic_credential_policy,
            synthetic_credential_bindings_hash: wire.synthetic_credential_bindings_hash,
            runtime_binding_hash: wire.runtime_binding_hash,
            sandbox_plan_hash: wire.sandbox_plan_hash,
            tool_call_id: wire.tool_call_id,
            tool_execution_attempt_id: wire.tool_execution_attempt_id,
            sandbox_execution_id: wire.sandbox_execution_id,
            authorization_snapshot_id: wire.authorization_snapshot_id,
            authenticated_user_identity_hash: wire.authenticated_user_identity_hash,
            command_invocation_id: wire.command_invocation_id,
            correlation_id: wire.correlation_id,
            approved_at: wire.approved_at,
            evidence_hash: wire.evidence_hash,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

/// Durable one-shot relation consumed by Sandbox Host before the W3 child may
/// start. It deliberately contains no `tbt_`, `trs_`, installed Instance, or
/// reusable PermissionGrant identity.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolConformanceSandboxDispatchBindingV1 {
    pub binding_version: u32,
    pub allow_once: ToolConformanceAllowOnceEvidenceV1,
    pub runtime_binding_hash: CanonicalHashV1,
    pub selected_mode: NativeSandboxMode,
    pub network_access: NetworkAccessRequirement,
    pub network_enforcement: NetworkEnforcementKind,
    pub sandbox_plan_hash: ContentHash,
    pub prepared_at: UtcTimestamp,
    pub binding_hash: CanonicalSha256HashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolConformanceSandboxDispatchBindingHashMaterialV1<'a> {
    binding_version: u32,
    allow_once: &'a ToolConformanceAllowOnceEvidenceV1,
    runtime_binding_hash: &'a CanonicalHashV1,
    selected_mode: NativeSandboxMode,
    network_access: NetworkAccessRequirement,
    network_enforcement: NetworkEnforcementKind,
    sandbox_plan_hash: &'a ContentHash,
    prepared_at: UtcTimestamp,
}

impl ToolConformanceSandboxDispatchBindingV1 {
    pub fn new(
        allow_once: ToolConformanceAllowOnceEvidenceV1,
        runtime_binding_hash: CanonicalHashV1,
        sandbox_plan_hash: ContentHash,
        prepared_at: UtcTimestamp,
    ) -> Result<Self, ToolConformanceAuthorizationErrorV1> {
        let mut value = Self {
            binding_version: TOOL_CONFORMANCE_SANDBOX_DISPATCH_BINDING_VERSION_V1,
            allow_once,
            runtime_binding_hash,
            selected_mode: NativeSandboxMode::StrictAppContainer,
            network_access: NetworkAccessRequirement::NotRequired,
            network_enforcement: NetworkEnforcementKind::AppContainerNetworkDenied,
            sandbox_plan_hash,
            prepared_at,
            binding_hash: CanonicalSha256HashV1::sha256(&[]),
        };
        value.binding_hash = value.compute_hash()?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ToolConformanceAuthorizationErrorV1> {
        self.allow_once.validate()?;
        if self.binding_version != TOOL_CONFORMANCE_SANDBOX_DISPATCH_BINDING_VERSION_V1
            || self.runtime_binding_hash != self.allow_once.runtime_binding_hash
            || self.sandbox_plan_hash != self.allow_once.sandbox_plan_hash
            || self.selected_mode != NativeSandboxMode::StrictAppContainer
            || self.network_access != NetworkAccessRequirement::NotRequired
            || self.network_enforcement != NetworkEnforcementKind::AppContainerNetworkDenied
            || self.binding_hash != self.compute_hash()?
        {
            return Err(ToolConformanceAuthorizationErrorV1::DispatchBindingInvalid);
        }
        Ok(())
    }

    fn compute_hash(&self) -> Result<CanonicalSha256HashV1, ToolConformanceAuthorizationErrorV1> {
        canonical_sha256(&ToolConformanceSandboxDispatchBindingHashMaterialV1 {
            binding_version: self.binding_version,
            allow_once: &self.allow_once,
            runtime_binding_hash: &self.runtime_binding_hash,
            selected_mode: self.selected_mode,
            network_access: self.network_access,
            network_enforcement: self.network_enforcement,
            sandbox_plan_hash: &self.sandbox_plan_hash,
            prepared_at: self.prepared_at,
        })
        .map_err(|_| ToolConformanceAuthorizationErrorV1::Canonicalization)
    }
}

impl<'de> Deserialize<'de> for ToolConformanceSandboxDispatchBindingV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            binding_version: u32,
            allow_once: ToolConformanceAllowOnceEvidenceV1,
            runtime_binding_hash: CanonicalHashV1,
            selected_mode: NativeSandboxMode,
            network_access: NetworkAccessRequirement,
            network_enforcement: NetworkEnforcementKind,
            sandbox_plan_hash: ContentHash,
            prepared_at: UtcTimestamp,
            binding_hash: CanonicalSha256HashV1,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            binding_version: wire.binding_version,
            allow_once: wire.allow_once,
            runtime_binding_hash: wire.runtime_binding_hash,
            selected_mode: wire.selected_mode,
            network_access: wire.network_access,
            network_enforcement: wire.network_enforcement,
            sandbox_plan_hash: wire.sandbox_plan_hash,
            prepared_at: wire.prepared_at,
            binding_hash: wire.binding_hash,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedToolConformanceSandboxExecutionV1 {
    pub runtime: NonSecretToolConformanceSandboxRuntimeFactsV1,
    pub binding: ToolConformanceSandboxDispatchBindingV1,
    pub plan: SandboxExecutionPlan,
}

impl AuthorizedToolConformanceSandboxExecutionV1 {
    pub fn new(
        runtime: NonSecretToolConformanceSandboxRuntimeFactsV1,
        binding: ToolConformanceSandboxDispatchBindingV1,
        plan: SandboxExecutionPlan,
    ) -> Result<Self, ToolConformanceAuthorizationErrorV1> {
        let value = Self { runtime, binding, plan };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ToolConformanceAuthorizationErrorV1> {
        self.runtime.validate()?;
        self.binding.validate()?;
        let allow = &self.binding.allow_once;
        if self.runtime.canonical_hash()? != self.binding.runtime_binding_hash
            || self.plan.execution_id != allow.sandbox_execution_id
            || self.plan.identity.actor != InvocationActor::User
            || self.plan.identity.workspace_id.is_some()
            || self.plan.identity.session_id.is_some()
            || self.plan.identity.turn_task_id.is_some()
            || self.plan.identity.agent_run_id.is_some()
            || self.plan.identity.parent_agent_run_id.is_some()
            || self.plan.identity.correlation_id != allow.correlation_id
            || self.plan.identity.permission_snapshot_id != Some(allow.authorization_snapshot_id)
            || self.plan.permission_snapshot_id != allow.authorization_snapshot_id
            || self.plan.mode != self.binding.selected_mode
            || self.plan.network_access != self.binding.network_access
            || self.plan.network_enforcement != self.binding.network_enforcement
            || self.plan.plan_hash != self.binding.sandbox_plan_hash
            || self.plan.executable != self.runtime.executable
            || self.plan.args != self.runtime.arguments
            || self.plan.working_directory != self.runtime.working_directory
            || self.plan.environment != self.runtime.environment
            || self.plan.filesystem_grants != self.runtime.filesystem_grants
            || !self.plan.network_grant_ids.is_empty()
            || self.plan.timeout_ms != self.runtime.timeout_ms
            || self.plan.output_limits != self.runtime.output_limits
        {
            return Err(ToolConformanceAuthorizationErrorV1::DispatchPlanMismatch);
        }
        Ok(())
    }

    pub fn aura_stdio_dispatch_bootstrap(
        &self,
    ) -> Result<
        Option<ToolConformanceAuraStdioDispatchBootstrapV1>,
        ToolConformanceAuthorizationErrorV1,
    > {
        self.validate()?;
        Ok(self.runtime.output_root.clone().map(|output_root| {
            ToolConformanceAuraStdioDispatchBootstrapV1 {
                tool_call_id: self.binding.allow_once.tool_call_id,
                tool_execution_attempt_id: self.binding.allow_once.tool_execution_attempt_id,
                output_root,
            }
        }))
    }
}

impl<'de> Deserialize<'de> for AuthorizedToolConformanceSandboxExecutionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            runtime: NonSecretToolConformanceSandboxRuntimeFactsV1,
            binding: ToolConformanceSandboxDispatchBindingV1,
            plan: SandboxExecutionPlan,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.runtime, wire.binding, wire.plan).map_err(D::Error::custom)
    }
}

/// Launch-only bootstrap derived from the consumed W3 dispatch binding. It is
/// Rust-only and cannot become a second persisted authority source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolConformanceAuraStdioDispatchBootstrapV1 {
    tool_call_id: ToolCallId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    output_root: CanonicalPath,
}

impl ToolConformanceAuraStdioDispatchBootstrapV1 {
    #[must_use]
    pub const fn tool_call_id(&self) -> ToolCallId {
        self.tool_call_id
    }

    #[must_use]
    pub const fn tool_execution_attempt_id(&self) -> ToolExecutionAttemptId {
        self.tool_execution_attempt_id
    }

    #[must_use]
    pub const fn output_root(&self) -> &CanonicalPath {
        &self.output_root
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolConformanceAuthorizationErrorV1 {
    #[error("conformance runtime facts are invalid")]
    RuntimeFactsInvalid,
    #[error("conformance AllowOnce evidence is invalid")]
    AllowOnceEvidenceInvalid,
    #[error("conformance Sandbox dispatch binding is invalid")]
    DispatchBindingInvalid,
    #[error("conformance Sandbox execution plan does not match its binding")]
    DispatchPlanMismatch,
    #[error("conformance authorization evidence could not be canonicalized")]
    Canonicalization,
}

fn has_duplicate_environment(values: &[SanitizedEnvironmentEntry]) -> bool {
    let mut names = std::collections::BTreeSet::new();
    values.iter().any(|entry| !names.insert(entry.name.to_ascii_uppercase()))
}

fn has_forbidden_runtime_environment(
    values: &[SanitizedEnvironmentEntry],
    credential: Option<&ToolConformanceSyntheticCredentialUseV1>,
) -> bool {
    values.iter().any(|entry| {
        entry.name.eq_ignore_ascii_case(AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1)
            || entry
                .name
                .eq_ignore_ascii_case(AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1)
            || entry.name.eq_ignore_ascii_case(AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1)
            || credential.is_some_and(|credential| {
                entry.name.eq_ignore_ascii_case(credential.environment_variable.as_str())
            })
    })
}

#[allow(dead_code)]
fn _scope_shape_is_closed(scope: &ResolvedScope) -> bool {
    matches!(scope, ResolvedScope::ToolConformanceRuntime { .. })
}
