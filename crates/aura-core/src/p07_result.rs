use std::collections::BTreeMap;

use aura_artifact_contract::{
    ArtifactAccess, ArtifactAccessLinkV1, ArtifactAccessSubjectV1, ArtifactOwnerRef, ArtifactScope,
    ToolArtifactOwnerRef,
};
use aura_context_domain::ToolResultContextProjectionV1;
use aura_context_runtime::project_tool_result_context_v1;
use aura_domain_common::ToolWireVersionV1;
use aura_domain_common::{CausationId, ErrorEnvelope, NormalizedIssueCode, UtcTimestamp};
use aura_execution_contract::{SideEffectStateV1, VerifiedAttemptSideEffectEvidenceV1};
use aura_persistence::{
    ArtifactWriteDraftV1, ArtifactWriteRequest, PersistenceCoordinator, PersistenceError,
    ToolExecutionAttemptFinalizationRequest, ToolExecutionAttemptRetryableFailureRequest,
    ToolExecutionAttemptTerminalFact, ToolExecutionPersistenceCommit,
};
use aura_tool_contract::{
    ModelSafeToolErrorV1, ModelVisibleToolArtifactRefV1, ModelVisibleToolResultEnvelopeV1,
    ModelVisibleToolResultOutcomeV1, ModelVisibleToolSuccessPayloadV1,
    TOOL_SIDE_EFFECT_UNVERIFIED_WARNING_V1, ToolCancellationReasonV1,
    ToolExecutionAttemptOutcomeRefV1, ToolExecutionAttemptRecordV1, ToolExecutionAttemptStatusV1,
    ToolResultOutcomeSummaryV1, ToolResultOutcomeV1, ToolResultV1, UserAuditAttemptSummaryV1,
    UserAuditToolResultEnvelopeV1, UserReadableToolArtifactView,
};
use aura_tool_projection::{ToolExecutionProjectionErrorV1, ToolExecutionProjectionStateV1};
use aura_tool_runtime::{
    ToolAttemptTerminalDecisionV1, ToolRuntimeCandidateV1, ToolRuntimeEvaluationErrorV1,
    ValidatedToolResultBodyV1, evaluate_tool_runtime_candidate_v1,
};
use aura_tool_scheduler::{
    AttemptSideEffectFactV1, CallSideEffectEvidenceV1, ReconciledAttemptSideEffectV1,
    SideEffectAggregationError, ToolRetryAssessmentError, ToolRetryAssessmentV1,
    ToolRetryDecisionV1, aggregate_side_effect_state, assess_failed_attempt_retry,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use thiserror::Error;

use crate::p07_execution::{DispatchedAuthorizedToolAttemptV1, ToolRuntimeDispatchEvidenceV1};

pub(crate) struct CompleteDispatchedToolAttemptInputV1 {
    pub dispatched: DispatchedAuthorizedToolAttemptV1,
    /// Core-verified facts for this exact `tea_`. Runtime output alone cannot
    /// construct this value or select the SideEffect state.
    pub attempt_side_effect: VerifiedAttemptSideEffectEvidenceV1,
    /// Optional deterministic reconciliation for earlier Unknown attempts.
    pub reconciliations: Vec<AttemptReconciliationInputV1>,
    /// Trusted call-level postcondition facts. The service creates and binds
    /// the InternalOnly aggregate evidence Artifact itself.
    pub call_side_effect_evidence: CallSideEffectEvidenceV1,
    pub cancellation_intent: Option<ToolCancellationReasonV1>,
    pub completed_at: UtcTimestamp,
}

pub(crate) struct AttemptReconciliationInputV1 {
    pub tool_execution_attempt_id: aura_domain_common::ToolExecutionAttemptId,
    pub reconciliation: ReconciledAttemptSideEffectV1,
}

// Both variants intentionally carry their complete, exact persistence outcome so callers cannot
// accidentally mix retry and terminal projection facts.
#[allow(clippy::large_enum_variant)]
pub(crate) enum CompleteDispatchedToolAttemptOutcomeV1 {
    RetryScheduled {
        attempt: aura_tool_contract::ToolExecutionAttemptRecordV1,
        projection_state: ToolExecutionProjectionStateV1,
        persistence: ToolExecutionPersistenceCommit,
        projection_publish_error: Option<PersistenceError>,
        next_attempt_number: u8,
        /// Gateway-parsed delay, capped to the remaining frozen attempt
        /// timeout. P08 may schedule it but cannot replace it with raw Header
        /// or Runtime data.
        retry_after_ms: Option<u64>,
    },
    Finalized {
        result: ToolResultV1,
        context_projection: ToolResultContextProjectionV1,
        projection_state: ToolExecutionProjectionStateV1,
        persistence: ToolExecutionPersistenceCommit,
        projection_publish_error: Option<PersistenceError>,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ToolAttemptCompletionServiceV1;

impl ToolAttemptCompletionServiceV1 {
    /// Reconciles one Host candidate against the exact ToolVersion contract,
    /// persists full InternalOnly Runtime evidence, and either records a
    /// retryable failed Attempt or atomically finalizes the logical ToolResult.
    /// SideEffect facts remain explicit trusted inputs and are never inferred
    /// from process exit, HTTP status, or Stdio claims.
    pub fn complete(
        self,
        persistence: &PersistenceCoordinator,
        input: CompleteDispatchedToolAttemptInputV1,
    ) -> Result<CompleteDispatchedToolAttemptOutcomeV1, ToolAttemptCompletionErrorV1> {
        let CompleteDispatchedToolAttemptInputV1 {
            mut dispatched,
            attempt_side_effect,
            reconciliations,
            mut call_side_effect_evidence,
            cancellation_intent,
            completed_at,
        } = input;
        if call_side_effect_evidence.aggregate_evidence_ref.is_some() {
            return Err(ToolAttemptCompletionErrorV1::CallerSuppliedAggregateArtifact);
        }
        attempt_side_effect
            .validate()
            .map_err(|_| ToolAttemptCompletionErrorV1::AttemptSideEffectEvidenceInvalid)?;
        if attempt_side_effect.tool_execution_attempt_id()
            != dispatched.attempt.tool_execution_attempt_id
        {
            return Err(ToolAttemptCompletionErrorV1::AttemptSideEffectEvidenceInvalid);
        }

        let trusted_retry_source = dispatch_retry_source(&dispatched.evidence);
        let candidate = dispatch_candidate(&dispatched.evidence).clone();
        let attempt_evidence_artifact = ArtifactWriteDraftV1::new(
            ArtifactWriteRequest {
                scope: ArtifactScope::ToolExecution,
                media_type: "application/vnd.aura.tool-attempt-evidence.v1+json".to_owned(),
                access: ArtifactAccess::NONE,
                owner: ArtifactOwnerRef::Tool {
                    owner: ToolArtifactOwnerRef::ExecutionAttempt {
                        tool_execution_attempt_id: dispatched.attempt.tool_execution_attempt_id,
                    },
                },
                bytes: runtime_evidence_bytes(
                    &dispatched.evidence,
                    &attempt_side_effect,
                    dispatched.runtime_started_error.as_deref(),
                )?,
            },
            completed_at,
        )?;
        let attempt_evidence = attempt_evidence_artifact.record()?;
        let mut evaluated = evaluate_tool_runtime_candidate_v1(
            candidate,
            &dispatched.result_contract,
            &attempt_side_effect,
            cancellation_intent,
            dispatched.correlation_id,
        )?;
        if let ToolAttemptTerminalDecisionV1::Failed { error } = &mut evaluated.terminal {
            // Runtime and external ErrorEnvelope flags are deliberately
            // discarded by aura-tool-runtime. Only this Core-owned closed
            // classification can mark the durable failure retryable.
            error.retryable = trusted_retry_source.is_some();
        }
        dispatched.attempt.status = terminal_status(&evaluated.terminal);
        dispatched.attempt.side_effect_state = evaluated.side_effect_state;
        dispatched.attempt.evidence_ref = Some(attempt_evidence.artifact_id);
        dispatched.attempt.completed_at = Some(completed_at);
        dispatched.attempt.validate().map_err(|_| ToolAttemptCompletionErrorV1::AttemptInvalid)?;

        if let ToolAttemptTerminalDecisionV1::Failed { error } = &evaluated.terminal {
            let retry = assess_failed_attempt_retry(ToolRetryAssessmentV1 {
                error,
                trusted_source_retryable: trusted_retry_source.is_some(),
                execution_semantics: &dispatched.execution_semantics,
                side_effect_state: dispatched.attempt.side_effect_state,
                attempt_number: dispatched.attempt.attempt_number,
                max_retries: dispatched.max_retries,
                cancellation_requested: cancellation_intent.is_some(),
            })?;
            if let ToolRetryDecisionV1::Retry { next_attempt_number } = retry {
                let projection =
                    dispatched.projection_state.failed_for_retry(&dispatched.attempt, error)?;
                let commit = persistence.commit_tool_execution_attempt_failed_for_retry(
                    ToolExecutionAttemptRetryableFailureRequest {
                        attempt: dispatched.attempt.clone(),
                        error: error.clone(),
                        attempt_projection: projection,
                        attempt_evidence_artifact,
                        causation_id: CausationId::from_ulid(
                            dispatched.dispatch_prepared_event_id.ulid(),
                        ),
                        correlation_id: dispatched.correlation_id,
                    },
                )?;
                let projection_publish_error =
                    persistence.publish_tool_projection_changes(&commit.projection_changes).err();
                let retry_after_ms = trusted_retry_source.and_then(|source| {
                    capped_retry_after_ms(
                        source.retry_after_ms(),
                        dispatched.attempt.started_at,
                        completed_at,
                        dispatched.timeout_ms,
                    )
                });
                return Ok(CompleteDispatchedToolAttemptOutcomeV1::RetryScheduled {
                    attempt: dispatched.attempt,
                    projection_state: dispatched.projection_state,
                    persistence: commit,
                    projection_publish_error,
                    next_attempt_number,
                    retry_after_ms,
                });
            }
        }

        let records = persistence.load_tool_execution_attempt_records_v1(
            dispatched.tool_batch_id,
            dispatched.attempt.tool_call_id,
        )?;
        let (attempt_facts, attempt_records, started_at) =
            attempt_facts(records, &dispatched.attempt, reconciliations)?;
        let aggregate_side_effect_evidence_artifact = ArtifactWriteDraftV1::new(
            ArtifactWriteRequest {
                scope: ArtifactScope::ToolExecution,
                media_type: "application/vnd.aura.tool-side-effect-aggregate.v1+json".to_owned(),
                access: ArtifactAccess::NONE,
                owner: ArtifactOwnerRef::Tool {
                    owner: ToolArtifactOwnerRef::ToolCall {
                        tool_call_id: dispatched.attempt.tool_call_id,
                    },
                },
                bytes: aggregate_evidence_bytes(&attempt_facts, &call_side_effect_evidence)?,
            },
            completed_at,
        )?;
        let aggregate_evidence = aggregate_side_effect_evidence_artifact.record()?;
        call_side_effect_evidence.aggregate_evidence_ref = Some(aggregate_evidence.artifact_id);
        let aggregate = aggregate_side_effect_state(&attempt_facts, &call_side_effect_evidence)?;
        let attempts = attempt_facts.iter().map(|fact| fact.attempt.clone()).collect::<Vec<_>>();
        let warnings = result_warnings(&evaluated.terminal, aggregate.state)?;
        let verified_outputs =
            if matches!(evaluated.terminal, ToolAttemptTerminalDecisionV1::Succeeded { .. }) {
                dispatch_output_artifacts(&dispatched.evidence)
            } else {
                &[]
            };
        let model_output_artifacts = verified_outputs
            .iter()
            .map(|output| ModelVisibleToolArtifactRefV1 {
                artifact_id: output.artifact.artifact_id,
                slot: output.slot.clone(),
                media_type: output.media_type.clone(),
                byte_length: output.artifact.byte_length,
                content_hash: output.artifact.content_hash.clone(),
            })
            .collect::<Vec<_>>();
        let user_output_artifacts = verified_outputs
            .iter()
            .map(|output| UserReadableToolArtifactView {
                artifact_id: output.artifact.artifact_id,
                media_type: output.media_type.clone(),
                byte_length: output.artifact.byte_length,
            })
            .collect::<Vec<_>>();
        let model_visible = ModelVisibleToolResultEnvelopeV1 {
            schema_version: ToolWireVersionV1,
            tool_call_id: dispatched.attempt.tool_call_id,
            definition_ref: Some(dispatched.result_context.definition_ref.clone()),
            outcome: model_visible_outcome(&evaluated.terminal),
            side_effect_state: aggregate.state,
            warnings: warnings.clone(),
            artifacts: model_output_artifacts,
            completed_at,
        };
        model_visible.validate().map_err(|_| ToolAttemptCompletionErrorV1::ResultInvalid)?;
        let user_audit = UserAuditToolResultEnvelopeV1 {
            schema_version: ToolWireVersionV1,
            tool_batch_id: dispatched.tool_batch_id,
            tool_call_id: dispatched.attempt.tool_call_id,
            ordinal: dispatched.result_context.ordinal,
            definition_ref: dispatched.result_context.definition_ref.clone(),
            tool_instance_id: dispatched.result_context.tool_instance_id,
            source: dispatched.result_context.source,
            purpose: Some(dispatched.result_context.purpose.clone()),
            arguments_hash: Some(dispatched.result_context.arguments_hash.clone()),
            outcome: result_outcome_summary(&evaluated.terminal),
            side_effect_state: aggregate.state,
            warnings,
            attempts: user_audit_attempts(&attempt_records, &evaluated.terminal),
            artifacts: user_output_artifacts,
            started_at: Some(started_at),
            completed_at,
        };
        user_audit.validate().map_err(|_| ToolAttemptCompletionErrorV1::ResultInvalid)?;
        let model_visible_bytes = aura_domain_common::jcs::canonical_json_jcs(&model_visible)
            .map_err(|_| ToolAttemptCompletionErrorV1::EvidenceCanonicalization)?;
        let model_visible_result_artifact = ArtifactWriteDraftV1::new(
            ArtifactWriteRequest {
                scope: ArtifactScope::ToolExecution,
                media_type: "application/vnd.aura.model-visible-tool-result.v1+json".to_owned(),
                access: ArtifactAccess::MODEL_READ,
                owner: ArtifactOwnerRef::Tool {
                    owner: ToolArtifactOwnerRef::ToolCall {
                        tool_call_id: dispatched.attempt.tool_call_id,
                    },
                },
                bytes: model_visible_bytes.clone(),
            },
            completed_at,
        )?;
        let model_visible_artifact = model_visible_result_artifact.record()?;
        let user_audit_result_artifact = ArtifactWriteDraftV1::new(
            ArtifactWriteRequest {
                scope: ArtifactScope::ToolExecution,
                media_type: "application/vnd.aura.user-audit-tool-result.v1+json".to_owned(),
                access: ArtifactAccess::USER_READ,
                owner: ArtifactOwnerRef::Tool {
                    owner: ToolArtifactOwnerRef::ToolCall {
                        tool_call_id: dispatched.attempt.tool_call_id,
                    },
                },
                bytes: aura_domain_common::jcs::canonical_json_jcs(&user_audit)
                    .map_err(|_| ToolAttemptCompletionErrorV1::EvidenceCanonicalization)?,
            },
            completed_at,
        )?;
        let user_audit_artifact = user_audit_result_artifact.record()?;
        let context_projection = project_tool_result_context_v1(
            model_visible_artifact.artifact_id,
            model_visible_artifact.content_hash.clone(),
            model_visible_artifact.byte_length,
            &model_visible_bytes,
        )
        .map_err(|_| ToolAttemptCompletionErrorV1::ResultInvalid)?;
        let model_visible_result_ref = Some(model_visible_artifact.artifact_id);
        let user_audit_result_ref = Some(user_audit_artifact.artifact_id);
        let mut result_artifact_access_links = vec![
            ArtifactAccessLinkV1::new(
                model_visible_artifact.artifact_id,
                ArtifactAccessSubjectV1::Session {
                    session_id: dispatched.result_context.session_id,
                },
                ArtifactAccess::MODEL_READ,
                completed_at,
            )
            .map_err(|_| ToolAttemptCompletionErrorV1::ResultInvalid)?,
            ArtifactAccessLinkV1::new(
                user_audit_artifact.artifact_id,
                ArtifactAccessSubjectV1::Session {
                    session_id: dispatched.result_context.session_id,
                },
                ArtifactAccess::USER_READ,
                completed_at,
            )
            .map_err(|_| ToolAttemptCompletionErrorV1::ResultInvalid)?,
        ];
        for output in verified_outputs {
            result_artifact_access_links.push(
                ArtifactAccessLinkV1::new(
                    output.artifact.artifact_id,
                    ArtifactAccessSubjectV1::Session {
                        session_id: dispatched.result_context.session_id,
                    },
                    ArtifactAccess::MODEL_READ.union(ArtifactAccess::USER_READ),
                    completed_at,
                )
                .map_err(|_| ToolAttemptCompletionErrorV1::ResultInvalid)?,
            );
        }
        let result = ToolResultV1 {
            tool_call_id: dispatched.attempt.tool_call_id,
            outcome: result_outcome(&evaluated.terminal),
            side_effect_state: aggregate.state,
            attempts,
            model_visible_result_ref,
            user_audit_result_ref,
            internal_diagnostic_ref: Some(attempt_evidence.artifact_id),
            aggregate_side_effect_evidence_ref: aggregate.aggregate_evidence_ref,
            started_at: Some(started_at),
            completed_at,
        };
        result.validate().map_err(|_| ToolAttemptCompletionErrorV1::ResultInvalid)?;
        let projections = dispatched.projection_state.finalize_attempt(
            &dispatched.attempt,
            result.clone(),
            Some(UserReadableToolArtifactView {
                artifact_id: model_visible_artifact.artifact_id,
                media_type: model_visible_artifact.media_type.clone(),
                byte_length: model_visible_artifact.byte_length,
            }),
            Some(UserReadableToolArtifactView {
                artifact_id: user_audit_artifact.artifact_id,
                media_type: user_audit_artifact.media_type.clone(),
                byte_length: user_audit_artifact.byte_length,
            }),
        )?;
        let entity_state = persistence
            .load_tool_execution_entity_state_v1(
                projections.batch.tool_batch_id,
                dispatched.attempt.tool_call_id,
            )?
            .ok_or(ToolAttemptCompletionErrorV1::ExecutionStateUnavailable)?;
        let terminal = terminal_fact(&evaluated.terminal);
        let commit = persistence.commit_tool_execution_attempt_finalized(
            ToolExecutionAttemptFinalizationRequest {
                attempt: dispatched.attempt,
                terminal,
                result: result.clone(),
                batch_result: projections.batch_result,
                expected_batch_revision: entity_state.batch_revision,
                expected_call_revision: entity_state.call_revision,
                batch_projection: projections.batch,
                call_projection: projections.call,
                attempt_projection: projections.attempt,
                session_activity_projection: projections.session,
                attempt_evidence_artifact,
                aggregate_side_effect_evidence_artifact,
                model_visible_result_artifact,
                user_audit_result_artifact,
                result_artifact_access_links,
                causation_id: CausationId::from_ulid(dispatched.dispatch_prepared_event_id.ulid()),
                correlation_id: dispatched.correlation_id,
            },
        )?;
        let projection_publish_error =
            persistence.publish_tool_projection_changes(&commit.projection_changes).err();
        Ok(CompleteDispatchedToolAttemptOutcomeV1::Finalized {
            result,
            context_projection,
            projection_state: dispatched.projection_state,
            persistence: commit,
            projection_publish_error,
        })
    }
}

fn dispatch_candidate(evidence: &ToolRuntimeDispatchEvidenceV1) -> &ToolRuntimeCandidateV1 {
    match evidence {
        ToolRuntimeDispatchEvidenceV1::BuiltinCoreHandler(value) => &value.candidate,
        ToolRuntimeDispatchEvidenceV1::Command(value) => &value.candidate,
        ToolRuntimeDispatchEvidenceV1::AuraStdio(value) => &value.candidate,
        ToolRuntimeDispatchEvidenceV1::DeclarativeHttp(value) => &value.candidate,
    }
}

fn dispatch_retry_source(
    evidence: &ToolRuntimeDispatchEvidenceV1,
) -> Option<crate::p07_execution::ToolCoreRetrySourceV1> {
    match evidence {
        ToolRuntimeDispatchEvidenceV1::DeclarativeHttp(value) => value.retry_source,
        ToolRuntimeDispatchEvidenceV1::BuiltinCoreHandler(_)
        | ToolRuntimeDispatchEvidenceV1::Command(_)
        | ToolRuntimeDispatchEvidenceV1::AuraStdio(_) => None,
    }
}

fn capped_retry_after_ms(
    retry_after_ms: Option<u64>,
    started_at: UtcTimestamp,
    completed_at: UtcTimestamp,
    frozen_timeout_ms: u64,
) -> Option<u64> {
    let elapsed = completed_at.value() - started_at.value();
    let elapsed_ms = u64::try_from(elapsed.whole_milliseconds().max(0)).unwrap_or(u64::MAX);
    let remaining_ms = frozen_timeout_ms.saturating_sub(elapsed_ms);
    retry_after_ms.map(|delay_ms| delay_ms.min(remaining_ms))
}

fn dispatch_output_artifacts(
    evidence: &ToolRuntimeDispatchEvidenceV1,
) -> &[crate::p07_output::VerifiedToolOutputArtifactV1] {
    match evidence {
        ToolRuntimeDispatchEvidenceV1::AuraStdio(value) => &value.verified_artifacts,
        ToolRuntimeDispatchEvidenceV1::BuiltinCoreHandler(_)
        | ToolRuntimeDispatchEvidenceV1::Command(_)
        | ToolRuntimeDispatchEvidenceV1::DeclarativeHttp(_) => &[],
    }
}

fn terminal_status(terminal: &ToolAttemptTerminalDecisionV1) -> ToolExecutionAttemptStatusV1 {
    match terminal {
        ToolAttemptTerminalDecisionV1::Succeeded { .. } => ToolExecutionAttemptStatusV1::Succeeded,
        ToolAttemptTerminalDecisionV1::Failed { .. } => ToolExecutionAttemptStatusV1::Failed,
        ToolAttemptTerminalDecisionV1::Cancelled { .. } => ToolExecutionAttemptStatusV1::Cancelled,
        ToolAttemptTerminalDecisionV1::Indeterminate { .. } => {
            ToolExecutionAttemptStatusV1::Indeterminate
        }
    }
}

fn terminal_fact(terminal: &ToolAttemptTerminalDecisionV1) -> ToolExecutionAttemptTerminalFact {
    match terminal {
        ToolAttemptTerminalDecisionV1::Succeeded { .. } => {
            ToolExecutionAttemptTerminalFact::Succeeded
        }
        ToolAttemptTerminalDecisionV1::Failed { error } => {
            ToolExecutionAttemptTerminalFact::Failed { error: error.clone() }
        }
        ToolAttemptTerminalDecisionV1::Cancelled { reason } => {
            ToolExecutionAttemptTerminalFact::Cancelled { reason: *reason }
        }
        ToolAttemptTerminalDecisionV1::Indeterminate { reason, error } => {
            ToolExecutionAttemptTerminalFact::Indeterminate {
                reason: *reason,
                error: error.clone(),
            }
        }
    }
}

fn result_outcome(terminal: &ToolAttemptTerminalDecisionV1) -> ToolResultOutcomeV1 {
    match terminal {
        ToolAttemptTerminalDecisionV1::Succeeded { .. } => ToolResultOutcomeV1::Succeeded,
        ToolAttemptTerminalDecisionV1::Failed { error } => {
            ToolResultOutcomeV1::Failed { error: error.clone() }
        }
        ToolAttemptTerminalDecisionV1::Cancelled { reason } => {
            ToolResultOutcomeV1::Cancelled { reason: *reason }
        }
        ToolAttemptTerminalDecisionV1::Indeterminate { reason, error } => {
            ToolResultOutcomeV1::Indeterminate { reason: *reason, error: error.clone() }
        }
    }
}

fn model_visible_outcome(
    terminal: &ToolAttemptTerminalDecisionV1,
) -> ModelVisibleToolResultOutcomeV1 {
    match terminal {
        ToolAttemptTerminalDecisionV1::Succeeded { body } => {
            ModelVisibleToolResultOutcomeV1::Succeeded {
                payload: match body {
                    ValidatedToolResultBodyV1::Unit => ModelVisibleToolSuccessPayloadV1::Unit,
                    ValidatedToolResultBodyV1::Utf8Text { text, .. } => {
                        ModelVisibleToolSuccessPayloadV1::Utf8Text { text: text.clone() }
                    }
                    ValidatedToolResultBodyV1::StructuredJson { value, .. } => {
                        ModelVisibleToolSuccessPayloadV1::StructuredJson { value: value.clone() }
                    }
                },
            }
        }
        ToolAttemptTerminalDecisionV1::Failed { error } => {
            ModelVisibleToolResultOutcomeV1::Failed { error: model_safe_error(error) }
        }
        ToolAttemptTerminalDecisionV1::Cancelled { reason } => {
            ModelVisibleToolResultOutcomeV1::Cancelled { reason: *reason }
        }
        ToolAttemptTerminalDecisionV1::Indeterminate { reason, error } => {
            ModelVisibleToolResultOutcomeV1::Indeterminate {
                reason: *reason,
                error: model_safe_error(error),
            }
        }
    }
}

fn result_outcome_summary(terminal: &ToolAttemptTerminalDecisionV1) -> ToolResultOutcomeSummaryV1 {
    match terminal {
        ToolAttemptTerminalDecisionV1::Succeeded { .. } => ToolResultOutcomeSummaryV1::Succeeded,
        ToolAttemptTerminalDecisionV1::Failed { .. } => ToolResultOutcomeSummaryV1::Failed,
        ToolAttemptTerminalDecisionV1::Cancelled { reason } => {
            ToolResultOutcomeSummaryV1::Cancelled { reason: *reason }
        }
        ToolAttemptTerminalDecisionV1::Indeterminate { reason, .. } => {
            ToolResultOutcomeSummaryV1::Indeterminate { reason: *reason }
        }
    }
}

fn model_safe_error(error: &ErrorEnvelope) -> ModelSafeToolErrorV1 {
    ModelSafeToolErrorV1 {
        code: error.code.clone(),
        category: error.category,
        message: error.message.clone(),
        retryable: error.retryable,
    }
}

fn result_warnings(
    terminal: &ToolAttemptTerminalDecisionV1,
    side_effect_state: SideEffectStateV1,
) -> Result<Vec<NormalizedIssueCode>, ToolAttemptCompletionErrorV1> {
    if matches!(terminal, ToolAttemptTerminalDecisionV1::Succeeded { .. })
        && side_effect_state == SideEffectStateV1::Unknown
    {
        return Ok(vec![
            NormalizedIssueCode::new(TOOL_SIDE_EFFECT_UNVERIFIED_WARNING_V1)
                .map_err(|_| ToolAttemptCompletionErrorV1::ResultInvalid)?,
        ]);
    }
    Ok(Vec::new())
}

fn user_audit_attempts(
    attempts: &[ToolExecutionAttemptRecordV1],
    terminal: &ToolAttemptTerminalDecisionV1,
) -> Vec<UserAuditAttemptSummaryV1> {
    let current_id = attempts.last().map(|attempt| attempt.tool_execution_attempt_id);
    attempts
        .iter()
        .map(|attempt| UserAuditAttemptSummaryV1 {
            tool_execution_attempt_id: attempt.tool_execution_attempt_id,
            attempt_number: attempt.attempt_number,
            status: attempt.status,
            side_effect_state: attempt.side_effect_state,
            started_at: attempt.started_at,
            completed_at: attempt.completed_at,
            safe_error: (Some(attempt.tool_execution_attempt_id) == current_id)
                .then(|| match terminal {
                    ToolAttemptTerminalDecisionV1::Failed { error }
                    | ToolAttemptTerminalDecisionV1::Indeterminate { error, .. } => {
                        Some(model_safe_error(error))
                    }
                    _ => None,
                })
                .flatten(),
        })
        .collect()
}

fn attempt_facts(
    mut persisted: Vec<aura_tool_contract::ToolExecutionAttemptRecordV1>,
    terminal: &aura_tool_contract::ToolExecutionAttemptRecordV1,
    reconciliations: Vec<AttemptReconciliationInputV1>,
) -> Result<
    (Vec<AttemptSideEffectFactV1>, Vec<ToolExecutionAttemptRecordV1>, UtcTimestamp),
    ToolAttemptCompletionErrorV1,
> {
    let started_at = persisted
        .first()
        .map(|attempt| attempt.started_at)
        .ok_or(ToolAttemptCompletionErrorV1::AttemptHistoryMismatch)?;
    let Some(current) = persisted.last_mut() else {
        return Err(ToolAttemptCompletionErrorV1::AttemptHistoryMismatch);
    };
    if current.tool_execution_attempt_id != terminal.tool_execution_attempt_id
        || current.tool_call_id != terminal.tool_call_id
        || current.attempt_number != terminal.attempt_number
        || current.status.is_terminal()
    {
        return Err(ToolAttemptCompletionErrorV1::AttemptHistoryMismatch);
    }
    *current = terminal.clone();
    let mut by_attempt = BTreeMap::new();
    for value in reconciliations {
        if by_attempt.insert(value.tool_execution_attempt_id, value.reconciliation).is_some() {
            return Err(ToolAttemptCompletionErrorV1::ReconciliationMismatch);
        }
    }
    let facts = persisted
        .iter()
        .map(|attempt| {
            let outcome = ToolExecutionAttemptOutcomeRefV1 {
                tool_execution_attempt_id: attempt.tool_execution_attempt_id,
                attempt_number: attempt.attempt_number,
                side_effect_state: attempt.side_effect_state,
                side_effect_evidence_ref: attempt.evidence_ref,
                dispatched: true,
            };
            let reconciliation = by_attempt.remove(&attempt.tool_execution_attempt_id);
            if reconciliation.is_some()
                && (attempt.tool_execution_attempt_id == terminal.tool_execution_attempt_id
                    || attempt.side_effect_state != SideEffectStateV1::Unknown)
            {
                return Err(ToolAttemptCompletionErrorV1::ReconciliationMismatch);
            }
            Ok(AttemptSideEffectFactV1 { attempt: outcome, reconciliation })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !by_attempt.is_empty() {
        return Err(ToolAttemptCompletionErrorV1::ReconciliationMismatch);
    }
    Ok((facts, persisted, started_at))
}

fn runtime_evidence_bytes(
    evidence: &ToolRuntimeDispatchEvidenceV1,
    side_effect: &VerifiedAttemptSideEffectEvidenceV1,
    runtime_started_error: Option<&str>,
) -> Result<Vec<u8>, ToolAttemptCompletionErrorV1> {
    let value = match evidence {
        ToolRuntimeDispatchEvidenceV1::BuiltinCoreHandler(value) => json!({
            "evidence_version": 1,
            "runtime": "builtin_core_handler",
            "handler": value.handler,
            "binding_hash": value.binding_hash,
            "source_registry_snapshot_ref": value.source_registry_snapshot_ref,
            "resulting_registry_snapshot_ref": value.resulting_registry_snapshot_ref,
            "changed": value.changed,
            "projection_publish_failed": value.projection_publish_failed,
            "candidate": candidate_value(&value.candidate)?,
            "runtime_started_error": runtime_started_error,
            "side_effect_evidence": side_effect,
        }),
        ToolRuntimeDispatchEvidenceV1::Command(value) => json!({
            "evidence_version": 1,
            "runtime": "command",
            "candidate": candidate_value(&value.candidate)?,
            "stderr_base64": BASE64.encode(&value.stderr),
            "internal_event_error": value.internal_event_error,
            "runtime_started_error": runtime_started_error,
            "side_effect_evidence": side_effect,
        }),
        ToolRuntimeDispatchEvidenceV1::AuraStdio(value) => json!({
            "evidence_version": 1,
            "runtime": "aura_stdio",
            "candidate": candidate_value(&value.candidate)?,
            "stderr_base64": BASE64.encode(&value.stderr),
            "internal_protocol_error": value.internal_protocol_error,
            "runtime_started_error": runtime_started_error,
            "verified_artifacts": value.verified_artifacts.iter().map(|artifact| json!({
                "sequence": artifact.sequence,
                "slot": artifact.slot,
                "artifact_id": artifact.artifact.artifact_id,
                "media_type": artifact.media_type,
                "byte_length": artifact.artifact.byte_length,
                "content_hash": artifact.artifact.content_hash,
            })).collect::<Vec<_>>(),
            "side_effect_evidence": side_effect,
        }),
        ToolRuntimeDispatchEvidenceV1::DeclarativeHttp(value) => json!({
            "evidence_version": 1,
            "runtime": "declarative_http",
            "candidate": candidate_value(&value.candidate)?,
            "response_header_names": value.response_header_names,
            "final_origin": value.final_origin,
            "final_destination": value.final_destination,
            "redirect_hops": value.redirect_hops,
            "gateway_error_code": value.gateway_error_code,
            "credential_reflection": value.credential_reflection,
            "retry_source": value.retry_source.map(|source| json!({
                "kind": source.safe_code(),
                "gateway_retry_after_ms": source.retry_after_ms(),
            })),
            "runtime_started_error": runtime_started_error,
            "side_effect_evidence": side_effect,
        }),
    };
    aura_domain_common::jcs::canonical_json_jcs(&value)
        .map_err(|_| ToolAttemptCompletionErrorV1::EvidenceCanonicalization)
}

fn candidate_value(
    candidate: &ToolRuntimeCandidateV1,
) -> Result<Value, ToolAttemptCompletionErrorV1> {
    Ok(match candidate {
        ToolRuntimeCandidateV1::CoreBuiltinHandlerCompleted { response_body } => json!({
            "type": "core_builtin_handler_completed",
            "response_body_base64": BASE64.encode(response_body),
        }),
        ToolRuntimeCandidateV1::CommandExited {
            exit_code,
            success_exit_codes,
            stdout,
            process_tree_terminated,
        } => json!({
            "type": "command_exited",
            "exit_code": exit_code,
            "success_exit_codes": success_exit_codes,
            "stdout_base64": BASE64.encode(stdout),
            "process_tree_terminated": process_tree_terminated,
        }),
        ToolRuntimeCandidateV1::HttpCompleted { status, success_status_ranges, response_body } => {
            json!({
                "type": "http_completed",
                "status": status,
                "success_status_ranges": success_status_ranges,
                "response_body_base64": BASE64.encode(response_body),
            })
        }
        ToolRuntimeCandidateV1::StdioCompleted { terminal, process_tree_terminated } => json!({
            "type": "stdio_completed",
            "terminal": terminal,
            "process_tree_terminated": process_tree_terminated,
        }),
        ToolRuntimeCandidateV1::DeterministicFailure { code } => json!({
            "type": "deterministic_failure",
            "code": code.error_code(),
        }),
        ToolRuntimeCandidateV1::DeterministicExternalFailure { error } => json!({
            "type": "deterministic_external_failure",
            "error": error,
        }),
        ToolRuntimeCandidateV1::CancelledAndTerminated => {
            json!({ "type": "cancelled_and_terminated" })
        }
        ToolRuntimeCandidateV1::TimedOutAndTerminated => {
            json!({ "type": "timed_out_and_terminated" })
        }
        ToolRuntimeCandidateV1::DispatchOutcomeUnknown => {
            json!({ "type": "dispatch_outcome_unknown" })
        }
        ToolRuntimeCandidateV1::RuntimeTerminationUnknown => {
            json!({ "type": "runtime_termination_unknown" })
        }
        ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid => {
            json!({ "type": "runtime_terminal_evidence_invalid" })
        }
    })
}

fn aggregate_evidence_bytes(
    attempts: &[AttemptSideEffectFactV1],
    evidence: &CallSideEffectEvidenceV1,
) -> Result<Vec<u8>, ToolAttemptCompletionErrorV1> {
    let reconciliations = attempts
        .iter()
        .filter_map(|fact| {
            fact.reconciliation.as_ref().map(|value| {
                json!({
                    "tool_execution_attempt_id": fact.attempt.tool_execution_attempt_id,
                    "resolved_state": value.resolved_state,
                    "evidence_ref": value.evidence_ref,
                    "covers_attempt": value.covers_attempt,
                    "covers_resource_claims": value.covers_resource_claims,
                    "covers_effect_scope": value.covers_effect_scope,
                    "deterministic": value.deterministic,
                })
            })
        })
        .collect::<Vec<_>>();
    aura_domain_common::jcs::canonical_json_jcs(&json!({
        "evidence_version": 1,
        "attempts": attempts.iter().map(|fact| &fact.attempt).collect::<Vec<_>>(),
        "reconciliations": reconciliations,
        "final_postcondition_verified": evidence.final_postcondition_verified,
        "all_effects_covered_by_final_state": evidence.all_effects_covered_by_final_state,
        "baseline_restoration_verified": evidence.baseline_restoration_verified,
        "duplicate_effects_absent_or_deduplicated": evidence.duplicate_effects_absent_or_deduplicated,
    }))
    .map_err(|_| ToolAttemptCompletionErrorV1::EvidenceCanonicalization)
}

#[derive(Debug, Error)]
pub(crate) enum ToolAttemptCompletionErrorV1 {
    #[error("the caller cannot supply an aggregate evidence Artifact identity")]
    CallerSuppliedAggregateArtifact,
    #[error("the persisted Attempt history does not match the dispatched Attempt")]
    AttemptHistoryMismatch,
    #[error("a reconciliation fact does not map uniquely to an Attempt")]
    ReconciliationMismatch,
    #[error("the terminal Attempt record is invalid")]
    AttemptInvalid,
    #[error(
        "the Core-provided Attempt SideEffect evidence is invalid or belongs to another Attempt"
    )]
    AttemptSideEffectEvidenceInvalid,
    #[error("the logical ToolResult is invalid")]
    ResultInvalid,
    #[error("the current Tool execution CAS state is unavailable")]
    ExecutionStateUnavailable,
    #[error("internal Runtime evidence cannot be canonicalized")]
    EvidenceCanonicalization,
    #[error(transparent)]
    Evaluation(#[from] ToolRuntimeEvaluationErrorV1),
    #[error(transparent)]
    Retry(#[from] ToolRetryAssessmentError),
    #[error(transparent)]
    Aggregation(#[from] SideEffectAggregationError),
    #[error(transparent)]
    Projection(#[from] ToolExecutionProjectionErrorV1),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use aura_domain_common::{ArtifactId, UtcTimestamp};
    use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
    use aura_execution_contract::SideEffectStateV1;
    use aura_tool_contract::{ToolExecutionAttemptRecordV1, ToolExecutionAttemptStatusV1};
    use aura_tool_scheduler::ReconciledAttemptSideEffectV1;

    use super::{AttemptReconciliationInputV1, ToolAttemptCompletionErrorV1, attempt_facts};

    fn timestamp(value: &str) -> UtcTimestamp {
        UtcTimestamp::from_str(value).expect("timestamp")
    }

    fn attempt(
        call_id: ToolCallId,
        attempt_number: u8,
        status: ToolExecutionAttemptStatusV1,
        state: SideEffectStateV1,
    ) -> ToolExecutionAttemptRecordV1 {
        ToolExecutionAttemptRecordV1 {
            tool_execution_attempt_id: ToolExecutionAttemptId::new(),
            tool_call_id: call_id,
            attempt_number,
            status,
            side_effect_state: state,
            evidence_ref: (state != SideEffectStateV1::NotStarted).then(ArtifactId::new),
            started_at: timestamp("2026-09-05T00:00:00Z"),
            dispatch_prepared_at: Some(timestamp("2026-09-05T00:00:01Z")),
            completed_at: status.is_terminal().then(|| timestamp("2026-09-05T00:00:02Z")),
            last_progress_sequence: 0,
        }
    }

    fn reconciliation(
        tool_execution_attempt_id: ToolExecutionAttemptId,
    ) -> AttemptReconciliationInputV1 {
        AttemptReconciliationInputV1 {
            tool_execution_attempt_id,
            reconciliation: ReconciledAttemptSideEffectV1 {
                resolved_state: SideEffectStateV1::NoEffect,
                evidence_ref: ArtifactId::new(),
                covers_attempt: true,
                covers_resource_claims: true,
                covers_effect_scope: true,
                deterministic: true,
            },
        }
    }

    #[test]
    fn attempt_history_replaces_only_current_attempt_and_binds_prior_reconciliation() {
        let call_id = ToolCallId::new();
        let first =
            attempt(call_id, 1, ToolExecutionAttemptStatusV1::Failed, SideEffectStateV1::Unknown);
        let running =
            attempt(call_id, 2, ToolExecutionAttemptStatusV1::Running, SideEffectStateV1::Unknown);
        let mut terminal = running.clone();
        terminal.status = ToolExecutionAttemptStatusV1::Succeeded;
        terminal.side_effect_state = SideEffectStateV1::NoEffect;
        terminal.completed_at = Some(timestamp("2026-09-05T00:00:03Z"));

        let (facts, records, started_at) = attempt_facts(
            vec![first.clone(), running],
            &terminal,
            vec![reconciliation(first.tool_execution_attempt_id)],
        )
        .expect("attempt facts");
        assert_eq!(started_at, first.started_at);
        assert_eq!(facts.len(), 2);
        assert_eq!(records.len(), 2);
        assert!(facts[0].reconciliation.is_some());
        assert!(facts[1].reconciliation.is_none());
        assert_eq!(facts[1].attempt.side_effect_state, SideEffectStateV1::NoEffect);
    }

    #[test]
    fn attempt_history_rejects_duplicate_or_current_attempt_reconciliation() {
        let call_id = ToolCallId::new();
        let first =
            attempt(call_id, 1, ToolExecutionAttemptStatusV1::Failed, SideEffectStateV1::Unknown);
        let running =
            attempt(call_id, 2, ToolExecutionAttemptStatusV1::Running, SideEffectStateV1::Unknown);
        let mut terminal = running.clone();
        terminal.status = ToolExecutionAttemptStatusV1::Failed;
        terminal.side_effect_state = SideEffectStateV1::NoEffect;
        terminal.completed_at = Some(timestamp("2026-09-05T00:00:03Z"));

        let duplicate = vec![
            reconciliation(first.tool_execution_attempt_id),
            reconciliation(first.tool_execution_attempt_id),
        ];
        assert!(matches!(
            attempt_facts(vec![first.clone(), running.clone()], &terminal, duplicate),
            Err(ToolAttemptCompletionErrorV1::ReconciliationMismatch)
        ));
        assert!(matches!(
            attempt_facts(
                vec![first, running],
                &terminal,
                vec![reconciliation(terminal.tool_execution_attempt_id)],
            ),
            Err(ToolAttemptCompletionErrorV1::ReconciliationMismatch)
        ));
    }
}
