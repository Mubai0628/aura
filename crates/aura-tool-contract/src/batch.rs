use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use aura_domain_common::{
    AgentRunId, ArtifactId, CommandInvocationId, ModelStepId, Revision, SessionId, TurnTaskId,
    UtcTimestamp,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use thiserror::Error;

use crate::{
    MAX_MAX_CONCURRENT_TOOL_CALLS, MAX_TOOL_RETRIES, MAX_TOOL_TIMEOUT_MS,
    MIN_MAX_CONCURRENT_TOOL_CALLS, MIN_TOOL_TIMEOUT_MS, ResolvedResourceClaimV1,
    ToolDefinitionSnapshotRef, ToolRegistrySnapshotRef, ToolResultV1,
};
use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{ToolBatchId, ToolCallId, ToolExecutionAttemptId};
use aura_execution_contract::SideEffectStateV1;

pub const MAX_TOOL_PURPOSE_BYTES: usize = 4_096;
pub const MAX_TOOL_EXECUTION_ATTEMPTS: u8 = MAX_TOOL_RETRIES + 1;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolPurposeTextError {
    #[error("tool purpose must contain 1..=4096 UTF-8 bytes after line-ending normalization")]
    InvalidLength,
    #[error("tool purpose contains a prohibited control character")]
    ProhibitedControl,
    #[error("tool purpose must not be whitespace-only")]
    WhitespaceOnly,
}

#[derive(Clone, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolPurposeTextV1(#[schemars(with = "String")] String);

impl ToolPurposeTextV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolPurposeTextError> {
        let value = value.into().replace("\r\n", "\n").replace('\r', "\n");
        if value.is_empty() || value.len() > MAX_TOOL_PURPOSE_BYTES {
            return Err(ToolPurposeTextError::InvalidLength);
        }
        if value.chars().any(|character| {
            character == '\u{007f}'
                || (character <= '\u{001f}' && !matches!(character, '\n' | '\t'))
        }) {
            return Err(ToolPurposeTextError::ProhibitedControl);
        }
        if value.chars().all(char::is_whitespace) {
            return Err(ToolPurposeTextError::WhitespaceOnly);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ToolPurposeTextV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ToolPurposeTextV1").field(&self.0).finish()
    }
}

impl fmt::Display for ToolPurposeTextV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ToolPurposeTextV1 {
    type Err = ToolPurposeTextError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for ToolPurposeTextV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedToolCallInputIssueV1 {
    /// The Provider argument string was not a complete JSON value.
    ArgumentsJsonInvalid,
    /// The same object key appeared twice. The key is named by
    /// [`NormalizedToolCallInputDiagnosticV1::fields`], because "invalid JSON" alone
    /// sends the model looking for a syntax error it did not make.
    ArgumentsJsonDuplicateKey,
    EnvelopeRootInvalid,
    EnvelopeShapeInvalid,
    /// Envelopes 1-2: a top-level field other than `_aura` and `arguments`. Envelope 3:
    /// the earlier `_aura` wrapper, whose fields belong at the top level.
    EnvelopeUnexpectedField,
    AuraMetadataMissing,
    /// `_aura` is present but is not an object.
    AuraMetadataShapeInvalid,
    /// `_aura` carried a field other than `purpose` — the failure the Shell calls
    /// actually hit, where the tool's own fields were nested inside the metadata.
    AuraMetadataUnexpectedField,
    /// Kept for stored values written before the finer issues existed. New
    /// normalization reports [`Self::AuraMetadataShapeInvalid`] or
    /// [`Self::AuraMetadataUnexpectedField`] instead.
    AuraMetadataInvalid,
    /// `_aura.purpose` (envelopes 1-2) or `aura_purpose` (envelope 3) is missing.
    PurposeMissing,
    PurposeInvalid,
    /// The purpose text was longer than the contract allows.
    PurposeTooLong,
    ArgumentsMissing,
    /// `arguments` is present but is not an object.
    ArgumentsRootInvalid,
}

/// Where a normalization issue was found, in a form that is safe to show the model.
///
/// It names structure, never content: a pointer inside the argument envelope and the
/// offending *field names*. The argument text itself is never copied here — the
/// corrective message is stored with the Tool result and rendered in the session, and
/// neither may echo a script, a credential or the whole envelope. Both members are
/// bounded by the producer.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedToolCallInputDiagnosticV1 {
    /// Where the issue is. `$` is the envelope root; `/_aura` and `/_aura/<name>` locate
    /// the metadata of envelopes 1-2, and `/aura_purpose` the purpose of envelope 3.
    pub pointer: String,
    /// The offending field names, in the stable order the strict parser keeps object
    /// keys in — the diagnostic names them, it does not reproduce the envelope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
}

impl NormalizedToolCallInputDiagnosticV1 {
    #[must_use]
    pub fn new(pointer: impl Into<String>, fields: Vec<String>) -> Self {
        Self { pointer: pointer.into(), fields }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NormalizedToolCallInputStateV1 {
    Ready {
        purpose: ToolPurposeTextV1,
        arguments: Value,
        arguments_hash: CanonicalHashV1,
    },
    InvalidInput {
        issue: NormalizedToolCallInputIssueV1,
        /// Absent for a value written before the diagnostic existed, and for an issue
        /// whose location is the whole envelope.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<NormalizedToolCallInputDiagnosticV1>,
    },
}

impl NormalizedToolCallInputStateV1 {
    /// The invalid state for `issue`, with no field-level detail.
    #[must_use]
    pub const fn invalid_input(issue: NormalizedToolCallInputIssueV1) -> Self {
        Self::InvalidInput { issue, detail: None }
    }

    /// The invalid state for `issue`, with the bounded location it was found at.
    #[must_use]
    pub fn invalid_input_at(
        issue: NormalizedToolCallInputIssueV1,
        detail: NormalizedToolCallInputDiagnosticV1,
    ) -> Self {
        Self::InvalidInput { issue, detail: Some(detail) }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreInvocationIdentityV1 {
    pub command_invocation_id: CommandInvocationId,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub model_step_id: ModelStepId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedToolCallV1 {
    pub tool_call_id: ToolCallId,
    pub ordinal: u32,
    /// None only for a Provider name absent from the frozen request. Such a call
    /// can only become ToolNotFound, never an executable operation.
    #[serde(deserialize_with = "aura_domain_common::nullable::required")]
    #[schemars(
        with = "aura_domain_common::nullable::Required<ToolDefinitionSnapshotRef>",
        required
    )]
    pub definition_ref: Option<ToolDefinitionSnapshotRef>,
    pub input: NormalizedToolCallInputStateV1,
    pub provider_arguments_envelope_hash: CanonicalHashV1,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchRequestV1 {
    pub tool_batch_id: ToolBatchId,
    pub batch_content_hash: CanonicalHashV1,
    pub invocation_identity: CoreInvocationIdentityV1,
    pub p08_state_revision: Revision,
    pub p08_state_hash: CanonicalHashV1,
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub calls: Vec<NormalizedToolCallV1>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolBatchHashMaterial<'a> {
    tool_batch_id: ToolBatchId,
    invocation_identity: &'a CoreInvocationIdentityV1,
    p08_state_revision: Revision,
    p08_state_hash: &'a CanonicalHashV1,
    registry_snapshot_ref: &'a ToolRegistrySnapshotRef,
    calls: &'a [NormalizedToolCallV1],
}

impl ToolBatchRequestV1 {
    pub fn new(
        tool_batch_id: ToolBatchId,
        invocation_identity: CoreInvocationIdentityV1,
        p08_state_revision: Revision,
        p08_state_hash: CanonicalHashV1,
        registry_snapshot_ref: ToolRegistrySnapshotRef,
        calls: Vec<NormalizedToolCallV1>,
    ) -> Result<Self, ToolBatchContractError> {
        if calls.is_empty() || !calls.windows(2).all(|pair| pair[0].ordinal < pair[1].ordinal) {
            return Err(ToolBatchContractError::InvalidOrdinalOrder);
        }
        let mut request = Self {
            tool_batch_id,
            batch_content_hash: CanonicalHashV1::sha256(&[]),
            invocation_identity,
            p08_state_revision,
            p08_state_hash,
            registry_snapshot_ref,
            calls,
        };
        request.batch_content_hash = request.compute_hash()?;
        Ok(request)
    }

    pub fn validate_hash(&self) -> Result<(), ToolBatchContractError> {
        if self.compute_hash()? != self.batch_content_hash {
            return Err(ToolBatchContractError::ContentHashMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub fn has_duplicate_tool_call_id(&self) -> bool {
        let mut ids = self.calls.iter().map(|call| call.tool_call_id).collect::<Vec<_>>();
        ids.sort_unstable();
        ids.windows(2).any(|pair| pair[0] == pair[1])
    }

    fn compute_hash(&self) -> Result<CanonicalHashV1, ToolBatchContractError> {
        canonical_sha256(&ToolBatchHashMaterial {
            tool_batch_id: self.tool_batch_id,
            invocation_identity: &self.invocation_identity,
            p08_state_revision: self.p08_state_revision,
            p08_state_hash: &self.p08_state_hash,
            registry_snapshot_ref: &self.registry_snapshot_ref,
            calls: &self.calls,
        })
        .map_err(|_| ToolBatchContractError::ContentHashMismatch)
    }
}

impl<'de> Deserialize<'de> for ToolBatchRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireBatch {
            tool_batch_id: ToolBatchId,
            batch_content_hash: CanonicalHashV1,
            invocation_identity: CoreInvocationIdentityV1,
            p08_state_revision: Revision,
            p08_state_hash: CanonicalHashV1,
            registry_snapshot_ref: ToolRegistrySnapshotRef,
            calls: Vec<NormalizedToolCallV1>,
        }

        let wire = WireBatch::deserialize(deserializer)?;
        let value = Self {
            tool_batch_id: wire.tool_batch_id,
            batch_content_hash: wire.batch_content_hash,
            invocation_identity: wire.invocation_identity,
            p08_state_revision: wire.p08_state_revision,
            p08_state_hash: wire.p08_state_hash,
            registry_snapshot_ref: wire.registry_snapshot_ref,
            calls: wire.calls,
        };
        if value.calls.is_empty()
            || !value.calls.windows(2).all(|pair| pair[0].ordinal < pair[1].ordinal)
        {
            return Err(D::Error::custom(ToolBatchContractError::InvalidOrdinalOrder));
        }
        value.validate_hash().map_err(D::Error::custom)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolBatchContractError {
    #[error("tool batch calls must be non-empty and strictly ordered by original ordinal")]
    InvalidOrdinalOrder,
    #[error("tool batch content hash does not match its immutable content")]
    ContentHashMismatch,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolBatchStatusV1 {
    PendingAdmission,
    Admitted,
    Running,
    Cancelling,
    Completed,
    RecoveryRequired,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatusV1 {
    PendingAdmission,
    WaitingDependency,
    WaitingPermission,
    WaitingApproval,
    WaitingResource,
    Queued,
    Running,
    Cancelling,
    Finalized,
    RecoveryRequired,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionAttemptStatusV1 {
    Started,
    DispatchPrepared,
    Running,
    WaitingExternal,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    Indeterminate,
}

impl ToolExecutionAttemptStatusV1 {
    #[must_use]
    pub const fn occupies_global_concurrency_slot(self) -> bool {
        matches!(
            self,
            Self::Started
                | Self::DispatchPrepared
                | Self::Running
                | Self::WaitingExternal
                | Self::Cancelling
        )
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled | Self::Indeterminate)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionPlanCallV1 {
    pub tool_call_id: ToolCallId,
    pub ordinal: u32,
    pub dependency_tool_call_ids: Vec<ToolCallId>,
    pub execution_atomic_group: Option<u32>,
    pub resolved_resource_claims: Vec<ResolvedResourceClaimV1>,
    pub timeout_ms: u64,
    pub max_retries: u8,
    pub local_concurrency_limit: Option<u32>,
    pub entity_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionPlanV1 {
    pub tool_batch_id: ToolBatchId,
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub tool_settings_revision: Revision,
    pub tool_settings_hash: CanonicalHashV1,
    pub calls: Vec<ToolExecutionPlanCallV1>,
    pub execution_plan_hash: CanonicalHashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolExecutionPlanHashMaterial<'a> {
    tool_batch_id: ToolBatchId,
    registry_snapshot_ref: &'a ToolRegistrySnapshotRef,
    tool_settings_revision: Revision,
    tool_settings_hash: &'a CanonicalHashV1,
    calls: &'a [ToolExecutionPlanCallV1],
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolExecutionPlanContractError {
    #[error("execution plan calls must be ordered by original ordinal")]
    InvalidCallOrder,
    #[error("execution plan contains a duplicate ToolCallId")]
    DuplicateToolCallId,
    #[error("execution plan dependency references are not canonical or form a cycle")]
    InvalidDependencyGraph,
    #[error("execution plan resource claims are not in canonical lock order")]
    InvalidResourceClaimOrder,
    #[error("execution plan timeout, retry, concurrency, or entity revision is invalid")]
    InvalidExecutionPolicy,
    #[error("execution plan hash does not match its immutable contents")]
    HashMismatch,
}

impl ToolExecutionPlanV1 {
    pub fn new(
        tool_batch_id: ToolBatchId,
        registry_snapshot_ref: ToolRegistrySnapshotRef,
        tool_settings_revision: Revision,
        tool_settings_hash: CanonicalHashV1,
        calls: Vec<ToolExecutionPlanCallV1>,
    ) -> Result<Self, ToolExecutionPlanContractError> {
        let mut plan = Self {
            tool_batch_id,
            registry_snapshot_ref,
            tool_settings_revision,
            tool_settings_hash,
            calls,
            execution_plan_hash: CanonicalHashV1::sha256(&[]),
        };
        plan.validate_without_hash()?;
        plan.execution_plan_hash = plan.compute_hash()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), ToolExecutionPlanContractError> {
        self.validate_without_hash()?;
        if self.compute_hash()? != self.execution_plan_hash {
            return Err(ToolExecutionPlanContractError::HashMismatch);
        }
        Ok(())
    }

    fn validate_without_hash(&self) -> Result<(), ToolExecutionPlanContractError> {
        if !self.calls.windows(2).all(|pair| pair[0].ordinal < pair[1].ordinal) {
            return Err(ToolExecutionPlanContractError::InvalidCallOrder);
        }
        let call_ids = self.calls.iter().map(|call| call.tool_call_id).collect::<BTreeSet<_>>();
        if call_ids.len() != self.calls.len() {
            return Err(ToolExecutionPlanContractError::DuplicateToolCallId);
        }
        if self.tool_settings_revision.value() == 0
            || self.calls.iter().any(|call| {
                !(MIN_TOOL_TIMEOUT_MS..=MAX_TOOL_TIMEOUT_MS).contains(&call.timeout_ms)
                    || call.max_retries > MAX_TOOL_RETRIES
                    || call.local_concurrency_limit.is_some_and(|limit| {
                        !(MIN_MAX_CONCURRENT_TOOL_CALLS..=MAX_MAX_CONCURRENT_TOOL_CALLS)
                            .contains(&limit)
                    })
                    || call.entity_revision.value() == 0
            })
        {
            return Err(ToolExecutionPlanContractError::InvalidExecutionPolicy);
        }
        for call in &self.calls {
            if !call.dependency_tool_call_ids.windows(2).all(|pair| pair[0] < pair[1])
                || call.dependency_tool_call_ids.iter().any(|dependency| {
                    !call_ids.contains(dependency) || *dependency == call.tool_call_id
                })
            {
                return Err(ToolExecutionPlanContractError::InvalidDependencyGraph);
            }
            if !call.resolved_resource_claims.windows(2).all(|pair| {
                pair[0].canonical_lock_key.as_bytes() < pair[1].canonical_lock_key.as_bytes()
            }) {
                return Err(ToolExecutionPlanContractError::InvalidResourceClaimOrder);
            }
        }
        if dependency_graph_has_cycle(&self.calls) {
            return Err(ToolExecutionPlanContractError::InvalidDependencyGraph);
        }
        Ok(())
    }

    fn compute_hash(&self) -> Result<CanonicalHashV1, ToolExecutionPlanContractError> {
        canonical_sha256(&ToolExecutionPlanHashMaterial {
            tool_batch_id: self.tool_batch_id,
            registry_snapshot_ref: &self.registry_snapshot_ref,
            tool_settings_revision: self.tool_settings_revision,
            tool_settings_hash: &self.tool_settings_hash,
            calls: &self.calls,
        })
        .map_err(|_| ToolExecutionPlanContractError::HashMismatch)
    }
}

fn dependency_graph_has_cycle(calls: &[ToolExecutionPlanCallV1]) -> bool {
    let graph = calls
        .iter()
        .map(|call| (call.tool_call_id, call.dependency_tool_call_ids.as_slice()))
        .collect::<BTreeMap<_, _>>();
    let mut permanently_visited = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    graph.keys().copied().any(|tool_call_id| {
        visit_dependency(tool_call_id, &graph, &mut visiting, &mut permanently_visited)
    })
}

fn visit_dependency(
    tool_call_id: ToolCallId,
    graph: &BTreeMap<ToolCallId, &[ToolCallId]>,
    visiting: &mut BTreeSet<ToolCallId>,
    permanently_visited: &mut BTreeSet<ToolCallId>,
) -> bool {
    if permanently_visited.contains(&tool_call_id) {
        return false;
    }
    if !visiting.insert(tool_call_id) {
        return true;
    }
    if graph[&tool_call_id]
        .iter()
        .copied()
        .any(|dependency| visit_dependency(dependency, graph, visiting, permanently_visited))
    {
        return true;
    }
    visiting.remove(&tool_call_id);
    permanently_visited.insert(tool_call_id);
    false
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionAttemptRecordV1 {
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub tool_call_id: ToolCallId,
    pub attempt_number: u8,
    pub status: ToolExecutionAttemptStatusV1,
    pub side_effect_state: SideEffectStateV1,
    pub evidence_ref: Option<ArtifactId>,
    pub started_at: UtcTimestamp,
    pub dispatch_prepared_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
    pub last_progress_sequence: u64,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolExecutionAttemptContractError {
    #[error("tool execution attempt number is outside the frozen retry bound")]
    InvalidAttemptNumber,
    #[error("tool execution attempt timestamps do not match its lifecycle state")]
    InvalidTimestamps,
    #[error("tool execution attempt terminal evidence is missing or inconsistent")]
    InvalidTerminalEvidence,
    #[error("tool execution attempt side-effect state is inconsistent with dispatch")]
    InvalidSideEffectState,
}

impl ToolExecutionAttemptRecordV1 {
    pub fn validate(&self) -> Result<(), ToolExecutionAttemptContractError> {
        if self.attempt_number == 0 || self.attempt_number > MAX_TOOL_EXECUTION_ATTEMPTS {
            return Err(ToolExecutionAttemptContractError::InvalidAttemptNumber);
        }
        if self.dispatch_prepared_at.as_ref().is_some_and(|prepared| prepared < &self.started_at)
            || self.completed_at.as_ref().is_some_and(|completed| completed < &self.started_at)
            || self
                .dispatch_prepared_at
                .as_ref()
                .zip(self.completed_at.as_ref())
                .is_some_and(|(prepared, completed)| completed < prepared)
        {
            return Err(ToolExecutionAttemptContractError::InvalidTimestamps);
        }
        if self.status.is_terminal() {
            if self.dispatch_prepared_at.is_none()
                || self.completed_at.is_none()
                || self.evidence_ref.is_none()
            {
                return Err(ToolExecutionAttemptContractError::InvalidTerminalEvidence);
            }
            if self.side_effect_state == SideEffectStateV1::NotStarted
                || self.status == ToolExecutionAttemptStatusV1::Succeeded
                    && !matches!(
                        self.side_effect_state,
                        SideEffectStateV1::NoEffect
                            | SideEffectStateV1::FullyApplied
                            | SideEffectStateV1::Unassessed
                            | SideEffectStateV1::Unknown
                    )
            {
                return Err(ToolExecutionAttemptContractError::InvalidSideEffectState);
            }
            return Ok(());
        }
        if self.completed_at.is_some() || self.evidence_ref.is_some() {
            return Err(ToolExecutionAttemptContractError::InvalidTerminalEvidence);
        }
        match self.status {
            ToolExecutionAttemptStatusV1::Started
                if self.dispatch_prepared_at.is_none()
                    && self.side_effect_state == SideEffectStateV1::NotStarted => {}
            ToolExecutionAttemptStatusV1::DispatchPrepared
                if self.dispatch_prepared_at.is_some()
                    && self.side_effect_state == SideEffectStateV1::NotStarted => {}
            ToolExecutionAttemptStatusV1::Running
            | ToolExecutionAttemptStatusV1::WaitingExternal
            | ToolExecutionAttemptStatusV1::Cancelling
                if self.dispatch_prepared_at.is_some()
                    && self.side_effect_state != SideEffectStateV1::NotStarted => {}
            _ => return Err(ToolExecutionAttemptContractError::InvalidSideEffectState),
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchResultV1 {
    pub tool_batch_id: ToolBatchId,
    pub status: ToolBatchStatusV1,
    pub results: Vec<ToolResultV1>,
    pub completed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolBatchResultContractError {
    #[error("a ToolBatch result must be terminal and stably paired by original ordinal")]
    InvalidBatchResult,
    #[error("a ToolBatch contains an invalid ToolResult")]
    InvalidToolResult,
    #[error("a ToolResult completed after its containing ToolBatch")]
    InvalidCompletionTime,
}

impl ToolBatchResultV1 {
    #[must_use]
    pub fn is_stably_ordered(&self, request: &ToolBatchRequestV1) -> bool {
        self.results.len() == request.calls.len()
            && self
                .results
                .iter()
                .zip(&request.calls)
                .all(|(result, call)| result.tool_call_id == call.tool_call_id)
    }

    pub fn validate(
        &self,
        request: &ToolBatchRequestV1,
    ) -> Result<(), ToolBatchResultContractError> {
        if self.tool_batch_id != request.tool_batch_id
            || self.status != ToolBatchStatusV1::Completed
            || !self.is_stably_ordered(request)
        {
            return Err(ToolBatchResultContractError::InvalidBatchResult);
        }
        for result in &self.results {
            result.validate().map_err(|_| ToolBatchResultContractError::InvalidToolResult)?;
            if result.completed_at > self.completed_at {
                return Err(ToolBatchResultContractError::InvalidCompletionTime);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{ArtifactId, Revision, UtcTimestamp};

    use super::{
        NormalizedToolCallInputDiagnosticV1, NormalizedToolCallInputIssueV1,
        NormalizedToolCallInputStateV1, ToolExecutionAttemptContractError,
        ToolExecutionAttemptRecordV1, ToolExecutionAttemptStatusV1, ToolExecutionPlanCallV1,
        ToolExecutionPlanContractError, ToolExecutionPlanV1, ToolPurposeTextV1,
    };
    use crate::ToolRegistrySnapshotRef;
    use aura_domain_common::jcs::CanonicalHashV1;
    use aura_domain_common::{
        ToolBatchId, ToolCallId, ToolExecutionAttemptId, ToolRegistrySnapshotId,
    };
    use aura_execution_contract::SideEffectStateV1;

    #[test]
    fn purpose_normalizes_line_endings_but_does_not_trim() {
        let purpose = ToolPurposeTextV1::new("  inspect\r\nthis  ").expect("purpose");
        assert_eq!(purpose.as_str(), "  inspect\nthis  ");
        assert!(ToolPurposeTextV1::new(" \t\n ").is_err());
        assert!(ToolPurposeTextV1::new("bad\0value").is_err());
    }

    #[test]
    fn invalid_provider_input_is_explicit_not_nullable() {
        let state = NormalizedToolCallInputStateV1::InvalidInput {
            issue: NormalizedToolCallInputIssueV1::PurposeMissing,
            detail: None,
        };
        assert_eq!(serde_json::to_value(state).expect("wire")["type"], "invalid_input");
    }

    /// A value written before the diagnostic existed still reads.
    #[test]
    fn an_invalid_input_without_a_diagnostic_reads_and_stays_readable() {
        let stored = serde_json::json!({
            "type": "invalid_input",
            "issue": "aura_metadata_invalid",
        });
        let state: NormalizedToolCallInputStateV1 =
            serde_json::from_value(stored).expect("a predecessor value must stay readable");
        assert_eq!(
            state,
            NormalizedToolCallInputStateV1::InvalidInput {
                issue: NormalizedToolCallInputIssueV1::AuraMetadataInvalid,
                detail: None,
            }
        );
        assert_eq!(
            serde_json::to_value(state).expect("wire"),
            serde_json::json!({ "type": "invalid_input", "issue": "aura_metadata_invalid" }),
            "an absent diagnostic is not written back as null"
        );
    }

    #[test]
    fn an_issue_that_names_fields_carries_them_through_the_wire() {
        let state = NormalizedToolCallInputStateV1::invalid_input_at(
            NormalizedToolCallInputIssueV1::AuraMetadataUnexpectedField,
            NormalizedToolCallInputDiagnosticV1::new(
                "/_aura",
                vec!["limit".to_owned(), "path".to_owned()],
            ),
        );
        let wire = serde_json::to_value(&state).expect("wire");
        assert_eq!(wire["issue"], "aura_metadata_unexpected_field");
        assert_eq!(wire["detail"]["pointer"], "/_aura");
        assert_eq!(wire["detail"]["fields"][0], "limit");
        assert_eq!(
            serde_json::from_value::<NormalizedToolCallInputStateV1>(wire).expect("round trip"),
            state
        );
    }

    fn plan_calls() -> Vec<ToolExecutionPlanCallV1> {
        vec![
            ToolExecutionPlanCallV1 {
                tool_call_id: ToolCallId::from_bytes([1; 16]),
                ordinal: 1,
                dependency_tool_call_ids: Vec::new(),
                execution_atomic_group: None,
                resolved_resource_claims: Vec::new(),
                timeout_ms: 120_000,
                max_retries: 3,
                local_concurrency_limit: None,
                entity_revision: Revision::INITIAL,
            },
            ToolExecutionPlanCallV1 {
                tool_call_id: ToolCallId::from_bytes([2; 16]),
                ordinal: 2,
                dependency_tool_call_ids: vec![ToolCallId::from_bytes([1; 16])],
                execution_atomic_group: None,
                resolved_resource_claims: Vec::new(),
                timeout_ms: 120_000,
                max_retries: 3,
                local_concurrency_limit: Some(2),
                entity_revision: Revision::INITIAL,
            },
        ]
    }

    fn execution_plan(
        calls: Vec<ToolExecutionPlanCallV1>,
    ) -> Result<ToolExecutionPlanV1, ToolExecutionPlanContractError> {
        ToolExecutionPlanV1::new(
            ToolBatchId::new(),
            ToolRegistrySnapshotRef {
                tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                registry_revision: Revision::INITIAL,
                content_hash: CanonicalHashV1::sha256(b"registry"),
            },
            Revision::INITIAL,
            CanonicalHashV1::sha256(b"settings"),
            calls,
        )
    }

    #[test]
    fn execution_plan_hash_and_dependency_graph_are_validated() {
        let plan = execution_plan(plan_calls()).expect("plan");
        plan.validate().expect("valid plan");

        let mut tampered = plan.clone();
        tampered.calls[0].timeout_ms += 1;
        assert_eq!(tampered.validate(), Err(ToolExecutionPlanContractError::HashMismatch));

        let mut cyclic = plan_calls();
        cyclic[0].dependency_tool_call_ids = vec![ToolCallId::from_bytes([2; 16])];
        assert_eq!(
            execution_plan(cyclic),
            Err(ToolExecutionPlanContractError::InvalidDependencyGraph)
        );
    }

    #[test]
    fn execution_plan_rejects_invalid_frozen_policy() {
        let mut calls = plan_calls();
        calls[0].timeout_ms = 999;
        assert_eq!(
            execution_plan(calls),
            Err(ToolExecutionPlanContractError::InvalidExecutionPolicy)
        );
    }

    #[test]
    fn execution_attempt_requires_dispatch_and_terminal_evidence() {
        let started_at = UtcTimestamp::now();
        let mut attempt = ToolExecutionAttemptRecordV1 {
            tool_execution_attempt_id: ToolExecutionAttemptId::new(),
            tool_call_id: ToolCallId::new(),
            attempt_number: 1,
            status: ToolExecutionAttemptStatusV1::Started,
            side_effect_state: SideEffectStateV1::NotStarted,
            evidence_ref: None,
            started_at,
            dispatch_prepared_at: None,
            completed_at: None,
            last_progress_sequence: 0,
        };
        attempt.validate().expect("started attempt");

        attempt.status = ToolExecutionAttemptStatusV1::Running;
        attempt.dispatch_prepared_at = Some(started_at);
        assert_eq!(
            attempt.validate(),
            Err(ToolExecutionAttemptContractError::InvalidSideEffectState)
        );
        attempt.status = ToolExecutionAttemptStatusV1::Started;
        attempt.dispatch_prepared_at = None;

        attempt.status = ToolExecutionAttemptStatusV1::Succeeded;
        attempt.side_effect_state = SideEffectStateV1::FullyApplied;
        attempt.dispatch_prepared_at = Some(started_at);
        attempt.completed_at = Some(started_at);
        assert_eq!(
            attempt.validate(),
            Err(ToolExecutionAttemptContractError::InvalidTerminalEvidence)
        );

        attempt.evidence_ref = Some(ArtifactId::new());
        attempt.validate().expect("terminal attempt");
        attempt.side_effect_state = SideEffectStateV1::Unknown;
        attempt.validate().expect("a succeeded Attempt may retain unknown side-effect evidence");
    }
}
