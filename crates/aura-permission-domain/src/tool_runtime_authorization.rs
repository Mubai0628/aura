use aura_domain_common::jcs::{
    CanonicalBlake3HashV1, CanonicalHashV1, canonical_blake3, canonical_sha256,
};
use aura_domain_common::{
    ContentHash, ContentHashAlgorithm, CredentialId, CredentialRevision, SandboxExecutionId,
    SecurityOperationId, UtcTimestamp,
};
use aura_domain_common::{ToolBatchId, ToolCallId, ToolExecutionAttemptId};
use aura_tool_contract::{
    AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1, CoreInvocationIdentityV1, CredentialSlotKey,
    EnvironmentVariableNameV1, HttpHeaderNameV1, ToolPurposeTextV1,
    ToolRuntimeOutputAuthorizationV1,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use thiserror::Error;

use crate::{
    CanonicalExecutableRef, NativeSandboxMode, NetworkAccessRequirement, NetworkEnforcementKind,
    ProcessOutputLimits, ResolvedScope, SandboxExecutionPlan, SandboxFilesystemGrant,
    SanitizedEnvironmentEntry, ToolSecuritySourceRefV1, WindowsOsString,
};
use aura_security_contract::{
    CanonicalPath, InvocationIdentity, OperationRiskFlag, PermissionAllowAuthorizationBasisV1,
    PermissionSnapshot, ResolvedCapability,
};

pub const W7_TOOL_OPERATION_PREIMAGE_VERSION_V1: u32 = 1;
pub const TOOL_SANDBOX_DISPATCH_BINDING_VERSION_V1: u32 = 1;
pub const AURA_STDIO_BOOTSTRAP_POLICY_VERSION_V1: u32 = 1;
pub const AURA_STDIO_OUTPUT_PROVISION_POLICY_VERSION_V1: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub enum W7ToolOperationFingerprintDomainV1 {
    #[serde(rename = "aura.p07.w7_tool_operation")]
    AuraP07W7ToolOperation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPreparedRuntimeKindV1 {
    DeclarativeCommandArgv,
    DeclarativeCommandPowershellScript,
    AuraStdio,
    DeclarativeHttp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub enum AuraStdioToolCallIdEnvironmentVariableV1 {
    #[serde(rename = "AURA_TOOL_CALL_ID")]
    AuraToolCallId,
}

impl AuraStdioToolCallIdEnvironmentVariableV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub enum AuraStdioToolExecutionAttemptIdEnvironmentVariableV1 {
    #[serde(rename = "AURA_TOOL_EXECUTION_ATTEMPT_ID")]
    AuraToolExecutionAttemptId,
}

impl AuraStdioToolExecutionAttemptIdEnvironmentVariableV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuraStdioToolCallIdSourceV1 {
    AdmittedToolCall,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuraStdioToolExecutionAttemptIdSourceV1 {
    DispatchBinding,
}

/// Approval-time Aura Stdio identity bootstrap policy. The actual `tea_` is
/// intentionally absent until the pre-dispatch transaction creates it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuraStdioBootstrapPolicyV1 {
    policy_version: u32,
    tool_call_id_environment_variable: AuraStdioToolCallIdEnvironmentVariableV1,
    tool_execution_attempt_id_environment_variable:
        AuraStdioToolExecutionAttemptIdEnvironmentVariableV1,
    tool_call_id_source: AuraStdioToolCallIdSourceV1,
    tool_execution_attempt_id_source: AuraStdioToolExecutionAttemptIdSourceV1,
}

impl AuraStdioBootstrapPolicyV1 {
    #[must_use]
    pub const fn frozen() -> Self {
        Self {
            policy_version: AURA_STDIO_BOOTSTRAP_POLICY_VERSION_V1,
            tool_call_id_environment_variable:
                AuraStdioToolCallIdEnvironmentVariableV1::AuraToolCallId,
            tool_execution_attempt_id_environment_variable:
                AuraStdioToolExecutionAttemptIdEnvironmentVariableV1::AuraToolExecutionAttemptId,
            tool_call_id_source: AuraStdioToolCallIdSourceV1::AdmittedToolCall,
            tool_execution_attempt_id_source:
                AuraStdioToolExecutionAttemptIdSourceV1::DispatchBinding,
        }
    }

    #[must_use]
    pub const fn is_frozen(&self) -> bool {
        self.policy_version == AURA_STDIO_BOOTSTRAP_POLICY_VERSION_V1
            && matches!(
                self.tool_call_id_environment_variable,
                AuraStdioToolCallIdEnvironmentVariableV1::AuraToolCallId
            )
            && matches!(
                self.tool_execution_attempt_id_environment_variable,
                AuraStdioToolExecutionAttemptIdEnvironmentVariableV1::AuraToolExecutionAttemptId
            )
            && matches!(self.tool_call_id_source, AuraStdioToolCallIdSourceV1::AdmittedToolCall)
            && matches!(
                self.tool_execution_attempt_id_source,
                AuraStdioToolExecutionAttemptIdSourceV1::DispatchBinding
            )
    }
}

impl<'de> Deserialize<'de> for AuraStdioBootstrapPolicyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WirePolicy {
            policy_version: u32,
            tool_call_id_environment_variable: AuraStdioToolCallIdEnvironmentVariableV1,
            tool_execution_attempt_id_environment_variable:
                AuraStdioToolExecutionAttemptIdEnvironmentVariableV1,
            tool_call_id_source: AuraStdioToolCallIdSourceV1,
            tool_execution_attempt_id_source: AuraStdioToolExecutionAttemptIdSourceV1,
        }

        let wire = WirePolicy::deserialize(deserializer)?;
        let value = Self {
            policy_version: wire.policy_version,
            tool_call_id_environment_variable: wire.tool_call_id_environment_variable,
            tool_execution_attempt_id_environment_variable: wire
                .tool_execution_attempt_id_environment_variable,
            tool_call_id_source: wire.tool_call_id_source,
            tool_execution_attempt_id_source: wire.tool_execution_attempt_id_source,
        };
        value.is_frozen().then_some(value).ok_or_else(|| {
            D::Error::custom("Aura Stdio bootstrap policy must match frozen policy version 1")
        })
    }
}

impl JsonSchema for AuraStdioBootstrapPolicyV1 {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("AuraStdioBootstrapPolicyV1")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "policy_version",
                "tool_call_id_environment_variable",
                "tool_execution_attempt_id_environment_variable",
                "tool_call_id_source",
                "tool_execution_attempt_id_source"
            ],
            "properties": {
                "policy_version": { "type": "integer", "const": 1 },
                "tool_call_id_environment_variable": {
                    "type": "string",
                    "const": "AURA_TOOL_CALL_ID"
                },
                "tool_execution_attempt_id_environment_variable": {
                    "type": "string",
                    "const": "AURA_TOOL_EXECUTION_ATTEMPT_ID"
                },
                "tool_call_id_source": { "type": "string", "const": "admitted_tool_call" },
                "tool_execution_attempt_id_source": {
                    "type": "string",
                    "const": "dispatch_binding"
                }
            }
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub enum AuraStdioOutputRootEnvironmentVariableV1 {
    #[serde(rename = "AURA_TOOL_OUTPUT_ROOT")]
    AuraToolOutputRoot,
}

impl AuraStdioOutputRootEnvironmentVariableV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuraStdioOutputRootSourceV1 {
    DispatchBinding,
}

/// Approval-time rule that Core will provide one private, Attempt-local
/// output root after `tea_` creation. The actual path and authorization are
/// deliberately absent from the operation fingerprint.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuraStdioOutputProvisionPolicyV1 {
    policy_version: u32,
    output_root_environment_variable: AuraStdioOutputRootEnvironmentVariableV1,
    output_root_source: AuraStdioOutputRootSourceV1,
}

impl AuraStdioOutputProvisionPolicyV1 {
    #[must_use]
    pub const fn frozen() -> Self {
        Self {
            policy_version: AURA_STDIO_OUTPUT_PROVISION_POLICY_VERSION_V1,
            output_root_environment_variable:
                AuraStdioOutputRootEnvironmentVariableV1::AuraToolOutputRoot,
            output_root_source: AuraStdioOutputRootSourceV1::DispatchBinding,
        }
    }

    #[must_use]
    pub const fn is_frozen(&self) -> bool {
        self.policy_version == AURA_STDIO_OUTPUT_PROVISION_POLICY_VERSION_V1
            && matches!(
                self.output_root_environment_variable,
                AuraStdioOutputRootEnvironmentVariableV1::AuraToolOutputRoot
            )
            && matches!(self.output_root_source, AuraStdioOutputRootSourceV1::DispatchBinding)
    }
}

impl<'de> Deserialize<'de> for AuraStdioOutputProvisionPolicyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WirePolicy {
            policy_version: u32,
            output_root_environment_variable: AuraStdioOutputRootEnvironmentVariableV1,
            output_root_source: AuraStdioOutputRootSourceV1,
        }

        let wire = WirePolicy::deserialize(deserializer)?;
        let value = Self {
            policy_version: wire.policy_version,
            output_root_environment_variable: wire.output_root_environment_variable,
            output_root_source: wire.output_root_source,
        };
        value.is_frozen().then_some(value).ok_or_else(|| {
            D::Error::custom(
                "Aura Stdio output provision policy must match frozen policy version 1",
            )
        })
    }
}

impl JsonSchema for AuraStdioOutputProvisionPolicyV1 {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("AuraStdioOutputProvisionPolicyV1")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "policy_version",
                "output_root_environment_variable",
                "output_root_source"
            ],
            "properties": {
                "policy_version": { "type": "integer", "const": 1 },
                "output_root_environment_variable": {
                    "type": "string",
                    "const": "AURA_TOOL_OUTPUT_ROOT"
                },
                "output_root_source": {
                    "type": "string",
                    "const": "dispatch_binding"
                }
            }
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "placement_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolCredentialPlacementV1 {
    Environment { environment_variable: EnvironmentVariableNameV1 },
    BearerAuthorization,
    FixedHeader { header_name: HttpHeaderNameV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialUseV1 {
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub slot_key: CredentialSlotKey,
    pub placement: ToolCredentialPlacementV1,
}

/// Complete non-secret facts that P04 approves for a short-lived command or
/// Aura Stdio launch. Attempt-local TEMP/TMP and the Credential plaintext are
/// deliberately absent and may only be materialized after the pre-dispatch
/// transaction has committed.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NonSecretSandboxRuntimeFactsV1 {
    pub runtime_kind: ToolPreparedRuntimeKindV1,
    pub executable: CanonicalExecutableRef,
    pub arguments: Vec<WindowsOsString>,
    pub working_directory: CanonicalPath,
    pub environment: Vec<SanitizedEnvironmentEntry>,
    /// Exact non-secret filesystem launch facts approved by P04. These are
    /// part of the operation fingerprint and cannot be widened when the
    /// Sandbox plan is materialized.
    pub filesystem_grants: Vec<SandboxFilesystemGrant>,
    pub credential: Option<ToolCredentialUseV1>,
    pub success_exit_codes: Vec<i32>,
    pub stdio_protocol_version: Option<u32>,
    pub aura_stdio_bootstrap: Option<AuraStdioBootstrapPolicyV1>,
    pub aura_stdio_output: Option<AuraStdioOutputProvisionPolicyV1>,
    pub timeout_ms: u64,
    pub output_limits: ProcessOutputLimits,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NonSecretHttpRuntimeFactsV1 {
    pub runtime_kind: ToolPreparedRuntimeKindV1,
    pub method: String,
    pub exact_url: String,
    pub canonical_path: String,
    pub headers_hash: CanonicalHashV1,
    pub body_hash: CanonicalHashV1,
    pub body_byte_length: u64,
    pub credential: Option<ToolCredentialUseV1>,
    pub success_status_ranges: Vec<(u16, u16)>,
    pub prepared_request_hash: CanonicalHashV1,
    pub grant_manifest_hash: CanonicalHashV1,
    pub timeout_ms: u64,
    pub response_wire_limit: u64,
    pub response_decompressed_limit: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "execution_path", rename_all = "snake_case", deny_unknown_fields)]
pub enum NonSecretToolRuntimeFactsV1 {
    Sandbox { facts: Box<NonSecretSandboxRuntimeFactsV1> },
    NetworkGateway { facts: Box<NonSecretHttpRuntimeFactsV1> },
}

impl NonSecretToolRuntimeFactsV1 {
    pub fn validate(&self) -> Result<(), ToolRuntimeAuthorizationErrorV1> {
        match self {
            Self::Sandbox { facts } => {
                if !matches!(
                    facts.runtime_kind,
                    ToolPreparedRuntimeKindV1::DeclarativeCommandArgv
                        | ToolPreparedRuntimeKindV1::DeclarativeCommandPowershellScript
                        | ToolPreparedRuntimeKindV1::AuraStdio
                ) || facts.timeout_ms == 0
                    || facts.output_limits.stdout_bytes == 0
                    || facts.output_limits.stderr_bytes == 0
                    || facts.output_limits.combined_bytes == 0
                    || (facts.runtime_kind == ToolPreparedRuntimeKindV1::AuraStdio)
                        != facts.stdio_protocol_version.is_some()
                    || (facts.runtime_kind == ToolPreparedRuntimeKindV1::AuraStdio)
                        != facts.aura_stdio_bootstrap.is_some()
                    || (facts.runtime_kind == ToolPreparedRuntimeKindV1::AuraStdio)
                        != facts.aura_stdio_output.is_some()
                    || facts.aura_stdio_bootstrap.as_ref().is_some_and(|policy| !policy.is_frozen())
                    || facts.aura_stdio_output.as_ref().is_some_and(|policy| !policy.is_frozen())
                    || (facts.runtime_kind == ToolPreparedRuntimeKindV1::AuraStdio
                        && !facts.success_exit_codes.is_empty())
                    || (facts.runtime_kind != ToolPreparedRuntimeKindV1::AuraStdio
                        && facts.success_exit_codes.is_empty())
                    || facts.credential.as_ref().is_some_and(|credential| {
                        !matches!(
                            &credential.placement,
                            ToolCredentialPlacementV1::Environment { .. }
                        )
                    })
                    || has_duplicate_environment(&facts.environment)
                    || has_attempt_local_environment(&facts.environment)
                    || has_aura_stdio_bootstrap_environment(&facts.environment)
                {
                    return Err(ToolRuntimeAuthorizationErrorV1::RuntimeFactsInvalid);
                }
            }
            Self::NetworkGateway { facts } => {
                if facts.runtime_kind != ToolPreparedRuntimeKindV1::DeclarativeHttp
                    || facts.method.is_empty()
                    || facts.exact_url.is_empty()
                    || !facts.canonical_path.starts_with('/')
                    || facts.timeout_ms == 0
                    || facts.response_wire_limit == 0
                    || facts.response_decompressed_limit == 0
                    || facts.response_wire_limit > facts.response_decompressed_limit
                    || facts.success_status_ranges.is_empty()
                    || facts.credential.as_ref().is_some_and(|credential| {
                        matches!(
                            &credential.placement,
                            ToolCredentialPlacementV1::Environment { .. }
                        )
                    })
                {
                    return Err(ToolRuntimeAuthorizationErrorV1::RuntimeFactsInvalid);
                }
            }
        }
        Ok(())
    }

    pub fn canonical_hash(&self) -> Result<CanonicalHashV1, ToolRuntimeAuthorizationErrorV1> {
        self.validate()?;
        canonical_sha256(self).map_err(|_| ToolRuntimeAuthorizationErrorV1::Canonicalization)
    }

    #[must_use]
    pub fn credential(&self) -> Option<&ToolCredentialUseV1> {
        match self {
            Self::Sandbox { facts } => facts.credential.as_ref(),
            Self::NetworkGateway { facts } => facts.credential.as_ref(),
        }
    }
}

#[derive(Clone, Debug, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct W7ToolOperationFingerprintPreimageV1 {
    pub kind: W7ToolOperationFingerprintDomainV1,
    pub preimage_version: u32,
    pub invocation_identity: CoreInvocationIdentityV1,
    pub tool_batch_id: ToolBatchId,
    pub tool_call_id: ToolCallId,
    pub source: ToolSecuritySourceRefV1,
    pub source_security_fingerprint: ContentHash,
    pub execution_plan_hash: CanonicalHashV1,
    pub capabilities: Vec<ResolvedCapability>,
    pub scopes: Vec<ResolvedScope>,
    pub risks: Vec<OperationRiskFlag>,
    pub purpose: ToolPurposeTextV1,
    pub arguments: Value,
    pub arguments_hash: CanonicalHashV1,
    pub runtime: NonSecretToolRuntimeFactsV1,
    pub runtime_binding_hash: CanonicalHashV1,
    pub sandbox_mode: Option<NativeSandboxMode>,
    pub network_access: NetworkAccessRequirement,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct W7ToolOperationFingerprintV1(CanonicalBlake3HashV1);

impl W7ToolOperationFingerprintPreimageV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        invocation_identity: CoreInvocationIdentityV1,
        tool_batch_id: ToolBatchId,
        tool_call_id: ToolCallId,
        source: ToolSecuritySourceRefV1,
        source_security_fingerprint: ContentHash,
        execution_plan_hash: CanonicalHashV1,
        capabilities: Vec<ResolvedCapability>,
        scopes: Vec<ResolvedScope>,
        risks: Vec<OperationRiskFlag>,
        purpose: ToolPurposeTextV1,
        arguments: Value,
        arguments_hash: CanonicalHashV1,
        runtime: NonSecretToolRuntimeFactsV1,
        sandbox_mode: Option<NativeSandboxMode>,
        network_access: NetworkAccessRequirement,
    ) -> Result<Self, ToolRuntimeAuthorizationErrorV1> {
        source.validate().map_err(|_| ToolRuntimeAuthorizationErrorV1::SourceMismatch)?;
        runtime.validate()?;
        let runtime_binding_hash = runtime.canonical_hash()?;
        let value = Self {
            kind: W7ToolOperationFingerprintDomainV1::AuraP07W7ToolOperation,
            preimage_version: W7_TOOL_OPERATION_PREIMAGE_VERSION_V1,
            invocation_identity,
            tool_batch_id,
            tool_call_id,
            source,
            source_security_fingerprint,
            execution_plan_hash,
            capabilities,
            scopes,
            risks,
            purpose,
            arguments,
            arguments_hash,
            runtime,
            runtime_binding_hash,
            sandbox_mode,
            network_access,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ToolRuntimeAuthorizationErrorV1> {
        if self.kind != W7ToolOperationFingerprintDomainV1::AuraP07W7ToolOperation
            || self.preimage_version != W7_TOOL_OPERATION_PREIMAGE_VERSION_V1
            || self.arguments_hash
                != canonical_sha256(&self.arguments)
                    .map_err(|_| ToolRuntimeAuthorizationErrorV1::Canonicalization)?
            || self.runtime_binding_hash != self.runtime.canonical_hash()?
            || self.capabilities.is_empty()
            || self.scopes.is_empty()
            || (matches!(self.runtime, NonSecretToolRuntimeFactsV1::Sandbox { .. })
                && self.sandbox_mode.is_none())
            || (matches!(self.runtime, NonSecretToolRuntimeFactsV1::NetworkGateway { .. })
                && self.sandbox_mode.is_some())
        {
            return Err(ToolRuntimeAuthorizationErrorV1::PreimageInvalid);
        }
        self.source.validate().map_err(|_| ToolRuntimeAuthorizationErrorV1::SourceMismatch)
    }

    pub fn fingerprint(
        &self,
    ) -> Result<W7ToolOperationFingerprintV1, ToolRuntimeAuthorizationErrorV1> {
        self.validate()?;
        canonical_blake3(self)
            .map(W7ToolOperationFingerprintV1)
            .map_err(|_| ToolRuntimeAuthorizationErrorV1::Canonicalization)
    }
}

impl W7ToolOperationFingerprintV1 {
    #[must_use]
    pub const fn canonical_hash(&self) -> &CanonicalBlake3HashV1 {
        &self.0
    }

    pub fn as_content_hash(&self) -> ContentHash {
        ContentHash::new(ContentHashAlgorithm::Blake3, self.0.hex())
            .expect("validated canonical BLAKE3 digest is a ContentHash")
    }
}

impl<'de> Deserialize<'de> for W7ToolOperationFingerprintV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CanonicalBlake3HashV1::deserialize(deserializer).map(Self)
    }
}

impl<'de> Deserialize<'de> for W7ToolOperationFingerprintPreimageV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WirePreimage {
            kind: W7ToolOperationFingerprintDomainV1,
            preimage_version: u32,
            invocation_identity: CoreInvocationIdentityV1,
            tool_batch_id: ToolBatchId,
            tool_call_id: ToolCallId,
            source: ToolSecuritySourceRefV1,
            source_security_fingerprint: ContentHash,
            execution_plan_hash: CanonicalHashV1,
            capabilities: Vec<ResolvedCapability>,
            scopes: Vec<ResolvedScope>,
            risks: Vec<OperationRiskFlag>,
            purpose: ToolPurposeTextV1,
            arguments: Value,
            arguments_hash: CanonicalHashV1,
            runtime: NonSecretToolRuntimeFactsV1,
            runtime_binding_hash: CanonicalHashV1,
            sandbox_mode: Option<NativeSandboxMode>,
            network_access: NetworkAccessRequirement,
        }
        let wire = WirePreimage::deserialize(deserializer)?;
        let value = Self {
            kind: wire.kind,
            preimage_version: wire.preimage_version,
            invocation_identity: wire.invocation_identity,
            tool_batch_id: wire.tool_batch_id,
            tool_call_id: wire.tool_call_id,
            source: wire.source,
            source_security_fingerprint: wire.source_security_fingerprint,
            execution_plan_hash: wire.execution_plan_hash,
            capabilities: wire.capabilities,
            scopes: wire.scopes,
            risks: wire.risks,
            purpose: wire.purpose,
            arguments: wire.arguments,
            arguments_hash: wire.arguments_hash,
            runtime: wire.runtime,
            runtime_binding_hash: wire.runtime_binding_hash,
            sandbox_mode: wire.sandbox_mode,
            network_access: wire.network_access,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

/// Durable, one-shot authorization/dispatch relation consumed by Sandbox
/// Host before it may spawn a Tool process. It contains no secret bytes.
#[derive(Clone, Debug, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSandboxDispatchBindingV1 {
    pub binding_version: u32,
    pub operation_id: SecurityOperationId,
    pub operation_identity: InvocationIdentity,
    pub operation_fingerprint: W7ToolOperationFingerprintV1,
    pub permission_snapshot: PermissionSnapshot,
    pub authorization_basis: PermissionAllowAuthorizationBasisV1,
    pub tool_batch_id: ToolBatchId,
    pub tool_call_id: ToolCallId,
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub sandbox_execution_id: SandboxExecutionId,
    pub execution_plan_hash: CanonicalHashV1,
    pub runtime_binding_hash: CanonicalHashV1,
    pub source: ToolSecuritySourceRefV1,
    pub source_security_fingerprint: ContentHash,
    pub selected_mode: NativeSandboxMode,
    pub network_access: NetworkAccessRequirement,
    pub network_enforcement: NetworkEnforcementKind,
    pub sandbox_plan_hash: ContentHash,
    pub runtime_output_authorization: Option<ToolRuntimeOutputAuthorizationV1>,
    pub attempt_output_root: Option<CanonicalPath>,
    pub prepared_at: UtcTimestamp,
    pub binding_hash: CanonicalHashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolSandboxDispatchBindingHashMaterialV1<'a> {
    binding_version: u32,
    operation_id: SecurityOperationId,
    operation_identity: &'a InvocationIdentity,
    operation_fingerprint: &'a W7ToolOperationFingerprintV1,
    permission_snapshot: &'a PermissionSnapshot,
    authorization_basis: &'a PermissionAllowAuthorizationBasisV1,
    tool_batch_id: ToolBatchId,
    tool_call_id: ToolCallId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    sandbox_execution_id: SandboxExecutionId,
    execution_plan_hash: &'a CanonicalHashV1,
    runtime_binding_hash: &'a CanonicalHashV1,
    source: &'a ToolSecuritySourceRefV1,
    source_security_fingerprint: &'a ContentHash,
    selected_mode: NativeSandboxMode,
    network_access: NetworkAccessRequirement,
    network_enforcement: NetworkEnforcementKind,
    sandbox_plan_hash: &'a ContentHash,
    runtime_output_authorization: &'a Option<ToolRuntimeOutputAuthorizationV1>,
    attempt_output_root: &'a Option<CanonicalPath>,
    prepared_at: UtcTimestamp,
}

impl ToolSandboxDispatchBindingV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: SecurityOperationId,
        operation_identity: InvocationIdentity,
        operation_fingerprint: W7ToolOperationFingerprintV1,
        permission_snapshot: PermissionSnapshot,
        authorization_basis: PermissionAllowAuthorizationBasisV1,
        tool_batch_id: ToolBatchId,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        sandbox_execution_id: SandboxExecutionId,
        execution_plan_hash: CanonicalHashV1,
        runtime_binding_hash: CanonicalHashV1,
        source: ToolSecuritySourceRefV1,
        source_security_fingerprint: ContentHash,
        selected_mode: NativeSandboxMode,
        network_access: NetworkAccessRequirement,
        network_enforcement: NetworkEnforcementKind,
        sandbox_plan_hash: ContentHash,
        runtime_output_authorization: Option<ToolRuntimeOutputAuthorizationV1>,
        attempt_output_root: Option<CanonicalPath>,
        prepared_at: UtcTimestamp,
    ) -> Result<Self, ToolRuntimeAuthorizationErrorV1> {
        let mut value = Self {
            binding_version: TOOL_SANDBOX_DISPATCH_BINDING_VERSION_V1,
            operation_id,
            operation_identity,
            operation_fingerprint,
            permission_snapshot,
            authorization_basis,
            tool_batch_id,
            tool_call_id,
            tool_execution_attempt_id,
            sandbox_execution_id,
            execution_plan_hash,
            runtime_binding_hash,
            source,
            source_security_fingerprint,
            selected_mode,
            network_access,
            network_enforcement,
            sandbox_plan_hash,
            runtime_output_authorization,
            attempt_output_root,
            prepared_at,
            binding_hash: CanonicalHashV1::sha256(&[]),
        };
        value.binding_hash = value.compute_hash()?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ToolRuntimeAuthorizationErrorV1> {
        let authorization_basis_matches_snapshot = match &self.authorization_basis {
            PermissionAllowAuthorizationBasisV1::PermissionMode { mode } => {
                *mode == self.permission_snapshot.mode
            }
            PermissionAllowAuthorizationBasisV1::PermissionGrant {
                permission_grant_id, ..
            } => self.permission_snapshot.active_grant_ids.contains(permission_grant_id),
        };
        if self.binding_version != TOOL_SANDBOX_DISPATCH_BINDING_VERSION_V1
            || self.operation_identity.permission_snapshot_id != Some(self.permission_snapshot.id)
            || self.operation_identity.session_id != Some(self.permission_snapshot.session_id)
            || self.operation_identity.workspace_id != Some(self.permission_snapshot.workspace_id)
            || !authorization_basis_matches_snapshot
            || !dispatch_output_binding_is_valid(self)
            || self.binding_hash != self.compute_hash()?
        {
            return Err(ToolRuntimeAuthorizationErrorV1::DispatchBindingInvalid);
        }
        self.source.validate().map_err(|_| ToolRuntimeAuthorizationErrorV1::SourceMismatch)
    }

    fn compute_hash(&self) -> Result<CanonicalHashV1, ToolRuntimeAuthorizationErrorV1> {
        canonical_sha256(&ToolSandboxDispatchBindingHashMaterialV1 {
            binding_version: self.binding_version,
            operation_id: self.operation_id,
            operation_identity: &self.operation_identity,
            operation_fingerprint: &self.operation_fingerprint,
            permission_snapshot: &self.permission_snapshot,
            authorization_basis: &self.authorization_basis,
            tool_batch_id: self.tool_batch_id,
            tool_call_id: self.tool_call_id,
            tool_execution_attempt_id: self.tool_execution_attempt_id,
            sandbox_execution_id: self.sandbox_execution_id,
            execution_plan_hash: &self.execution_plan_hash,
            runtime_binding_hash: &self.runtime_binding_hash,
            source: &self.source,
            source_security_fingerprint: &self.source_security_fingerprint,
            selected_mode: self.selected_mode,
            network_access: self.network_access,
            network_enforcement: self.network_enforcement,
            sandbox_plan_hash: &self.sandbox_plan_hash,
            runtime_output_authorization: &self.runtime_output_authorization,
            attempt_output_root: &self.attempt_output_root,
            prepared_at: self.prepared_at,
        })
        .map_err(|_| ToolRuntimeAuthorizationErrorV1::Canonicalization)
    }
}

impl<'de> Deserialize<'de> for ToolSandboxDispatchBindingV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireBinding {
            binding_version: u32,
            operation_id: SecurityOperationId,
            operation_identity: InvocationIdentity,
            operation_fingerprint: W7ToolOperationFingerprintV1,
            permission_snapshot: PermissionSnapshot,
            authorization_basis: PermissionAllowAuthorizationBasisV1,
            tool_batch_id: ToolBatchId,
            tool_call_id: ToolCallId,
            tool_execution_attempt_id: ToolExecutionAttemptId,
            sandbox_execution_id: SandboxExecutionId,
            execution_plan_hash: CanonicalHashV1,
            runtime_binding_hash: CanonicalHashV1,
            source: ToolSecuritySourceRefV1,
            source_security_fingerprint: ContentHash,
            selected_mode: NativeSandboxMode,
            network_access: NetworkAccessRequirement,
            network_enforcement: NetworkEnforcementKind,
            sandbox_plan_hash: ContentHash,
            runtime_output_authorization: Option<ToolRuntimeOutputAuthorizationV1>,
            attempt_output_root: Option<CanonicalPath>,
            prepared_at: UtcTimestamp,
            binding_hash: CanonicalHashV1,
        }
        let wire = WireBinding::deserialize(deserializer)?;
        let value = Self {
            binding_version: wire.binding_version,
            operation_id: wire.operation_id,
            operation_identity: wire.operation_identity,
            operation_fingerprint: wire.operation_fingerprint,
            permission_snapshot: wire.permission_snapshot,
            authorization_basis: wire.authorization_basis,
            tool_batch_id: wire.tool_batch_id,
            tool_call_id: wire.tool_call_id,
            tool_execution_attempt_id: wire.tool_execution_attempt_id,
            sandbox_execution_id: wire.sandbox_execution_id,
            execution_plan_hash: wire.execution_plan_hash,
            runtime_binding_hash: wire.runtime_binding_hash,
            source: wire.source,
            source_security_fingerprint: wire.source_security_fingerprint,
            selected_mode: wire.selected_mode,
            network_access: wire.network_access,
            network_enforcement: wire.network_enforcement,
            sandbox_plan_hash: wire.sandbox_plan_hash,
            runtime_output_authorization: wire.runtime_output_authorization,
            attempt_output_root: wire.attempt_output_root,
            prepared_at: wire.prepared_at,
            binding_hash: wire.binding_hash,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedToolSandboxExecutionV1 {
    pub operation: W7ToolOperationFingerprintPreimageV1,
    pub binding: ToolSandboxDispatchBindingV1,
    pub plan: SandboxExecutionPlan,
}

/// Launch-only Aura Stdio identity material derived from the already
/// authorized one-shot dispatch binding. It is deliberately not serializable
/// and therefore cannot become a second persisted identity source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuraStdioDispatchBootstrapV1 {
    tool_call_id: ToolCallId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    output_root: CanonicalPath,
    output_authorization: ToolRuntimeOutputAuthorizationV1,
}

impl AuraStdioDispatchBootstrapV1 {
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

    #[must_use]
    pub const fn output_authorization(&self) -> &ToolRuntimeOutputAuthorizationV1 {
        &self.output_authorization
    }
}

impl AuthorizedToolSandboxExecutionV1 {
    pub fn new(
        operation: W7ToolOperationFingerprintPreimageV1,
        binding: ToolSandboxDispatchBindingV1,
        plan: SandboxExecutionPlan,
    ) -> Result<Self, ToolRuntimeAuthorizationErrorV1> {
        let value = Self { operation, binding, plan };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ToolRuntimeAuthorizationErrorV1> {
        self.binding.validate()?;
        self.operation.validate()?;
        if self.operation.fingerprint()? != self.binding.operation_fingerprint
            || self.operation.tool_batch_id != self.binding.tool_batch_id
            || self.operation.tool_call_id != self.binding.tool_call_id
            || self.operation.source != self.binding.source
            || self.operation.source_security_fingerprint
                != self.binding.source_security_fingerprint
            || self.operation.execution_plan_hash != self.binding.execution_plan_hash
            || self.operation.runtime_binding_hash != self.binding.runtime_binding_hash
            || !operation_identity_matches_invocation(
                &self.binding.operation_identity,
                &self.operation.invocation_identity,
            )
            || self.operation.sandbox_mode != Some(self.binding.selected_mode)
            || self.operation.network_access != self.binding.network_access
            || !sandbox_plan_matches_operation(&self.plan, &self.operation, &self.binding)?
            || self.plan.execution_id != self.binding.sandbox_execution_id
            || self.plan.identity != self.binding.operation_identity
            || self.plan.permission_snapshot_id != self.binding.permission_snapshot.id
            || self.plan.mode != self.binding.selected_mode
            || self.plan.network_access != self.binding.network_access
            || self.plan.network_enforcement != self.binding.network_enforcement
            || self.plan.plan_hash != self.binding.sandbox_plan_hash
        {
            return Err(ToolRuntimeAuthorizationErrorV1::DispatchPlanMismatch);
        }
        Ok(())
    }

    pub fn aura_stdio_dispatch_bootstrap(
        &self,
    ) -> Result<Option<AuraStdioDispatchBootstrapV1>, ToolRuntimeAuthorizationErrorV1> {
        self.validate()?;
        let NonSecretToolRuntimeFactsV1::Sandbox { facts } = &self.operation.runtime else {
            return Ok(None);
        };
        if facts.runtime_kind != ToolPreparedRuntimeKindV1::AuraStdio {
            return Ok(None);
        }
        if !facts.aura_stdio_bootstrap.as_ref().is_some_and(AuraStdioBootstrapPolicyV1::is_frozen) {
            return Err(ToolRuntimeAuthorizationErrorV1::DispatchBindingInvalid);
        }
        if !facts
            .aura_stdio_output
            .as_ref()
            .is_some_and(AuraStdioOutputProvisionPolicyV1::is_frozen)
        {
            return Err(ToolRuntimeAuthorizationErrorV1::DispatchBindingInvalid);
        }
        let output_root = self
            .binding
            .attempt_output_root
            .clone()
            .ok_or(ToolRuntimeAuthorizationErrorV1::DispatchBindingInvalid)?;
        let output_authorization = self
            .binding
            .runtime_output_authorization
            .clone()
            .ok_or(ToolRuntimeAuthorizationErrorV1::DispatchBindingInvalid)?;
        Ok(Some(AuraStdioDispatchBootstrapV1 {
            tool_call_id: self.binding.tool_call_id,
            tool_execution_attempt_id: self.binding.tool_execution_attempt_id,
            output_root,
            output_authorization,
        }))
    }
}

impl<'de> Deserialize<'de> for AuthorizedToolSandboxExecutionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireExecution {
            operation: W7ToolOperationFingerprintPreimageV1,
            binding: ToolSandboxDispatchBindingV1,
            plan: SandboxExecutionPlan,
        }
        let wire = WireExecution::deserialize(deserializer)?;
        Self::new(wire.operation, wire.binding, wire.plan).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolRuntimeAuthorizationErrorV1 {
    #[error("the exact Tool source binding is inconsistent")]
    SourceMismatch,
    #[error("the non-secret Runtime facts are invalid")]
    RuntimeFactsInvalid,
    #[error("the W7 operation fingerprint preimage is invalid")]
    PreimageInvalid,
    #[error("the Tool Sandbox dispatch binding is invalid")]
    DispatchBindingInvalid,
    #[error("the authorized dispatch does not match the frozen Sandbox plan")]
    DispatchPlanMismatch,
    #[error("the authorization material cannot be represented as canonical JSON")]
    Canonicalization,
}

fn has_duplicate_environment(environment: &[SanitizedEnvironmentEntry]) -> bool {
    let mut names =
        environment.iter().map(|entry| entry.name.to_ascii_uppercase()).collect::<Vec<_>>();
    names.sort_unstable();
    names.windows(2).any(|pair| pair[0] == pair[1])
}

fn has_attempt_local_environment(environment: &[SanitizedEnvironmentEntry]) -> bool {
    environment.iter().any(|entry| {
        entry.name.eq_ignore_ascii_case("TEMP") || entry.name.eq_ignore_ascii_case("TMP")
    })
}

fn has_aura_stdio_bootstrap_environment(environment: &[SanitizedEnvironmentEntry]) -> bool {
    environment.iter().any(|entry| {
        entry.name.eq_ignore_ascii_case(AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1)
            || entry
                .name
                .eq_ignore_ascii_case(AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1)
            || entry.name.eq_ignore_ascii_case(AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1)
    })
}

fn dispatch_output_binding_is_valid(binding: &ToolSandboxDispatchBindingV1) -> bool {
    match (&binding.runtime_output_authorization, &binding.attempt_output_root) {
        (None, None) => true,
        (Some(authorization), Some(root)) => {
            authorization.validate().is_ok()
                && authorization.tool_call_id == binding.tool_call_id
                && authorization.tool_execution_attempt_id == binding.tool_execution_attempt_id
                && !root.display.is_empty()
                && root.display.encode_utf16().eq(root.utf16.iter().copied())
        }
        _ => false,
    }
}

fn operation_identity_matches_invocation(
    identity: &InvocationIdentity,
    invocation: &CoreInvocationIdentityV1,
) -> bool {
    identity.session_id == Some(invocation.session_id)
        && identity.turn_task_id == Some(invocation.turn_task_id)
        && identity.agent_run_id == Some(invocation.agent_run_id)
}

fn sandbox_plan_matches_operation(
    plan: &SandboxExecutionPlan,
    operation: &W7ToolOperationFingerprintPreimageV1,
    binding: &ToolSandboxDispatchBindingV1,
) -> Result<bool, ToolRuntimeAuthorizationErrorV1> {
    let NonSecretToolRuntimeFactsV1::Sandbox { facts } = &operation.runtime else {
        return Ok(false);
    };
    let persistent_environment = plan
        .environment
        .iter()
        .filter(|entry| {
            !entry.name.eq_ignore_ascii_case("TEMP")
                && !entry.name.eq_ignore_ascii_case("TMP")
                && !entry.name.eq_ignore_ascii_case("LOCALAPPDATA")
        })
        .cloned()
        .collect::<Vec<_>>();
    if has_duplicate_environment(&persistent_environment)
        || plan.environment.iter().filter(|entry| entry.name.eq_ignore_ascii_case("TEMP")).count()
            > 1
        || plan.environment.iter().filter(|entry| entry.name.eq_ignore_ascii_case("TMP")).count()
            > 1
        || plan
            .environment
            .iter()
            .filter(|entry| entry.name.eq_ignore_ascii_case("LOCALAPPDATA"))
            .count()
            > 1
    {
        return Err(ToolRuntimeAuthorizationErrorV1::DispatchPlanMismatch);
    }
    let mut expected_filesystem_grants = facts.filesystem_grants.clone();
    if let Some(output_root) = &binding.attempt_output_root {
        expected_filesystem_grants.push(SandboxFilesystemGrant {
            path: output_root.clone(),
            identity: None,
            access: crate::SandboxFilesystemAccess::ReadWrite,
            recursive: true,
        });
    }
    Ok(plan.executable == facts.executable
        && plan.args == facts.arguments
        && plan.working_directory == facts.working_directory
        && persistent_environment == facts.environment
        && plan.filesystem_grants == expected_filesystem_grants
        && plan.network_grant_ids.is_empty()
        && plan.timeout_ms == facts.timeout_ms
        && plan.output_limits == facts.output_limits)
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{
        AgentRunId, CancellationId, CommandInvocationId, CorrelationId, FileIdentity, ModelStepId,
        PermissionSnapshotId, PrincipalId, Revision, SessionId, TurnTaskId, WorkspaceId,
    };
    use aura_domain_common::{ToolId, ToolInstanceId, ToolRegistrySnapshotId};
    use aura_tool_contract::{
        SignedBuiltinToolContractFingerprintV1, ToolDefinitionSnapshotRef, ToolRegistrySnapshotRef,
        ToolSourceContractFingerprintV1, ToolSourceKindV1, ToolVersion,
    };
    use serde_json::json;

    use super::*;
    use aura_security_contract::{InvocationActor, PermissionMode};

    struct Fixture {
        operation: W7ToolOperationFingerprintPreimageV1,
        identity: InvocationIdentity,
        snapshot: PermissionSnapshot,
        plan: SandboxExecutionPlan,
    }

    fn fixture() -> Fixture {
        let workspace_id = WorkspaceId::from_bytes([1; 16]);
        let session_id = SessionId::from_bytes([2; 16]);
        let turn_task_id = TurnTaskId::from_bytes([3; 16]);
        let agent_run_id = AgentRunId::from_bytes([4; 16]);
        let correlation_id = CorrelationId::from_bytes([5; 16]);
        let snapshot = PermissionSnapshot {
            id: PermissionSnapshotId::from_bytes([6; 16]),
            session_id,
            workspace_id,
            mode: PermissionMode::FullAccess,
            session_permission_revision: 7,
            active_grant_ids: vec![],
            created_at: "2026-09-05T00:00:00Z".parse().expect("timestamp"),
        };
        let registry_snapshot_ref = ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::from_bytes([8; 16]),
            registry_revision: Revision::new(9),
            content_hash: CanonicalHashV1::sha256(b"registry"),
        };
        let source = ToolSecuritySourceRefV1::new(
            ToolDefinitionSnapshotRef {
                tool_id: ToolId::from_bytes([10; 16]),
                tool_version: ToolVersion::new(None, Revision::new(11)).expect("tool version"),
                registry_snapshot_ref,
            },
            ToolInstanceId::from_bytes([12; 16]),
            ToolSourceKindV1::BuiltIn,
            CanonicalHashV1::sha256(b"definition"),
            ToolSourceContractFingerprintV1::BuiltIn {
                signed_builtin_tool_contract_fingerprint:
                    SignedBuiltinToolContractFingerprintV1::new(CanonicalHashV1::sha256(
                        b"builtin contract",
                    )),
            },
        )
        .expect("source");
        let invocation_identity = CoreInvocationIdentityV1 {
            command_invocation_id: CommandInvocationId::from_bytes([13; 16]),
            session_id,
            turn_task_id,
            agent_run_id,
            model_step_id: ModelStepId::from_bytes([14; 16]),
        };
        let identity = InvocationIdentity {
            principal_id: PrincipalId::from_bytes([15; 16]),
            actor: InvocationActor::ParentAgent,
            workspace_id: Some(workspace_id),
            session_id: Some(session_id),
            turn_task_id: Some(turn_task_id),
            agent_run_id: Some(agent_run_id),
            parent_agent_run_id: None,
            correlation_id,
            permission_snapshot_id: Some(snapshot.id),
        };
        let executable = CanonicalExecutableRef {
            path: CanonicalPath {
                display: r"C:\fixture\tool.exe".to_owned(),
                utf16: r"C:\fixture\tool.exe".encode_utf16().collect(),
            },
            identity: FileIdentity {
                volume_serial_number: "volume".to_owned(),
                file_id: "file".to_owned(),
            },
            content_hash: ContentHash::sha256(b"executable"),
        };
        let working_directory = CanonicalPath {
            display: r"C:\fixture".to_owned(),
            utf16: r"C:\fixture".encode_utf16().collect(),
        };
        let runtime = NonSecretToolRuntimeFactsV1::Sandbox {
            facts: Box::new(NonSecretSandboxRuntimeFactsV1 {
                runtime_kind: ToolPreparedRuntimeKindV1::DeclarativeCommandArgv,
                executable: executable.clone(),
                arguments: vec![WindowsOsString { utf16: "--fixture".encode_utf16().collect() }],
                working_directory: working_directory.clone(),
                environment: vec![],
                filesystem_grants: vec![],
                credential: Some(ToolCredentialUseV1 {
                    credential_id: CredentialId::from_bytes([16; 16]),
                    credential_revision: CredentialRevision::new(Revision::new(2))
                        .expect("credential revision"),
                    slot_key: CredentialSlotKey::new("fixture").expect("slot"),
                    placement: ToolCredentialPlacementV1::Environment {
                        environment_variable: EnvironmentVariableNameV1::new("AURA_FIXTURE_SECRET")
                            .expect("environment"),
                    },
                }),
                success_exit_codes: vec![0],
                stdio_protocol_version: None,
                aura_stdio_bootstrap: None,
                aura_stdio_output: None,
                timeout_ms: 5_000,
                output_limits: ProcessOutputLimits {
                    stdout_bytes: 1_024,
                    stderr_bytes: 1_024,
                    combined_bytes: 2_048,
                },
            }),
        };
        let arguments = json!({"value": "fixture"});
        let operation = W7ToolOperationFingerprintPreimageV1::new(
            invocation_identity,
            ToolBatchId::from_bytes([17; 16]),
            ToolCallId::from_bytes([18; 16]),
            source,
            ContentHash::blake3(b"source security"),
            CanonicalHashV1::sha256(b"execution plan"),
            vec![ResolvedCapability::ProcessExecute],
            vec![ResolvedScope::WorkspaceRoot { workspace_id }],
            vec![],
            ToolPurposeTextV1::new("Run the fixture Tool").expect("purpose"),
            arguments.clone(),
            canonical_sha256(&arguments).expect("arguments hash"),
            runtime,
            Some(NativeSandboxMode::CompatibilityAppContainer),
            NetworkAccessRequirement::NotRequired,
        )
        .expect("operation");
        let mut plan = SandboxExecutionPlan {
            execution_id: SandboxExecutionId::from_bytes([19; 16]),
            identity: identity.clone(),
            permission_snapshot_id: snapshot.id,
            mode: NativeSandboxMode::CompatibilityAppContainer,
            network_access: NetworkAccessRequirement::NotRequired,
            network_enforcement: NetworkEnforcementKind::AppContainerNetworkDenied,
            executable,
            args: vec![WindowsOsString { utf16: "--fixture".encode_utf16().collect() }],
            working_directory,
            environment: vec![],
            filesystem_grants: vec![],
            network_grant_ids: vec![],
            timeout_ms: 5_000,
            output_limits: ProcessOutputLimits {
                stdout_bytes: 1_024,
                stderr_bytes: 1_024,
                combined_bytes: 2_048,
            },
            cancellation_id: CancellationId::from_bytes([20; 16]),
            plan_hash: ContentHash::blake3(b"sandbox plan"),
        };
        // The P04 Sandbox planner computes and freezes this value. Domain
        // validation binds it; the Host independently recomputes it.
        plan.plan_hash = ContentHash::blake3(b"sandbox plan");
        Fixture { operation, identity, snapshot, plan }
    }

    fn authorized_with_attempt(
        fixture: &Fixture,
        tool_execution_attempt_id: ToolExecutionAttemptId,
    ) -> AuthorizedToolSandboxExecutionV1 {
        let is_stdio = matches!(
            fixture.operation.runtime,
            NonSecretToolRuntimeFactsV1::Sandbox { ref facts }
                if facts.runtime_kind == ToolPreparedRuntimeKindV1::AuraStdio
        );
        let attempt_output_root = is_stdio.then(|| CanonicalPath {
            display: r"C:\Aura\attempt-output".to_owned(),
            utf16: r"C:\Aura\attempt-output".encode_utf16().collect(),
        });
        let runtime_output_authorization = is_stdio.then(|| {
            ToolRuntimeOutputAuthorizationV1::new(
                fixture.operation.tool_call_id,
                tool_execution_attempt_id,
            )
        });
        let mut plan = fixture.plan.clone();
        if let Some(root) = &attempt_output_root {
            plan.filesystem_grants.push(SandboxFilesystemGrant {
                path: root.clone(),
                identity: None,
                access: crate::SandboxFilesystemAccess::ReadWrite,
                recursive: true,
            });
        }
        let binding = ToolSandboxDispatchBindingV1::new(
            SecurityOperationId::from_bytes([21; 16]),
            fixture.identity.clone(),
            fixture.operation.fingerprint().expect("fingerprint"),
            fixture.snapshot.clone(),
            PermissionAllowAuthorizationBasisV1::PermissionMode { mode: fixture.snapshot.mode },
            fixture.operation.tool_batch_id,
            fixture.operation.tool_call_id,
            tool_execution_attempt_id,
            fixture.plan.execution_id,
            fixture.operation.execution_plan_hash.clone(),
            fixture.operation.runtime_binding_hash.clone(),
            fixture.operation.source.clone(),
            fixture.operation.source_security_fingerprint.clone(),
            fixture.plan.mode,
            fixture.plan.network_access,
            fixture.plan.network_enforcement,
            fixture.plan.plan_hash.clone(),
            runtime_output_authorization,
            attempt_output_root,
            "2026-09-05T00:00:01Z".parse().expect("prepared timestamp"),
        )
        .expect("binding");
        AuthorizedToolSandboxExecutionV1::new(fixture.operation.clone(), binding, plan)
            .expect("authorized execution")
    }

    fn authorized(fixture: &Fixture) -> AuthorizedToolSandboxExecutionV1 {
        authorized_with_attempt(fixture, ToolExecutionAttemptId::from_bytes([22; 16]))
    }

    fn stdio_fixture() -> Fixture {
        let mut value = fixture();
        let NonSecretToolRuntimeFactsV1::Sandbox { facts } = &mut value.operation.runtime else {
            panic!("sandbox runtime")
        };
        facts.runtime_kind = ToolPreparedRuntimeKindV1::AuraStdio;
        facts.success_exit_codes.clear();
        facts.stdio_protocol_version = Some(1);
        facts.aura_stdio_bootstrap = Some(AuraStdioBootstrapPolicyV1::frozen());
        facts.aura_stdio_output = Some(AuraStdioOutputProvisionPolicyV1::frozen());
        value.operation.runtime_binding_hash =
            value.operation.runtime.canonical_hash().expect("stdio runtime hash");
        value.operation.validate().expect("stdio operation");
        value
    }

    #[test]
    fn operation_fingerprint_binds_runtime_mode_and_credential_revision() {
        let original = fixture();
        let original_hash = original.operation.fingerprint().expect("fingerprint");

        let mut changed = original.operation.clone();
        changed.sandbox_mode = Some(NativeSandboxMode::StrictAppContainer);
        assert_ne!(changed.fingerprint().expect("changed mode"), original_hash);

        let mut changed = original.operation.clone();
        let NonSecretToolRuntimeFactsV1::Sandbox { facts } = &mut changed.runtime else {
            panic!("sandbox runtime")
        };
        facts.credential.as_mut().expect("credential").credential_revision =
            CredentialRevision::new(Revision::new(3)).expect("credential revision");
        changed.runtime_binding_hash = changed.runtime.canonical_hash().expect("runtime hash");
        assert_ne!(changed.fingerprint().expect("changed credential"), original_hash);
    }

    #[test]
    fn aura_stdio_requires_the_frozen_bootstrap_policy() {
        let original = fixture();
        let NonSecretToolRuntimeFactsV1::Sandbox { facts } = original.operation.runtime else {
            panic!("sandbox runtime")
        };
        let mut facts = *facts;
        facts.runtime_kind = ToolPreparedRuntimeKindV1::AuraStdio;
        facts.success_exit_codes.clear();
        facts.stdio_protocol_version = Some(1);
        assert_eq!(
            NonSecretToolRuntimeFactsV1::Sandbox { facts: Box::new(facts.clone()) }.validate(),
            Err(ToolRuntimeAuthorizationErrorV1::RuntimeFactsInvalid)
        );

        facts.aura_stdio_bootstrap = Some(AuraStdioBootstrapPolicyV1::frozen());
        facts.aura_stdio_output = Some(AuraStdioOutputProvisionPolicyV1::frozen());
        NonSecretToolRuntimeFactsV1::Sandbox { facts: Box::new(facts.clone()) }
            .validate()
            .expect("frozen Aura Stdio bootstrap policy");

        facts.runtime_kind = ToolPreparedRuntimeKindV1::DeclarativeCommandArgv;
        facts.success_exit_codes = vec![0];
        facts.stdio_protocol_version = None;
        assert_eq!(
            NonSecretToolRuntimeFactsV1::Sandbox { facts: Box::new(facts) }.validate(),
            Err(ToolRuntimeAuthorizationErrorV1::RuntimeFactsInvalid)
        );
    }

    #[test]
    fn runtime_facts_reject_bootstrap_name_injection_even_through_internal_entries() {
        let original = fixture();
        let NonSecretToolRuntimeFactsV1::Sandbox { facts } = original.operation.runtime else {
            panic!("sandbox runtime")
        };
        let mut facts = *facts;
        facts.environment.push(SanitizedEnvironmentEntry {
            name: "aura_tool_call_id".to_owned(),
            value: WindowsOsString { utf16: "forged".encode_utf16().collect() },
        });
        assert_eq!(
            NonSecretToolRuntimeFactsV1::Sandbox { facts: Box::new(facts) }.validate(),
            Err(ToolRuntimeAuthorizationErrorV1::RuntimeFactsInvalid)
        );
    }

    #[test]
    fn bootstrap_policy_deserialization_rejects_non_frozen_values() {
        let value =
            serde_json::to_value(AuraStdioBootstrapPolicyV1::frozen()).expect("serialize policy");
        serde_json::from_value::<AuraStdioBootstrapPolicyV1>(value.clone())
            .expect("frozen policy roundtrip");

        let mut changed = value;
        changed["policy_version"] = json!(2);
        assert!(serde_json::from_value::<AuraStdioBootstrapPolicyV1>(changed).is_err());
    }

    #[test]
    fn dispatch_bootstrap_uses_the_bound_call_and_attempt_identities() {
        let value = stdio_fixture();
        let first_attempt_id = ToolExecutionAttemptId::from_bytes([22; 16]);
        let second_attempt_id = ToolExecutionAttemptId::from_bytes([23; 16]);
        let first = authorized_with_attempt(&value, first_attempt_id);
        let second = authorized_with_attempt(&value, second_attempt_id);

        assert_eq!(
            first.operation.fingerprint().expect("first operation fingerprint"),
            second.operation.fingerprint().expect("second operation fingerprint")
        );
        assert_ne!(first.binding.binding_hash, second.binding.binding_hash);

        let first_bootstrap = first
            .aura_stdio_dispatch_bootstrap()
            .expect("bootstrap validation")
            .expect("Aura Stdio bootstrap");
        let second_bootstrap = second
            .aura_stdio_dispatch_bootstrap()
            .expect("bootstrap validation")
            .expect("Aura Stdio bootstrap");
        assert_eq!(first_bootstrap.tool_call_id(), value.operation.tool_call_id);
        assert_eq!(second_bootstrap.tool_call_id(), value.operation.tool_call_id);
        assert_eq!(first_bootstrap.tool_execution_attempt_id(), first_attempt_id);
        assert_eq!(second_bootstrap.tool_execution_attempt_id(), second_attempt_id);
    }

    #[test]
    fn authorized_execution_rejects_launch_mutation_after_approval() {
        let original = fixture();
        let authorized_execution = authorized(&original);
        authorized_execution.validate().expect("stable execution");

        let mut changed = authorized_execution.clone();
        changed.plan.args.push(WindowsOsString { utf16: "--changed".encode_utf16().collect() });
        assert_eq!(changed.validate(), Err(ToolRuntimeAuthorizationErrorV1::DispatchPlanMismatch));

        let mut changed = authorized_execution;
        changed.plan.mode = NativeSandboxMode::StrictAppContainer;
        assert_eq!(changed.validate(), Err(ToolRuntimeAuthorizationErrorV1::DispatchPlanMismatch));

        let mut changed = authorized(&original);
        changed.plan.filesystem_grants.push(SandboxFilesystemGrant {
            path: CanonicalPath {
                display: r"C:\changed".to_owned(),
                utf16: r"C:\changed".encode_utf16().collect(),
            },
            identity: None,
            access: crate::SandboxFilesystemAccess::ReadWrite,
            recursive: true,
        });
        assert_eq!(changed.validate(), Err(ToolRuntimeAuthorizationErrorV1::DispatchPlanMismatch));
    }

    #[test]
    fn dispatch_binding_rejects_an_unrelated_allow_basis() {
        let original = fixture();
        let mut binding = authorized(&original).binding;
        binding.authorization_basis =
            PermissionAllowAuthorizationBasisV1::PermissionMode { mode: PermissionMode::ReadOnly };
        assert_eq!(
            binding.validate(),
            Err(ToolRuntimeAuthorizationErrorV1::DispatchBindingInvalid)
        );
    }

    #[test]
    fn tampered_preimage_is_rejected_during_deserialization() {
        let original = fixture();
        let mut value = serde_json::to_value(&original.operation).expect("serialize");
        value["arguments"]["value"] = json!("changed");
        assert!(serde_json::from_value::<W7ToolOperationFingerprintPreimageV1>(value).is_err());
    }
}
