mod errors;
pub(crate) use errors::{ToolAttemptDispatchErrorV1, ToolAttemptPreDispatchErrorV1};
mod admission;

mod builtin_admission;
mod builtin_dispatch;
mod final_authority;
use final_authority::{
    dispatch_prepared_attempt, result_assembly_context, validate_final_authority_and_state,
    validate_plan_identity,
};
mod http_admission;
mod http_dispatch;

mod retry;
pub(crate) use retry::ToolCoreRetrySourceV1;
mod network_errors;
mod sandbox_admission;
mod sandbox_dispatch;
use network_errors::safe_network_gateway_error_code;
#[cfg(test)]
mod capacity_observer;
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{
    CausationId, CorrelationId, ErrorCategory, ErrorCode, ErrorEnvelope, EventId, UtcTimestamp,
};
use aura_domain_common::{ToolExecutionAttemptId, ToolInstanceId};
use aura_execution_contract::SideEffectStateV1;
use aura_ipc::ToolRuntimeSecretFrameV1;
use aura_network_control::{
    CoreHttpClientGateway, GatewayResponse, GatewayRetryAfterV1, ToolHttpGatewayResponseV1,
};
use aura_permission_domain::{
    CredentialReflectionScanEvidenceV1, NonSecretToolRuntimeFactsV1, ToolPreparedRuntimeKindV1,
};
use aura_persistence::{
    AuthorizedBuiltinCoreHandlerAttemptDispatchPreparedCommitRequestV1,
    AuthorizedToolExecutionAttemptDispatchPreparedCommitRequestV1, ExactToolExecutionAuthorityV1,
    PersistenceCoordinator, PersistenceError, ToolBatchAdmissionCommitRequest,
    ToolExecutionAttemptDispatchPreparedCommitRequest, ToolExecutionAttemptProgressRequest,
    ToolExecutionAttemptRuntimeStartedRequest, ToolExecutionPersistenceCommit,
    ToolRegistryBuildRequestV1,
};
use aura_tool_contract::{
    NormalizedToolCallInputStateV1, ToolBatchErrorCode, ToolBatchStatusV1, ToolCallStatusV1,
    ToolDefinitionSnapshotRef, ToolExecutionAttemptRecordV1, ToolExecutionAttemptStatusV1,
    ToolPurposeTextV1, ToolRegistrySnapshotV1, ToolResultV1, ToolRuntimeErrorCode,
    ToolSourceKindV1,
};
use aura_tool_domain::ToolExecutionSemanticsV1;
use aura_tool_projection::{
    ToolAttemptProgressView, ToolExecutionProjectionErrorV1, ToolExecutionProjectionStateV1,
};
use aura_tool_scheduler::{
    AdmittedToolBatchV1, PlannedToolBatchV1, PlannedToolRuntimeCallV1, ToolDispatchPermitV1,
    ToolExecutionSchedulerV1, ToolScheduleErrorV1, ToolScheduleRequestV1,
};
use thiserror::Error;
use tokio::sync::watch;
use zeroize::Zeroizing;

use crate::{
    p07::{AuthorizedToolSandboxOperationV1, ToolSandboxAssemblyErrorV1},
    p07_builtin::{
        AuthorizedToolBuiltinOperationV1, BuiltinCoreHandlerDispatchEvidenceV1,
        ToolBuiltinAssemblyErrorV1, ToolBuiltinHandlerErrorV1, invoke_builtin_core_handler_v1,
    },
    p07_http::{AuthorizedToolHttpOperationV1, ToolHttpAssemblyErrorV1},
    p07_output::{ToolOutputFailureClassV1, cleanup_output_root, import_stdio_attempt_outputs_v1},
    sidecars::{
        ToolCommandDispatchEvidenceV1, ToolRuntimeDispatchLifecycleUpdateV1,
        ToolSandboxExecutionError, ToolSandboxExecutionService, ToolSandboxHostPermit,
        ToolStdioDispatchEvidenceV1,
    },
};

pub(crate) use aura_execution_orchestrator::{
    BatchPreparationError as ToolBatchPreparationErrorV1,
    PrepareBatch as PrepareToolBatchExecutionInputV1,
};

/// Frozen inputs for the final W7 boundary. P04 has already returned Allow,
/// while no Runtime attempt, attempt-local directory, or secret material exists
/// yet.
pub(crate) struct BeginAuthorizedToolAttemptInputV1<'a> {
    pub admitted_batch: &'a AdmittedToolBatchV1,
    pub planned_batch: &'a PlannedToolBatchV1,
    pub planned_call: &'a PlannedToolRuntimeCallV1,
    pub authorized_operation: AuthorizedToolSandboxOperationV1,
    pub evaluated_authority: &'a ExactToolExecutionAuthorityV1,
    pub projection_state: ToolExecutionProjectionStateV1,
    pub admitted_event_id: EventId,
    pub attempt_temp_directory: std::path::PathBuf,
    pub correlation_id: CorrelationId,
}

/// Frozen HTTP inputs at the same final guard as the Sandbox path. The
/// approved operation still has no `tea_`; the real Attempt is created only
/// by the atomic pre-dispatch transaction below.
pub(crate) struct BeginAuthorizedToolHttpAttemptInputV1<'a> {
    pub admitted_batch: &'a AdmittedToolBatchV1,
    pub planned_batch: &'a PlannedToolBatchV1,
    pub planned_call: &'a PlannedToolRuntimeCallV1,
    pub authorized_operation: AuthorizedToolHttpOperationV1,
    pub evaluated_authority: &'a ExactToolExecutionAuthorityV1,
    pub projection_state: ToolExecutionProjectionStateV1,
    pub admitted_event_id: EventId,
    pub correlation_id: CorrelationId,
}

/// Frozen signed Built-in inputs at the same final guard as Sandbox and HTTP.
/// The optional Registry rebuild context is Core-owned and is required only
/// by `tools.load` / `tools.unload`.
pub(crate) struct BeginAuthorizedToolBuiltinAttemptInputV1<'a> {
    pub admitted_batch: &'a AdmittedToolBatchV1,
    pub planned_batch: &'a PlannedToolBatchV1,
    pub planned_call: &'a PlannedToolRuntimeCallV1,
    pub authorized_operation: AuthorizedToolBuiltinOperationV1,
    pub evaluated_authority: &'a ExactToolExecutionAuthorityV1,
    pub projection_state: ToolExecutionProjectionStateV1,
    pub admitted_event_id: EventId,
    pub registry_build_request: Option<ToolRegistryBuildRequestV1>,
    pub correlation_id: CorrelationId,
}

#[derive(Clone)]
pub(crate) struct ToolResultAssemblyContextV1 {
    pub session_id: aura_domain_common::SessionId,
    pub ordinal: u32,
    pub definition_ref: ToolDefinitionSnapshotRef,
    pub tool_instance_id: ToolInstanceId,
    pub source: ToolSourceKindV1,
    pub purpose: ToolPurposeTextV1,
    pub arguments_hash: CanonicalSha256HashV1,
}

/// A committed pre-dispatch boundary. Holding both permits keeps every
/// scheduler and Host-capacity constraint live until the Host supplies proven
/// process-tree terminal evidence.
pub(crate) struct CommittedAuthorizedToolAttemptV1 {
    pub tool_batch_id: aura_domain_common::ToolBatchId,
    pub attempt: ToolExecutionAttemptRecordV1,
    pub execution: aura_permission_domain::AuthorizedToolSandboxExecutionV1,
    pub projection_state: ToolExecutionProjectionStateV1,
    pub persistence: ToolExecutionPersistenceCommit,
    pub projection_publish_error: Option<PersistenceError>,
    pub scheduler_permit: ToolDispatchPermitV1,
    pub host_permit: ToolSandboxHostPermit,
    pub result_contract: aura_tool_contract::ToolResultContractV1,
    pub execution_semantics: ToolExecutionSemanticsV1,
    pub timeout_ms: u64,
    pub max_retries: u8,
    pub dispatch_prepared_event_id: EventId,
    pub correlation_id: CorrelationId,
    pub result_context: ToolResultAssemblyContextV1,
    pub attempt_output_root: Option<std::path::PathBuf>,
}

/// A committed HTTP attempt retains the same scheduler constraints as a
/// Sandbox attempt. No request has crossed the Gateway boundary yet.
pub(crate) struct CommittedAuthorizedToolHttpAttemptV1 {
    pub tool_batch_id: aura_domain_common::ToolBatchId,
    pub attempt: ToolExecutionAttemptRecordV1,
    pub gateway_request: aura_network_control::ToolHttpGatewayRequestV1,
    pub success_status_ranges: Vec<(u16, u16)>,
    pub projection_state: ToolExecutionProjectionStateV1,
    pub persistence: ToolExecutionPersistenceCommit,
    pub projection_publish_error: Option<PersistenceError>,
    pub scheduler_permit: ToolDispatchPermitV1,
    pub result_contract: aura_tool_contract::ToolResultContractV1,
    pub execution_semantics: ToolExecutionSemanticsV1,
    pub timeout_ms: u64,
    pub max_retries: u8,
    pub dispatch_prepared_event_id: EventId,
    pub correlation_id: CorrelationId,
    pub result_context: ToolResultAssemblyContextV1,
}

pub(crate) struct CommittedAuthorizedToolBuiltinAttemptV1 {
    pub tool_batch_id: aura_domain_common::ToolBatchId,
    pub attempt: ToolExecutionAttemptRecordV1,
    pub binding: aura_tool_registry::BuiltinCoreHandlerDispatchBindingV1,
    pub arguments: serde_json::Value,
    pub registry: ToolRegistrySnapshotV1,
    pub registry_build_request: Option<ToolRegistryBuildRequestV1>,
    pub projection_state: ToolExecutionProjectionStateV1,
    pub persistence: ToolExecutionPersistenceCommit,
    pub projection_publish_error: Option<PersistenceError>,
    pub scheduler_permit: ToolDispatchPermitV1,
    pub result_contract: aura_tool_contract::ToolResultContractV1,
    pub execution_semantics: ToolExecutionSemanticsV1,
    pub timeout_ms: u64,
    pub max_retries: u8,
    pub dispatch_prepared_event_id: EventId,
    pub correlation_id: CorrelationId,
    pub result_context: ToolResultAssemblyContextV1,
}

pub(crate) struct DispatchedAuthorizedBuiltinAttemptV1 {
    pub dispatched: DispatchedAuthorizedToolAttemptV1,
    pub attempt_side_effect: aura_execution_contract::VerifiedAttemptSideEffectEvidenceV1,
    pub call_side_effect_evidence: aura_tool_scheduler::CallSideEffectEvidenceV1,
}

pub(crate) struct ToolHttpDispatchEvidenceV1 {
    pub candidate: aura_tool_runtime::ToolRuntimeCandidateV1,
    /// Response Header values are intentionally excluded from durable
    /// evidence. The Gateway already redacts controlled Header values, while
    /// Core needs only the names to prove response-shape handling.
    pub response_header_names: Vec<String>,
    pub final_origin: Option<aura_permission_domain::NormalizedOrigin>,
    pub final_destination: Option<aura_security_contract::ResolvedNetworkDestination>,
    pub redirect_hops: Option<u8>,
    /// Stable P07 code only. Raw reqwest, DNS, OS, URL and credential errors
    /// are never copied into the attempt Artifact.
    pub gateway_error_code: Option<&'static str>,
    pub credential_reflection: Option<CredentialReflectionScanEvidenceV1>,
    /// Only Core can construct this closed evidence from the P04 Gateway's
    /// typed result. Runtime, Package, model, and Renderer inputs never carry
    /// a retryable flag into the retry Gate.
    pub retry_source: Option<ToolCoreRetrySourceV1>,
}

pub(crate) enum ToolRuntimeDispatchEvidenceV1 {
    BuiltinCoreHandler(BuiltinCoreHandlerDispatchEvidenceV1),
    Command(ToolCommandDispatchEvidenceV1),
    AuraStdio(ToolStdioDispatchEvidenceV1),
    DeclarativeHttp(ToolHttpDispatchEvidenceV1),
}

/// Candidate Runtime evidence after the short-lived Host operation has
/// reached a reported terminal. The scheduler permit is deliberately retained
/// until Core-side result, side-effect, and process-tree reconciliation has
/// finished.
pub(crate) struct DispatchedAuthorizedToolAttemptV1 {
    pub tool_batch_id: aura_domain_common::ToolBatchId,
    pub attempt: ToolExecutionAttemptRecordV1,
    pub projection_state: ToolExecutionProjectionStateV1,
    pub evidence: ToolRuntimeDispatchEvidenceV1,
    pub pre_dispatch_persistence: ToolExecutionPersistenceCommit,
    pub runtime_started_persistence: Option<ToolExecutionPersistenceCommit>,
    pub progress_persistence: Vec<ToolExecutionPersistenceCommit>,
    pub projection_publish_errors: Vec<PersistenceError>,
    pub runtime_started_error: Option<String>,
    pub scheduler_permit: ToolDispatchPermitV1,
    pub result_contract: aura_tool_contract::ToolResultContractV1,
    pub execution_semantics: ToolExecutionSemanticsV1,
    pub timeout_ms: u64,
    pub max_retries: u8,
    pub dispatch_prepared_event_id: EventId,
    pub correlation_id: CorrelationId,
    pub result_context: ToolResultAssemblyContextV1,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ToolAttemptPreDispatchServiceV1;

#[cfg(any(test, feature = "p08-contract-driver"))]
#[cfg_attr(not(test), allow(dead_code, unused_imports))]
pub(crate) mod tests;

pub(crate) struct PreparedToolBatchExecutionV1 {
    pub admitted: AdmittedToolBatchV1,
    pub planned: PlannedToolBatchV1,
    pub projection_state: ToolExecutionProjectionStateV1,
    commit_request: ToolBatchAdmissionCommitRequest,
}

pub(crate) enum ToolBatchPreparationOutcomeV1 {
    /// No valid Aura ToolCall identity set exists, so no synthetic Result may
    /// be created. P08 must move the current ModelStep/TurnTask to recovery.
    IdentityIntegrityFault {
        code: ToolBatchErrorCode,
    },
    /// Identity is unique, so every Provider call can receive its stable
    /// batch_rejected Result without creating a Runtime attempt.
    BatchRejected {
        code: ToolBatchErrorCode,
        results: Vec<ToolResultV1>,
    },
    Prepared(Box<PreparedToolBatchExecutionV1>),
}

pub(crate) struct CommittedToolBatchExecutionV1 {
    pub admitted: AdmittedToolBatchV1,
    pub planned: PlannedToolBatchV1,
    pub projection_state: ToolExecutionProjectionStateV1,
    pub persistence: ToolExecutionPersistenceCommit,
    /// Publication occurs only after the SQLite transaction commits. A
    /// publication failure must never make the caller replay admission.
    pub projection_publish_error: Option<PersistenceError>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ToolBatchAdmissionServiceV1;
