use aura_domain_common::ToolWireVersionV1;
use aura_execution_contract::SideEffectStateV1;

use aura_domain_common::{
    ArtifactId, ContentHash, ErrorCategory, ErrorCode, ErrorEnvelope, NormalizedIssueCode,
    UtcTimestamp,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use thiserror::Error;

use crate::{
    ToolDefinitionSnapshotRef, ToolExecutionAttemptStatusV1, ToolOutputSlotKeyV1,
    ToolPurposeTextV1, ToolSourceKindV1,
};
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{ToolBatchId, ToolCallId, ToolExecutionAttemptId, ToolInstanceId};

pub const TOOL_SIDE_EFFECT_UNVERIFIED_WARNING_V1: &str = "tool.side_effect.unverified";

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolNotExecutedReasonV1 {
    SchemaValidationFailed,
    ToolNotFound,
    VersionMismatch,
    NotLoaded,
    Disabled,
    TrustRequired,
    AudienceDenied,
    PurposeRequired,
    PermissionDenied,
    ApprovalDenied,
    RuntimeUnavailable,
    DependencyNotExecuted,
    AtomicGroupNotExecuted,
    LineageReconciliation,
    CompletionReview,
    CancelledBeforeDispatch,
    BatchRejected,
    ResourceClaimResolutionFailed,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolCancellationReasonV1 {
    UserRequested,
    TurnInterrupted,
    PermissionRevoked,
    ScopeClosed,
    CoreShutdown,
    Timeout,
    ParentAgentCancelled,
    BatchCancelled,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolIndeterminateReasonV1 {
    DispatchOutcomeUnknown,
    RuntimeTerminalMissing,
    RuntimeTerminationUnknown,
    ExternalEffectUnknown,
    ResultEvidenceInvalid,
    PersistenceOutcomeUnknown,
    ReconciliationInconclusive,
}

impl ToolIndeterminateReasonV1 {
    /// Lower values have higher priority when several facts are unresolved.
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            Self::ReconciliationInconclusive => 0,
            Self::PersistenceOutcomeUnknown => 1,
            Self::RuntimeTerminationUnknown => 2,
            Self::DispatchOutcomeUnknown => 3,
            Self::RuntimeTerminalMissing => 4,
            Self::ResultEvidenceInvalid => 5,
            Self::ExternalEffectUnknown => 6,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolResultOutcomeV1 {
    Succeeded,
    Failed {
        error: ErrorEnvelope,
    },
    /// Invocation returned normally; the requested operation was rejected.
    OperationRejected {
        error: ErrorEnvelope,
    },
    Cancelled {
        reason: ToolCancellationReasonV1,
    },
    NotExecuted {
        reason: ToolNotExecutedReasonV1,
        error: Option<ErrorEnvelope>,
    },
    Indeterminate {
        reason: ToolIndeterminateReasonV1,
        error: ErrorEnvelope,
    },
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolResultContractError {
    #[error("succeeded result must have no_effect, fully_applied, or unknown side effect state")]
    InvalidSucceededSideEffect,
    #[error("not_executed result must have not_started side effect state")]
    InvalidNotExecutedSideEffect,
    #[error("not_executed result cannot reference execution attempts")]
    NotExecutedHasAttempts,
    #[error("not_executed result cannot reference aggregate side-effect evidence")]
    NotExecutedHasAggregateEvidence,
    #[error("dispatched terminal result must reference at least one attempt")]
    DispatchedResultHasNoAttempts,
    #[error("dispatched terminal result must reference aggregate side-effect evidence")]
    DispatchedResultHasNoAggregateEvidence,
    #[error("tool result attempt references must be unique and ordered")]
    InvalidAttemptOrder,
    #[error("tool result timestamps are inconsistent")]
    InvalidTimestampOrder,
    #[error("model-visible and internal diagnostic artifacts must be distinct")]
    InvalidArtifactSeparation,
    #[error("succeeded result with unknown side effects must include the unverified warning")]
    MissingUnverifiedSideEffectWarning,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionAttemptOutcomeRefV1 {
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub attempt_number: u8,
    pub side_effect_state: SideEffectStateV1,
    pub side_effect_evidence_ref: Option<ArtifactId>,
    pub dispatched: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolResultV1 {
    pub tool_call_id: ToolCallId,
    pub outcome: ToolResultOutcomeV1,
    pub side_effect_state: SideEffectStateV1,
    pub attempts: Vec<ToolExecutionAttemptOutcomeRefV1>,
    pub model_visible_result_ref: Option<ArtifactId>,
    pub user_audit_result_ref: Option<ArtifactId>,
    pub internal_diagnostic_ref: Option<ArtifactId>,
    pub aggregate_side_effect_evidence_ref: Option<ArtifactId>,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: UtcTimestamp,
}

impl ToolResultV1 {
    pub fn validate(&self) -> Result<(), ToolResultContractError> {
        match self.outcome {
            ToolResultOutcomeV1::Succeeded
                if !matches!(
                    self.side_effect_state,
                    SideEffectStateV1::NoEffect
                        | SideEffectStateV1::FullyApplied
                        | SideEffectStateV1::Unassessed
                        | SideEffectStateV1::Unknown
                ) =>
            {
                return Err(ToolResultContractError::InvalidSucceededSideEffect);
            }
            ToolResultOutcomeV1::NotExecuted { .. }
                if self.side_effect_state != SideEffectStateV1::NotStarted =>
            {
                return Err(ToolResultContractError::InvalidNotExecutedSideEffect);
            }
            ToolResultOutcomeV1::NotExecuted { .. } if !self.attempts.is_empty() => {
                return Err(ToolResultContractError::NotExecutedHasAttempts);
            }
            ToolResultOutcomeV1::Succeeded
            | ToolResultOutcomeV1::Failed { .. }
            | ToolResultOutcomeV1::OperationRejected { .. }
            | ToolResultOutcomeV1::Cancelled { .. }
            | ToolResultOutcomeV1::Indeterminate { .. }
                if self.attempts.is_empty() =>
            {
                return Err(ToolResultContractError::DispatchedResultHasNoAttempts);
            }
            ToolResultOutcomeV1::Succeeded
            | ToolResultOutcomeV1::Failed { .. }
            | ToolResultOutcomeV1::OperationRejected { .. }
            | ToolResultOutcomeV1::Cancelled { .. }
            | ToolResultOutcomeV1::Indeterminate { .. }
                if self.aggregate_side_effect_evidence_ref.is_none() =>
            {
                return Err(ToolResultContractError::DispatchedResultHasNoAggregateEvidence);
            }
            ToolResultOutcomeV1::NotExecuted { .. }
                if self.aggregate_side_effect_evidence_ref.is_some() =>
            {
                return Err(ToolResultContractError::NotExecutedHasAggregateEvidence);
            }
            _ => {}
        }
        if !self.attempts.windows(2).all(|pair| {
            pair[0].attempt_number < pair[1].attempt_number
                && pair[0].tool_execution_attempt_id != pair[1].tool_execution_attempt_id
        }) || self
            .attempts
            .iter()
            .enumerate()
            .any(|(index, attempt)| attempt.attempt_number != u8::try_from(index + 1).unwrap_or(0))
        {
            return Err(ToolResultContractError::InvalidAttemptOrder);
        }
        if self.started_at.as_ref().is_some_and(|started| started > &self.completed_at) {
            return Err(ToolResultContractError::InvalidTimestampOrder);
        }
        if self.model_visible_result_ref.is_some()
            && self.model_visible_result_ref == self.internal_diagnostic_ref
        {
            return Err(ToolResultContractError::InvalidArtifactSeparation);
        }
        Ok(())
    }
}

/// Renderer/model-safe error surface. It deliberately excludes `details`,
/// causation chains, raw Runtime diagnostics, and Provider payloads.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSafeToolErrorV1 {
    pub code: ErrorCode,
    pub category: ErrorCategory,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelVisibleToolSuccessPayloadV1 {
    Unit,
    Utf8Text { text: String },
    StructuredJson { value: Value },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelVisibleToolResultOutcomeV1 {
    Succeeded { payload: ModelVisibleToolSuccessPayloadV1 },
    Failed { error: ModelSafeToolErrorV1 },
    OperationRejected { error: ModelSafeToolErrorV1 },
    Cancelled { reason: ToolCancellationReasonV1 },
    NotExecuted { reason: ToolNotExecutedReasonV1, error: Option<ModelSafeToolErrorV1> },
    Indeterminate { reason: ToolIndeterminateReasonV1, error: ModelSafeToolErrorV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelVisibleToolArtifactRefV1 {
    pub artifact_id: ArtifactId,
    pub slot: ToolOutputSlotKeyV1,
    pub media_type: String,
    pub byte_length: u64,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelVisibleToolResultEnvelopeV1 {
    pub schema_version: ToolWireVersionV1,
    pub tool_call_id: ToolCallId,
    /// Null only for a ToolNotFound call whose Provider name was absent from the frozen request.
    #[schemars(
        with = "aura_domain_common::nullable::Required<ToolDefinitionSnapshotRef>",
        required
    )]
    pub definition_ref: Option<ToolDefinitionSnapshotRef>,
    pub outcome: ModelVisibleToolResultOutcomeV1,
    pub side_effect_state: SideEffectStateV1,
    pub warnings: Vec<NormalizedIssueCode>,
    pub artifacts: Vec<ModelVisibleToolArtifactRefV1>,
    pub completed_at: UtcTimestamp,
}

impl ModelVisibleToolResultEnvelopeV1 {
    pub fn validate(&self) -> Result<(), ToolResultContractError> {
        if self.definition_ref.is_none()
            && (!matches!(self.outcome, ModelVisibleToolResultOutcomeV1::NotExecuted { .. })
                || self.side_effect_state != SideEffectStateV1::NotStarted
                || !self.artifacts.is_empty())
        {
            return Err(ToolResultContractError::InvalidNotExecutedSideEffect);
        }
        validate_visible_success_state_and_warning(
            matches!(self.outcome, ModelVisibleToolResultOutcomeV1::Succeeded { .. }),
            self.side_effect_state,
            &self.warnings,
        )
    }
}

impl<'de> Deserialize<'de> for ModelVisibleToolResultEnvelopeV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            schema_version: ToolWireVersionV1,
            tool_call_id: ToolCallId,
            #[serde(deserialize_with = "aura_domain_common::nullable::required")]
            definition_ref: Option<ToolDefinitionSnapshotRef>,
            outcome: ModelVisibleToolResultOutcomeV1,
            side_effect_state: SideEffectStateV1,
            warnings: Vec<NormalizedIssueCode>,
            artifacts: Vec<ModelVisibleToolArtifactRefV1>,
            completed_at: UtcTimestamp,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            schema_version: wire.schema_version,
            tool_call_id: wire.tool_call_id,
            definition_ref: wire.definition_ref,
            outcome: wire.outcome,
            side_effect_state: wire.side_effect_state,
            warnings: wire.warnings,
            artifacts: wire.artifacts,
            completed_at: wire.completed_at,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolResultOutcomeSummaryV1 {
    Succeeded,
    Failed,
    Cancelled { reason: ToolCancellationReasonV1 },
    NotExecuted { reason: ToolNotExecutedReasonV1 },
    Indeterminate { reason: ToolIndeterminateReasonV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserAuditAttemptSummaryV1 {
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub attempt_number: u8,
    pub status: ToolExecutionAttemptStatusV1,
    pub side_effect_state: SideEffectStateV1,
    pub started_at: UtcTimestamp,
    pub completed_at: Option<UtcTimestamp>,
    pub safe_error: Option<ModelSafeToolErrorV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserReadableToolArtifactView {
    pub artifact_id: ArtifactId,
    pub media_type: String,
    pub byte_length: u64,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserAuditToolResultEnvelopeV1 {
    pub schema_version: ToolWireVersionV1,
    pub tool_batch_id: ToolBatchId,
    pub tool_call_id: ToolCallId,
    pub ordinal: u32,
    pub definition_ref: ToolDefinitionSnapshotRef,
    pub tool_instance_id: ToolInstanceId,
    pub source: ToolSourceKindV1,
    pub purpose: Option<ToolPurposeTextV1>,
    pub arguments_hash: Option<CanonicalSha256HashV1>,
    pub outcome: ToolResultOutcomeSummaryV1,
    pub side_effect_state: SideEffectStateV1,
    pub warnings: Vec<NormalizedIssueCode>,
    pub attempts: Vec<UserAuditAttemptSummaryV1>,
    pub artifacts: Vec<UserReadableToolArtifactView>,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: UtcTimestamp,
}

impl UserAuditToolResultEnvelopeV1 {
    pub fn validate(&self) -> Result<(), ToolResultContractError> {
        validate_visible_success_state_and_warning(
            self.outcome == ToolResultOutcomeSummaryV1::Succeeded,
            self.side_effect_state,
            &self.warnings,
        )
    }
}

impl<'de> Deserialize<'de> for UserAuditToolResultEnvelopeV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            schema_version: ToolWireVersionV1,
            tool_batch_id: ToolBatchId,
            tool_call_id: ToolCallId,
            ordinal: u32,
            definition_ref: ToolDefinitionSnapshotRef,
            tool_instance_id: ToolInstanceId,
            source: ToolSourceKindV1,
            purpose: Option<ToolPurposeTextV1>,
            arguments_hash: Option<CanonicalSha256HashV1>,
            outcome: ToolResultOutcomeSummaryV1,
            side_effect_state: SideEffectStateV1,
            warnings: Vec<NormalizedIssueCode>,
            attempts: Vec<UserAuditAttemptSummaryV1>,
            artifacts: Vec<UserReadableToolArtifactView>,
            started_at: Option<UtcTimestamp>,
            completed_at: UtcTimestamp,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            schema_version: wire.schema_version,
            tool_batch_id: wire.tool_batch_id,
            tool_call_id: wire.tool_call_id,
            ordinal: wire.ordinal,
            definition_ref: wire.definition_ref,
            tool_instance_id: wire.tool_instance_id,
            source: wire.source,
            purpose: wire.purpose,
            arguments_hash: wire.arguments_hash,
            outcome: wire.outcome,
            side_effect_state: wire.side_effect_state,
            warnings: wire.warnings,
            attempts: wire.attempts,
            artifacts: wire.artifacts,
            started_at: wire.started_at,
            completed_at: wire.completed_at,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

fn validate_visible_success_state_and_warning(
    succeeded: bool,
    side_effect_state: SideEffectStateV1,
    warnings: &[NormalizedIssueCode],
) -> Result<(), ToolResultContractError> {
    if succeeded
        && !matches!(
            side_effect_state,
            SideEffectStateV1::NoEffect
                | SideEffectStateV1::FullyApplied
                | SideEffectStateV1::Unassessed
                | SideEffectStateV1::Unknown
        )
    {
        return Err(ToolResultContractError::InvalidSucceededSideEffect);
    }
    if succeeded
        && side_effect_state == SideEffectStateV1::Unknown
        && !warnings
            .iter()
            .any(|warning| warning.as_str() == TOOL_SIDE_EFFECT_UNVERIFIED_WARNING_V1)
    {
        return Err(ToolResultContractError::MissingUnverifiedSideEffectWarning);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{ArtifactId, NormalizedIssueCode, UtcTimestamp};

    use super::{
        ToolExecutionAttemptOutcomeRefV1, ToolNotExecutedReasonV1, ToolResultContractError,
        ToolResultOutcomeV1, ToolResultV1, validate_visible_success_state_and_warning,
    };
    use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
    use aura_execution_contract::SideEffectStateV1;

    #[test]
    fn not_executed_is_never_dispatched_and_has_no_attempt() {
        let result = ToolResultV1 {
            tool_call_id: ToolCallId::new(),
            outcome: ToolResultOutcomeV1::NotExecuted {
                reason: ToolNotExecutedReasonV1::SchemaValidationFailed,
                error: None,
            },
            side_effect_state: SideEffectStateV1::NotStarted,
            attempts: Vec::new(),
            model_visible_result_ref: None,
            user_audit_result_ref: None,
            internal_diagnostic_ref: None,
            aggregate_side_effect_evidence_ref: None,
            started_at: None,
            completed_at: UtcTimestamp::now(),
        };
        result.validate().expect("valid not-executed result");
    }

    #[test]
    fn succeeded_can_report_unknown_side_effects_with_durable_evidence() {
        let completed_at = UtcTimestamp::now();
        let result = ToolResultV1 {
            tool_call_id: ToolCallId::new(),
            outcome: ToolResultOutcomeV1::Succeeded,
            side_effect_state: SideEffectStateV1::Unknown,
            attempts: vec![ToolExecutionAttemptOutcomeRefV1 {
                tool_execution_attempt_id: ToolExecutionAttemptId::new(),
                attempt_number: 1,
                side_effect_state: SideEffectStateV1::Unknown,
                side_effect_evidence_ref: Some(ArtifactId::new()),
                dispatched: true,
            }],
            model_visible_result_ref: None,
            user_audit_result_ref: None,
            internal_diagnostic_ref: None,
            aggregate_side_effect_evidence_ref: Some(ArtifactId::new()),
            started_at: Some(completed_at),
            completed_at,
        };
        result.validate().expect("succeeded + unknown is a valid terminal result");
    }

    #[test]
    fn succeeded_requires_attempt_and_aggregate_side_effect_evidence() {
        let completed_at = UtcTimestamp::now();
        let result = ToolResultV1 {
            tool_call_id: ToolCallId::new(),
            outcome: ToolResultOutcomeV1::Succeeded,
            side_effect_state: SideEffectStateV1::NoEffect,
            attempts: Vec::new(),
            model_visible_result_ref: None,
            user_audit_result_ref: None,
            internal_diagnostic_ref: None,
            aggregate_side_effect_evidence_ref: None,
            started_at: Some(completed_at),
            completed_at,
        };
        assert_eq!(result.validate(), Err(ToolResultContractError::DispatchedResultHasNoAttempts));

        let evidence_ref = ArtifactId::new();
        let result = ToolResultV1 {
            attempts: vec![ToolExecutionAttemptOutcomeRefV1 {
                tool_execution_attempt_id: ToolExecutionAttemptId::new(),
                attempt_number: 1,
                side_effect_state: SideEffectStateV1::NoEffect,
                side_effect_evidence_ref: Some(ArtifactId::new()),
                dispatched: true,
            }],
            ..result
        };
        assert_eq!(
            result.validate(),
            Err(ToolResultContractError::DispatchedResultHasNoAggregateEvidence)
        );

        ToolResultV1 { aggregate_side_effect_evidence_ref: Some(evidence_ref), ..result }
            .validate()
            .expect("complete dispatched result");
    }

    #[test]
    fn safe_succeeded_unknown_requires_the_frozen_warning() {
        assert_eq!(
            validate_visible_success_state_and_warning(true, SideEffectStateV1::Unknown, &[]),
            Err(ToolResultContractError::MissingUnverifiedSideEffectWarning)
        );
        validate_visible_success_state_and_warning(
            true,
            SideEffectStateV1::Unknown,
            &[NormalizedIssueCode::new(super::TOOL_SIDE_EFFECT_UNVERIFIED_WARNING_V1)
                .expect("registered warning")],
        )
        .expect("warning makes the safe envelope explicit");
    }
}
