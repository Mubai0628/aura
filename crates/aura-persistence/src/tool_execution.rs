mod recovery_projection;
use crate::tool_reference::resolved as resolved_tool;
use recovery_projection::{recovery_call_projection, recovery_call_summary};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use aura_artifact_contract::{
    ArtifactAccess, ArtifactAccessLinkV1, ArtifactAccessSubjectV1, ArtifactOwnerRef, ArtifactScope,
    ToolArtifactOwnerRef,
};
use aura_domain_common::ToolCallId;
use aura_domain_common::{
    ArtifactId, CausationId, ContentHashAlgorithm, CorrelationId, ErrorCategory, ErrorEnvelope,
    EventId, PermissionGrantId, Revision, UtcTimestamp, WorkspaceId,
};
use aura_execution_contract::SideEffectStateV1;
use aura_permission_domain::{
    AuthorizedToolSandboxExecutionV1, PermissionGrant, SecuritySource, ToolSandboxDispatchBindingV1,
};
use aura_projection_contract::ProjectionChange;
use aura_security_contract::{PermissionGrantLifetime, PermissionMode};
use aura_tool_contract::{
    NormalizedToolCallInputStateV1, ToolBatchAdmittedPayloadV1, ToolBatchRequestV1,
    ToolBatchResultV1, ToolBatchStatusV1, ToolCallStatusV1, ToolCancellationReasonV1,
    ToolExecutionAttemptCancellationRequestedPayloadV1, ToolExecutionAttemptCancelledPayloadV1,
    ToolExecutionAttemptDispatchPreparedPayloadV1, ToolExecutionAttemptFailedPayloadV1,
    ToolExecutionAttemptIndeterminatePayloadV1, ToolExecutionAttemptRecordV1,
    ToolExecutionAttemptStartedPayloadV1, ToolExecutionAttemptStatusV1,
    ToolExecutionAttemptSucceededPayloadV1, ToolExecutionPlanCallV1, ToolExecutionPlanV1,
    ToolIndeterminateReasonV1, ToolLifecycleEventV1, ToolRegistrySnapshotV1,
    ToolResultFinalizedPayloadV1, ToolResultOutcomeV1, ToolResultV1,
};
use aura_tool_domain::{ToolDefinitionRecordV1, ToolInstanceRecordV1};
use aura_tool_projection::{
    SessionToolActivityProjection, ToolBatchActivitySummaryView, ToolBatchProjection,
    ToolCallActivitySummaryView, ToolCallErrorClassV1, ToolCallProjection,
    ToolExecutionAttemptProjection, ToolProjectionErrorView, ToolResultOutcomeSummaryView,
};
use aura_tool_registry::BuiltinCoreHandlerDispatchBindingV1;
use aura_tool_scheduler::{
    ToolRetryAssessmentV1, ToolRetryDecisionV1, assess_failed_attempt_retry,
};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::{
    ArtifactWriteDraftV1, ArtifactWriteRequest, EventActor, PersistenceCoordinator,
    PersistenceError, PersistenceFaultPoint, PersistentEventDraft, ToolProjectionDraft,
    artifact::parse_owner,
    artifact::{insert_artifact_in_transaction, insert_artifact_write_draft_in_transaction},
    coordinator::insert_event,
    tool_activity::attach_older_activity_cursor,
    tool_entities::{
        decode_json, decode_verified_record, encode_json, encode_record, sqlite_revision,
    },
    tool_history::finalize_tool_history_atomic_group_if_pending,
    tool_package_reads::PAYLOAD_RETENTION_DAYS,
    tool_projection::insert_tool_projections,
};

#[derive(Clone, Debug)]
pub struct ToolBatchAdmissionCommitRequest {
    pub batch: ToolBatchRequestV1,
    pub execution_plan: ToolExecutionPlanV1,
    pub not_executed_results: Vec<ToolResultV1>,
    pub batch_projection: ToolBatchProjection,
    pub call_projections: Vec<ToolCallProjection>,
    pub session_activity_projection: SessionToolActivityProjection,
    pub admitted_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolExecutionPersistenceCommit {
    pub event_ids: Vec<EventId>,
    pub projection_changes: Vec<ProjectionChange>,
    pub projection_outbox_ids: Vec<i64>,
}

/// Exact mutable entity state used by the Core dispatch coordinator for the
/// final compare-and-swap immediately before creating a Runtime attempt.
///
/// The immutable batch request and execution plan remain the authority for
/// identity and policy. This read model only exposes the current lifecycle
/// revisions and the next attempt number; it is never a substitute for the
/// frozen `trs_` or execution plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolExecutionEntityStateV1 {
    pub tool_batch_id: aura_domain_common::ToolBatchId,
    pub tool_call_id: ToolCallId,
    pub batch_status: ToolBatchStatusV1,
    pub call_status: ToolCallStatusV1,
    pub batch_revision: Revision,
    pub call_revision: Revision,
    pub next_attempt_number: u8,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToolExecutionStartupRecoveryReport {
    pub pre_dispatch_calls_terminalized: usize,
    pub dispatch_unknown_attempts_terminalized: usize,
    pub completed_batches: usize,
    pub historical_started_only_attempts: usize,
}

#[derive(Clone, Debug)]
pub struct ToolExecutionAttemptDispatchPreparedCommitRequest {
    pub attempt: ToolExecutionAttemptRecordV1,
    pub execution_plan_hash: aura_domain_common::jcs::CanonicalHashV1,
    pub dispatch_evidence_hash: aura_domain_common::jcs::CanonicalHashV1,
    pub expected_batch_revision: aura_domain_common::Revision,
    pub expected_call_revision: aura_domain_common::Revision,
    pub admitted_event_id: EventId,
    pub causation_id: CausationId,
    pub batch_projection: ToolBatchProjection,
    pub call_projection: ToolCallProjection,
    pub attempt_projection: ToolExecutionAttemptProjection,
    pub session_activity_projection: SessionToolActivityProjection,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct AuthorizedToolExecutionAttemptDispatchPreparedCommitRequestV1 {
    pub attempt_commit: ToolExecutionAttemptDispatchPreparedCommitRequest,
    pub sandbox_dispatch_binding: ToolSandboxDispatchBindingV1,
}

#[derive(Clone, Debug)]
pub struct AuthorizedBuiltinCoreHandlerAttemptDispatchPreparedCommitRequestV1 {
    pub attempt_commit: ToolExecutionAttemptDispatchPreparedCommitRequest,
    pub builtin_dispatch_binding: BuiltinCoreHandlerDispatchBindingV1,
}

// Both variants carry exact, transaction-bound authorization records. Keeping
// them inline avoids introducing a second ownership convention in this private
// one-shot persistence boundary.
#[allow(clippy::large_enum_variant)]
enum AuthorizedAttemptDispatchBindingV1 {
    Sandbox(ToolSandboxDispatchBindingV1),
    BuiltinCoreHandler(BuiltinCoreHandlerDispatchBindingV1),
}

#[derive(Clone, Debug)]
pub struct ToolExecutionAttemptRuntimeStartedRequest {
    pub attempt: ToolExecutionAttemptRecordV1,
    pub attempt_projection: ToolExecutionAttemptProjection,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug)]
pub struct ToolExecutionAttemptProgressRequest {
    pub attempt_projection: ToolExecutionAttemptProjection,
}

#[derive(Clone, Debug)]
pub struct ToolExecutionAttemptCancellationRequest {
    pub attempt: ToolExecutionAttemptRecordV1,
    pub reason: ToolCancellationReasonV1,
    pub requested_at: UtcTimestamp,
    pub attempt_projection: ToolExecutionAttemptProjection,
    pub causation_id: CausationId,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ToolExecutionAttemptTerminalFact {
    Succeeded,
    Failed { error: ErrorEnvelope },
    Cancelled { reason: ToolCancellationReasonV1 },
    Indeterminate { reason: ToolIndeterminateReasonV1, error: ErrorEnvelope },
}

#[derive(Clone, Debug)]
pub struct ToolExecutionAttemptFinalizationRequest {
    pub attempt: ToolExecutionAttemptRecordV1,
    pub terminal: ToolExecutionAttemptTerminalFact,
    pub result: ToolResultV1,
    /// Present only when this call supplies the final result required to close
    /// the containing batch.
    pub batch_result: Option<ToolBatchResultV1>,
    pub expected_batch_revision: Revision,
    pub expected_call_revision: Revision,
    pub batch_projection: ToolBatchProjection,
    pub call_projection: ToolCallProjection,
    pub attempt_projection: ToolExecutionAttemptProjection,
    pub session_activity_projection: SessionToolActivityProjection,
    /// These four Tool-owned Artifacts become visible only in the same
    /// terminal transaction as the unique Result and projections.
    pub attempt_evidence_artifact: ArtifactWriteDraftV1,
    pub aggregate_side_effect_evidence_artifact: ArtifactWriteDraftV1,
    pub model_visible_result_artifact: ArtifactWriteDraftV1,
    pub user_audit_result_artifact: ArtifactWriteDraftV1,
    /// Access is granted only by the same transaction that makes the Result
    /// durable. The referenced Tool-owned Artifacts exist beforehand with no
    /// implicit Session access.
    pub result_artifact_access_links: Vec<ArtifactAccessLinkV1>,
    pub causation_id: CausationId,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ToolExecutionAttemptRetryableFailureRequest {
    pub attempt: ToolExecutionAttemptRecordV1,
    pub error: ErrorEnvelope,
    pub attempt_projection: ToolExecutionAttemptProjection,
    pub attempt_evidence_artifact: ArtifactWriteDraftV1,
    pub causation_id: CausationId,
    pub correlation_id: CorrelationId,
}

impl PersistenceCoordinator {
    /// Loads the immutable Attempt records for one admitted ToolCall in retry
    /// order. This is a read-only reconciliation input: it never derives an
    /// outcome from operational progress and rejects any broken batch/call or
    /// attempt sequence.
    pub fn load_tool_execution_attempt_records_v1(
        &self,
        tool_batch_id: aura_domain_common::ToolBatchId,
        tool_call_id: ToolCallId,
    ) -> Result<Vec<ToolExecutionAttemptRecordV1>, PersistenceError> {
        self.with_reader(|connection| {
            let call_exists = connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM tool_calls
                    WHERE tool_batch_id=?1 AND tool_call_id=?2
                )",
                params![tool_batch_id.to_string(), tool_call_id.to_string()],
                |row| row.get::<_, bool>(0),
            )?;
            if !call_exists {
                return Ok(Vec::new());
            }
            let rows = connection
                .prepare(
                    "SELECT record_json, record_hash_json
                     FROM tool_execution_attempts
                     WHERE tool_batch_id=?1 AND tool_call_id=?2
                     ORDER BY attempt_number",
                )?
                .query_map(params![tool_batch_id.to_string(), tool_call_id.to_string()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let attempts = rows
                .into_iter()
                .map(|(record, hash)| {
                    decode_verified_record::<ToolExecutionAttemptRecordV1>(&record, &hash)
                })
                .collect::<Result<Vec<_>, _>>()?;
            for (index, attempt) in attempts.iter().enumerate() {
                attempt.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
                if attempt.tool_call_id != tool_call_id
                    || attempt.attempt_number != u8::try_from(index + 1).unwrap_or(0)
                {
                    return Err(PersistenceError::ToolEntityIntegrity);
                }
            }
            Ok(attempts)
        })
    }

    /// Loads the exact batch/call CAS state without deriving or updating any
    /// execution fact. A missing batch or call returns `None`; an inconsistent
    /// relationship or attempt sequence fails closed.
    pub fn load_tool_execution_entity_state_v1(
        &self,
        tool_batch_id: aura_domain_common::ToolBatchId,
        tool_call_id: ToolCallId,
    ) -> Result<Option<ToolExecutionEntityStateV1>, PersistenceError> {
        self.with_reader(|connection| {
            let row = connection
                .query_row(
                    "SELECT batch.status, batch.entity_revision,
                            call.status, call.entity_revision,
                            MAX(attempt.attempt_number), COUNT(attempt.tool_execution_attempt_id)
                     FROM tool_batches AS batch
                     JOIN tool_calls AS call
                       ON call.tool_batch_id=batch.tool_batch_id
                     LEFT JOIN tool_execution_attempts AS attempt
                       ON attempt.tool_call_id=call.tool_call_id
                      AND attempt.tool_batch_id=batch.tool_batch_id
                     WHERE batch.tool_batch_id=?1 AND call.tool_call_id=?2
                     GROUP BY batch.status, batch.entity_revision,
                              call.status, call.entity_revision",
                    params![tool_batch_id.to_string(), tool_call_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, Option<i64>>(4)?,
                            row.get::<_, i64>(5)?,
                        ))
                    },
                )
                .optional()?;
            let Some((batch_status, batch_revision, call_status, call_revision, maximum, count)) =
                row
            else {
                return Ok(None);
            };
            if count < 0 || maximum.is_none() != (count == 0) {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
            let next_attempt_number = match maximum {
                None => 1,
                Some(value) => value
                    .checked_add(1)
                    .and_then(|value| u8::try_from(value).ok())
                    .filter(|value| *value <= aura_tool_contract::MAX_TOOL_EXECUTION_ATTEMPTS)
                    .ok_or(PersistenceError::ToolEntityStateConflict)?,
            };
            let batch_revision = u64::try_from(batch_revision)
                .ok()
                .filter(|value| *value > 0)
                .map(Revision::new)
                .ok_or(PersistenceError::ToolEntityIntegrity)?;
            let call_revision = u64::try_from(call_revision)
                .ok()
                .filter(|value| *value > 0)
                .map(Revision::new)
                .ok_or(PersistenceError::ToolEntityIntegrity)?;
            Ok(Some(ToolExecutionEntityStateV1 {
                tool_batch_id,
                tool_call_id,
                batch_status: parse_batch_status(&batch_status)?,
                call_status: parse_call_status(&call_status)?,
                batch_revision,
                call_revision,
                next_attempt_number,
            }))
        })
    }

    pub fn commit_tool_batch_admission(
        &self,
        request: ToolBatchAdmissionCommitRequest,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        self.commit_tool_batch_admission_with_fault(request, None)
    }

    pub fn commit_tool_batch_admission_with_fault(
        &self,
        request: ToolBatchAdmissionCommitRequest,
        fault: Option<PersistenceFaultPoint>,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        let partition = validate_admission_request(&request)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let workspace_id = verify_admission_authority(&transaction, &request)?;
            ensure_new_batch_and_calls(&transaction, &request.batch)?;

            insert_admitted_batch(&transaction, &request)?;
            insert_admitted_calls(&transaction, &request, &partition)?;
            acquire_batch_payload_leases(&transaction, &request)?;

            let admitted_event_id = EventId::new();
            let admitted_event = execution_event(
                admitted_event_id,
                workspace_id,
                &request.batch,
                request.correlation_id,
                None,
                ToolLifecycleEventV1::ToolBatchAdmitted(ToolBatchAdmittedPayloadV1 {
                    tool_batch_id: request.batch.tool_batch_id,
                    batch_content_hash: request.batch.batch_content_hash.clone(),
                    execution_plan_hash: request.execution_plan.execution_plan_hash.clone(),
                    admitted_at: request.admitted_at,
                }),
                request.admitted_at,
            )?;
            insert_event(&transaction, &admitted_event)?;

            let mut event_ids = vec![admitted_event_id];
            for result in &request.not_executed_results {
                let event_id = EventId::new();
                let event = execution_event(
                    event_id,
                    workspace_id,
                    &request.batch,
                    request.correlation_id,
                    None,
                    ToolLifecycleEventV1::ToolResultFinalized(ToolResultFinalizedPayloadV1 {
                        tool_batch_id: request.batch.tool_batch_id,
                        result: result.clone(),
                    }),
                    result.completed_at,
                )?;
                insert_event(&transaction, &event)?;
                insert_tool_result(&transaction, &request.batch, result, event_id)?;
                event_ids.push(event_id);
            }

            let mut drafts = Vec::with_capacity(request.call_projections.len() + 2);
            drafts.push(ToolProjectionDraft::ToolBatch(Box::new(request.batch_projection)));
            drafts.extend(
                request
                    .call_projections
                    .into_iter()
                    .map(|projection| ToolProjectionDraft::ToolCall(Box::new(projection))),
            );
            drafts.push(ToolProjectionDraft::SessionToolActivity(attach_older_activity_cursor(
                &transaction,
                request.session_activity_projection,
            )?));
            let (projection_changes, projection_outbox_ids) =
                insert_tool_projections(&transaction, runtime_instance_id, drafts)?;

            if fault == Some(PersistenceFaultPoint::BeforeCommit) {
                return Err(PersistenceError::InvalidRequest(
                    "injected tool admission persistence failure before commit".to_owned(),
                ));
            }
            transaction.commit()?;
            Ok(ToolExecutionPersistenceCommit {
                event_ids,
                projection_changes,
                projection_outbox_ids,
            })
        })
    }

    /// Atomically creates an execution attempt and persists both durable
    /// pre-dispatch boundaries. A runtime boundary must never be crossed unless
    /// this transaction has committed successfully.
    pub fn commit_tool_execution_attempt_dispatch_prepared(
        &self,
        request: ToolExecutionAttemptDispatchPreparedCommitRequest,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        self.commit_tool_execution_attempt_dispatch_prepared_with_fault(request, None)
    }

    pub fn commit_tool_execution_attempt_dispatch_prepared_with_fault(
        &self,
        request: ToolExecutionAttemptDispatchPreparedCommitRequest,
        fault: Option<PersistenceFaultPoint>,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        self.commit_tool_execution_attempt_dispatch_prepared_inner(request, None, fault)
    }

    /// W7-only pre-dispatch commit. The one-shot P04/Sandbox authorization
    /// relation is inserted in the same SQLite transaction as `tea_`, Started,
    /// and DispatchPrepared. The Host cannot consume a binding that outlives a
    /// rolled-back Attempt transaction.
    pub fn commit_authorized_tool_execution_attempt_dispatch_prepared_v1(
        &self,
        request: AuthorizedToolExecutionAttemptDispatchPreparedCommitRequestV1,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        self.commit_authorized_tool_execution_attempt_dispatch_prepared_with_fault_v1(request, None)
    }

    pub fn commit_authorized_tool_execution_attempt_dispatch_prepared_with_fault_v1(
        &self,
        request: AuthorizedToolExecutionAttemptDispatchPreparedCommitRequestV1,
        fault: Option<PersistenceFaultPoint>,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        validate_authorized_dispatch_prepared_request(&request)?;
        self.commit_tool_execution_attempt_dispatch_prepared_inner(
            request.attempt_commit,
            Some(AuthorizedAttemptDispatchBindingV1::Sandbox(request.sandbox_dispatch_binding)),
            fault,
        )
    }

    /// Built-in counterpart to the W7 Sandbox boundary. The exact signed
    /// handler and P04 authorization are committed with `tea_`; no handler is
    /// callable until this binding is consumed once.
    pub fn commit_authorized_builtin_core_handler_attempt_dispatch_prepared_v1(
        &self,
        request: AuthorizedBuiltinCoreHandlerAttemptDispatchPreparedCommitRequestV1,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        validate_authorized_builtin_dispatch_prepared_request(&request)?;
        self.commit_tool_execution_attempt_dispatch_prepared_inner(
            request.attempt_commit,
            Some(AuthorizedAttemptDispatchBindingV1::BuiltinCoreHandler(
                request.builtin_dispatch_binding,
            )),
            None,
        )
    }

    /// Atomically consumes the exact one-shot Core-to-Host authorization
    /// binding before Sandbox Host may cross the native process boundary.
    /// Marking the binding consumed before spawn deliberately makes a crash
    /// outcome unknown rather than allowing an automatic replay.
    pub fn consume_authorized_tool_sandbox_dispatch_v1(
        &self,
        execution: AuthorizedToolSandboxExecutionV1,
        consumed_at: UtcTimestamp,
    ) -> Result<ToolSandboxDispatchBindingV1, PersistenceError> {
        execution.validate().map_err(|_| PersistenceError::ToolSandboxDispatchBindingMismatch)?;
        let expected = execution.binding;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let stored = transaction
                .query_row(
                    "SELECT binding_json, binding_hash_json, state
                     FROM tool_sandbox_dispatch_bindings
                     WHERE sandbox_execution_id=?1",
                    [expected.sandbox_execution_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?
                .ok_or(PersistenceError::ToolSandboxDispatchBindingNotFound)?;
            let stored_binding: ToolSandboxDispatchBindingV1 = decode_json(&stored.0)?;
            let stored_hash: aura_domain_common::jcs::CanonicalHashV1 = decode_json(&stored.1)?;
            if stored_binding != expected
                || stored_hash != expected.binding_hash
                || stored_hash != stored_binding.binding_hash
            {
                return Err(PersistenceError::ToolSandboxDispatchBindingMismatch);
            }
            if stored.2 != "dispatch_prepared" {
                return Err(PersistenceError::ToolSandboxDispatchBindingUnavailable);
            }
            let attempt_state = transaction
                .query_row(
                    "SELECT status, terminal_event_id
                     FROM tool_execution_attempts
                     WHERE tool_execution_attempt_id=?1",
                    [expected.tool_execution_attempt_id.to_string()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()?
                .ok_or(PersistenceError::ToolEntityNotFound)?;
            if attempt_state.0 != "dispatch_prepared" || attempt_state.1.is_some() {
                return Err(PersistenceError::ToolSandboxDispatchBindingUnavailable);
            }
            verify_permission_snapshot_and_dispatch_authorization(
                &transaction,
                &expected,
                consumed_at,
            )?;
            let changed = transaction.execute(
                "UPDATE tool_sandbox_dispatch_bindings
                 SET state='consumed', consumed_at=?1, updated_at=?1
                 WHERE sandbox_execution_id=?2 AND state='dispatch_prepared'
                   AND consumed_at IS NULL",
                params![consumed_at.to_string(), expected.sandbox_execution_id.to_string()],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ToolSandboxDispatchBindingUnavailable);
            }
            transaction.commit()?;
            Ok(expected)
        })
    }

    /// Consumes the exact one-shot P04 binding before the in-process closed
    /// handler enum may be dispatched. A Core crash after this commit never
    /// causes transparent handler replay.
    pub fn consume_authorized_builtin_core_handler_dispatch_v1(
        &self,
        expected: BuiltinCoreHandlerDispatchBindingV1,
        consumed_at: UtcTimestamp,
    ) -> Result<BuiltinCoreHandlerDispatchBindingV1, PersistenceError> {
        expected
            .validate()
            .map_err(|_| PersistenceError::BuiltinCoreHandlerDispatchBindingMismatch)?;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let stored = transaction
                .query_row(
                    "SELECT binding_json, binding_hash_json, state
                     FROM tool_builtin_core_handler_dispatch_bindings
                     WHERE tool_execution_attempt_id=?1",
                    [expected.tool_execution_attempt_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?
                .ok_or(PersistenceError::BuiltinCoreHandlerDispatchBindingNotFound)?;
            let stored_binding: BuiltinCoreHandlerDispatchBindingV1 = decode_json(&stored.0)?;
            let stored_hash: aura_domain_common::jcs::CanonicalHashV1 = decode_json(&stored.1)?;
            if stored_binding != expected
                || stored_hash != expected.binding_hash
                || stored_hash != stored_binding.binding_hash
            {
                return Err(PersistenceError::BuiltinCoreHandlerDispatchBindingMismatch);
            }
            if stored.2 != "dispatch_prepared" {
                return Err(PersistenceError::BuiltinCoreHandlerDispatchBindingUnavailable);
            }
            let attempt_state = transaction
                .query_row(
                    "SELECT status, terminal_event_id
                     FROM tool_execution_attempts
                     WHERE tool_execution_attempt_id=?1",
                    [expected.tool_execution_attempt_id.to_string()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()?
                .ok_or(PersistenceError::ToolEntityNotFound)?;
            if attempt_state.0 != "dispatch_prepared" || attempt_state.1.is_some() {
                return Err(PersistenceError::BuiltinCoreHandlerDispatchBindingUnavailable);
            }
            verify_builtin_permission_snapshot_and_dispatch_authorization(
                &transaction,
                &expected,
                consumed_at,
            )?;
            let changed = transaction.execute(
                "UPDATE tool_builtin_core_handler_dispatch_bindings
                 SET state='consumed', consumed_at=?1, updated_at=?1
                 WHERE tool_execution_attempt_id=?2 AND state='dispatch_prepared'
                   AND consumed_at IS NULL",
                params![consumed_at.to_string(), expected.tool_execution_attempt_id.to_string()],
            )?;
            if changed != 1 {
                return Err(PersistenceError::BuiltinCoreHandlerDispatchBindingUnavailable);
            }
            transaction.commit()?;
            Ok(expected)
        })
    }

    fn commit_tool_execution_attempt_dispatch_prepared_inner(
        &self,
        request: ToolExecutionAttemptDispatchPreparedCommitRequest,
        authorized_dispatch_binding: Option<AuthorizedAttemptDispatchBindingV1>,
        fault: Option<PersistenceFaultPoint>,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        validate_dispatch_prepared_commit_request(&request)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let authority = verify_attempt_start_authority(&transaction, &request)?;
            let prepared_at = request
                .attempt
                .dispatch_prepared_at
                .ok_or(PersistenceError::ToolEntityIntegrity)?;

            let started_event_id = EventId::new();
            let dispatch_prepared_event_id = EventId::new();
            let started_event = execution_event(
                started_event_id,
                authority.workspace_id,
                &authority.batch,
                request.correlation_id,
                Some(request.causation_id),
                ToolLifecycleEventV1::ToolExecutionAttemptStarted(
                    ToolExecutionAttemptStartedPayloadV1 {
                        tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
                        tool_call_id: request.attempt.tool_call_id,
                        tool_batch_id: authority.batch.tool_batch_id,
                        attempt_number: request.attempt.attempt_number,
                        execution_plan_hash: request.execution_plan_hash.clone(),
                        started_at: request.attempt.started_at,
                    },
                ),
                request.attempt.started_at,
            )?;
            insert_event(&transaction, &started_event)?;
            let dispatch_prepared_event = execution_event(
                dispatch_prepared_event_id,
                authority.workspace_id,
                &authority.batch,
                request.correlation_id,
                Some(CausationId::from_ulid(started_event_id.ulid())),
                ToolLifecycleEventV1::ToolExecutionAttemptDispatchPrepared(
                    ToolExecutionAttemptDispatchPreparedPayloadV1 {
                        tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
                        dispatch_evidence_hash: request.dispatch_evidence_hash.clone(),
                        prepared_at,
                    },
                ),
                prepared_at,
            )?;
            insert_event(&transaction, &dispatch_prepared_event)?;
            insert_dispatch_prepared_attempt(
                &transaction,
                &request,
                authority.batch.tool_batch_id,
                started_event_id,
                dispatch_prepared_event_id,
            )?;
            if let Some(binding) = authorized_dispatch_binding.as_ref() {
                match binding {
                    AuthorizedAttemptDispatchBindingV1::Sandbox(binding) => {
                        insert_tool_sandbox_dispatch_binding(
                            &transaction,
                            &request,
                            &authority,
                            binding,
                        )?;
                    }
                    AuthorizedAttemptDispatchBindingV1::BuiltinCoreHandler(binding) => {
                        insert_builtin_core_handler_dispatch_binding(
                            &transaction,
                            &request,
                            &authority,
                            binding,
                        )?;
                    }
                }
            }
            transition_call_and_batch_to_running(&transaction, &request)?;
            let (projection_changes, projection_outbox_ids) = insert_tool_projections(
                &transaction,
                runtime_instance_id,
                vec![
                    ToolProjectionDraft::ToolBatch(Box::new(request.batch_projection)),
                    ToolProjectionDraft::ToolCall(Box::new(request.call_projection)),
                    ToolProjectionDraft::ToolExecutionAttempt(Box::new(request.attempt_projection)),
                    ToolProjectionDraft::SessionToolActivity(attach_older_activity_cursor(
                        &transaction,
                        request.session_activity_projection,
                    )?),
                ],
            )?;
            if fault == Some(PersistenceFaultPoint::BeforeCommit) {
                return Err(PersistenceError::InvalidRequest(
                    "injected atomic tool pre-dispatch persistence failure before commit"
                        .to_owned(),
                ));
            }
            transaction.commit()?;
            Ok(ToolExecutionPersistenceCommit {
                event_ids: vec![started_event_id, dispatch_prepared_event_id],
                projection_changes,
                projection_outbox_ids,
            })
        })
    }

    pub fn commit_tool_execution_attempt_runtime_started(
        &self,
        request: ToolExecutionAttemptRuntimeStartedRequest,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        self.commit_tool_execution_attempt_runtime_started_with_fault(request, None)
    }

    pub fn commit_tool_execution_attempt_runtime_started_with_fault(
        &self,
        request: ToolExecutionAttemptRuntimeStartedRequest,
        fault: Option<PersistenceFaultPoint>,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        validate_runtime_started_request(&request)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            transition_attempt_to_running(&transaction, &request)?;
            let (projection_changes, projection_outbox_ids) = insert_tool_projections(
                &transaction,
                runtime_instance_id,
                vec![ToolProjectionDraft::ToolExecutionAttempt(Box::new(
                    request.attempt_projection,
                ))],
            )?;
            if fault == Some(PersistenceFaultPoint::BeforeCommit) {
                return Err(PersistenceError::InvalidRequest(
                    "injected tool runtime start persistence failure before commit".to_owned(),
                ));
            }
            transaction.commit()?;
            Ok(ToolExecutionPersistenceCommit {
                event_ids: Vec::new(),
                projection_changes,
                projection_outbox_ids,
            })
        })
    }

    pub fn commit_tool_execution_attempt_progress(
        &self,
        request: ToolExecutionAttemptProgressRequest,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        self.commit_tool_execution_attempt_progress_with_fault(request, None)
    }

    pub fn commit_tool_execution_attempt_progress_with_fault(
        &self,
        request: ToolExecutionAttemptProgressRequest,
        fault: Option<PersistenceFaultPoint>,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        validate_progress_request(&request)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            update_attempt_progress(&transaction, &request)?;
            let (projection_changes, projection_outbox_ids) = insert_tool_projections(
                &transaction,
                runtime_instance_id,
                vec![ToolProjectionDraft::ToolExecutionAttempt(Box::new(
                    request.attempt_projection,
                ))],
            )?;
            if fault == Some(PersistenceFaultPoint::BeforeCommit) {
                return Err(PersistenceError::InvalidRequest(
                    "injected tool progress persistence failure before commit".to_owned(),
                ));
            }
            transaction.commit()?;
            Ok(ToolExecutionPersistenceCommit {
                event_ids: Vec::new(),
                projection_changes,
                projection_outbox_ids,
            })
        })
    }

    pub fn commit_tool_execution_attempt_cancellation_requested(
        &self,
        request: ToolExecutionAttemptCancellationRequest,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        self.commit_tool_execution_attempt_cancellation_requested_with_fault(request, None)
    }

    pub fn commit_tool_execution_attempt_cancellation_requested_with_fault(
        &self,
        request: ToolExecutionAttemptCancellationRequest,
        fault: Option<PersistenceFaultPoint>,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        validate_cancellation_request(&request)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let authority = verify_cancellation_authority(&transaction, &request)?;
            let event_id = EventId::new();
            let event = execution_event(
                event_id,
                authority.workspace_id,
                &authority.batch,
                request.correlation_id,
                Some(request.causation_id),
                ToolLifecycleEventV1::ToolExecutionAttemptCancellationRequested(
                    ToolExecutionAttemptCancellationRequestedPayloadV1 {
                        tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
                        reason: request.reason,
                        requested_at: request.requested_at,
                    },
                ),
                request.requested_at,
            )?;
            insert_event(&transaction, &event)?;
            transition_attempt_to_cancelling(&transaction, &request, event_id)?;
            let (projection_changes, projection_outbox_ids) = insert_tool_projections(
                &transaction,
                runtime_instance_id,
                vec![ToolProjectionDraft::ToolExecutionAttempt(Box::new(
                    request.attempt_projection,
                ))],
            )?;
            if fault == Some(PersistenceFaultPoint::BeforeCommit) {
                return Err(PersistenceError::InvalidRequest(
                    "injected tool cancellation request persistence failure before commit"
                        .to_owned(),
                ));
            }
            transaction.commit()?;
            Ok(ToolExecutionPersistenceCommit {
                event_ids: vec![event_id],
                projection_changes,
                projection_outbox_ids,
            })
        })
    }

    pub fn commit_tool_execution_attempt_failed_for_retry(
        &self,
        request: ToolExecutionAttemptRetryableFailureRequest,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        self.commit_tool_execution_attempt_failed_for_retry_with_fault(request, None)
    }

    pub fn commit_tool_execution_attempt_failed_for_retry_with_fault(
        &self,
        request: ToolExecutionAttemptRetryableFailureRequest,
        fault: Option<PersistenceFaultPoint>,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        validate_retryable_failure_request(&request)?;
        let runtime_instance_id = self.runtime_instance_id();
        let artifact_root = self.artifact_root().to_path_buf();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let completed_at =
                request.attempt.completed_at.ok_or(PersistenceError::ToolEntityIntegrity)?;
            let evidence_ref =
                request.attempt.evidence_ref.ok_or(PersistenceError::ToolEntityIntegrity)?;
            insert_artifact_write_draft_in_transaction(
                &transaction,
                &artifact_root,
                request.attempt_evidence_artifact.clone(),
            )?;
            let authority = verify_retryable_failure_authority(&transaction, &request)?;
            let event_id = EventId::new();
            let event = execution_event(
                event_id,
                authority.workspace_id,
                &authority.batch,
                request.correlation_id,
                Some(request.causation_id),
                ToolLifecycleEventV1::ToolExecutionAttemptFailed(
                    ToolExecutionAttemptFailedPayloadV1 {
                        tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
                        side_effect_state: request.attempt.side_effect_state,
                        error: request.error,
                        evidence_ref,
                        completed_at,
                    },
                ),
                completed_at,
            )?;
            insert_event(&transaction, &event)?;
            finalize_attempt(&transaction, &request.attempt, event_id)?;
            let (projection_changes, projection_outbox_ids) = insert_tool_projections(
                &transaction,
                runtime_instance_id,
                vec![ToolProjectionDraft::ToolExecutionAttempt(Box::new(
                    request.attempt_projection,
                ))],
            )?;
            if fault == Some(PersistenceFaultPoint::BeforeCommit) {
                return Err(PersistenceError::InvalidRequest(
                    "injected retryable tool attempt failure persistence failure before commit"
                        .to_owned(),
                ));
            }
            transaction.commit()?;
            Ok(ToolExecutionPersistenceCommit {
                event_ids: vec![event_id],
                projection_changes,
                projection_outbox_ids,
            })
        })
    }

    pub fn commit_tool_execution_attempt_finalized(
        &self,
        request: ToolExecutionAttemptFinalizationRequest,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        self.commit_tool_execution_attempt_finalized_with_fault(request, None)
    }

    pub fn commit_tool_execution_attempt_finalized_with_fault(
        &self,
        request: ToolExecutionAttemptFinalizationRequest,
        fault: Option<PersistenceFaultPoint>,
    ) -> Result<ToolExecutionPersistenceCommit, PersistenceError> {
        validate_attempt_finalization_request(&request)?;
        let runtime_instance_id = self.runtime_instance_id();
        let artifact_root = self.artifact_root().to_path_buf();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let completed_at =
                request.attempt.completed_at.ok_or(PersistenceError::ToolEntityIntegrity)?;

            insert_artifact_write_draft_in_transaction(
                &transaction,
                &artifact_root,
                request.attempt_evidence_artifact.clone(),
            )?;
            insert_artifact_write_draft_in_transaction(
                &transaction,
                &artifact_root,
                request.aggregate_side_effect_evidence_artifact.clone(),
            )?;
            insert_artifact_write_draft_in_transaction(
                &transaction,
                &artifact_root,
                request.model_visible_result_artifact.clone(),
            )?;
            insert_artifact_write_draft_in_transaction(
                &transaction,
                &artifact_root,
                request.user_audit_result_artifact.clone(),
            )?;

            let authority = verify_attempt_finalization_authority(&transaction, &request)?;

            insert_result_artifact_access_links(&transaction, &authority.batch, &request)?;

            let attempt_event_id = EventId::new();
            let attempt_event = execution_event(
                attempt_event_id,
                authority.workspace_id,
                &authority.batch,
                request.correlation_id,
                Some(request.causation_id),
                terminal_lifecycle_event(&request)?,
                completed_at,
            )?;
            insert_event(&transaction, &attempt_event)?;
            finalize_attempt(&transaction, &request.attempt, attempt_event_id)?;

            let result_event_id = EventId::new();
            let result_event = execution_event(
                result_event_id,
                authority.workspace_id,
                &authority.batch,
                request.correlation_id,
                Some(request.causation_id),
                ToolLifecycleEventV1::ToolResultFinalized(ToolResultFinalizedPayloadV1 {
                    tool_batch_id: authority.batch.tool_batch_id,
                    result: request.result.clone(),
                }),
                request.result.completed_at,
            )?;
            insert_event(&transaction, &result_event)?;
            insert_tool_result(&transaction, &authority.batch, &request.result, result_event_id)?;
            finalize_tool_call_and_batch(&transaction, &authority.batch, &request)?;
            if request.batch_result.is_some() {
                finalize_tool_history_atomic_group_if_pending(
                    &transaction,
                    authority.batch.tool_batch_id,
                    request.result.completed_at,
                )?;
                release_batch_payload_leases(
                    &transaction,
                    authority.batch.tool_batch_id,
                    request.result.completed_at,
                )?;
            }

            let drafts = vec![
                ToolProjectionDraft::ToolBatch(Box::new(request.batch_projection)),
                ToolProjectionDraft::ToolCall(Box::new(request.call_projection)),
                ToolProjectionDraft::ToolExecutionAttempt(Box::new(request.attempt_projection)),
                ToolProjectionDraft::SessionToolActivity(attach_older_activity_cursor(
                    &transaction,
                    request.session_activity_projection,
                )?),
            ];
            let (projection_changes, projection_outbox_ids) =
                insert_tool_projections(&transaction, runtime_instance_id, drafts)?;
            if fault == Some(PersistenceFaultPoint::BeforeCommit) {
                return Err(PersistenceError::InvalidRequest(
                    "injected tool attempt finalization persistence failure before commit"
                        .to_owned(),
                ));
            }
            transaction.commit()?;
            Ok(ToolExecutionPersistenceCommit {
                event_ids: vec![attempt_event_id, result_event_id],
                projection_changes,
                projection_outbox_ids,
            })
        })
    }

    /// Reconciles Tool execution facts left non-terminal by a previous Core
    /// process. Calls that never acquired an execution attempt are closed as
    /// pre-dispatch NotExecuted. Once DispatchPrepared was committed, an
    /// absent durable terminal is never replayed or inferred from progress;
    /// it is closed as indeterminate/unknown with internal evidence.
    ///
    /// Historical `started`-only rows predate the atomic W7 pre-dispatch
    /// boundary. Their raw records are retained and the affected authority is
    /// moved to RecoveryRequired after a stable diagnostic is committed.
    pub fn recover_incomplete_tool_executions(
        &self,
    ) -> Result<ToolExecutionStartupRecoveryReport, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        let artifact_root = self.artifact_root().to_path_buf();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut report = ToolExecutionStartupRecoveryReport::default();
            let batch_ids = load_recoverable_batch_ids(&transaction)?;

            for batch_id in batch_ids {
                let batch = load_batch_request(&transaction, batch_id)?;
                let workspace_id =
                    workspace_for_session(&transaction, batch.invocation_identity.session_id)?;
                let calls = load_recovery_calls(&transaction, &batch)?;
                let started_only = calls
                    .iter()
                    .flat_map(|call| &call.attempts)
                    .filter(|attempt| {
                        attempt.record.status == ToolExecutionAttemptStatusV1::Started
                    })
                    .collect::<Vec<_>>();
                if !started_only.is_empty() {
                    for attempt in &started_only {
                        insert_execution_recovery_diagnostic(
                            &transaction,
                            &attempt.record,
                            "historical_started_only",
                            UtcTimestamp::now(),
                        )?;
                    }
                    mark_execution_authority_recovery_required(&transaction, batch_id)?;
                    report.historical_started_only_attempts += started_only.len();
                    continue;
                }

                let recovered_at = UtcTimestamp::now();
                let correlation_id = CorrelationId::new();
                let mut changed_attempts = Vec::new();
                let mut changed_call_ids = BTreeSet::new();

                for call in &calls {
                    if call.result.is_some() {
                        continue;
                    }
                    let active = call
                        .attempts
                        .iter()
                        .filter(|attempt| !attempt.record.status.is_terminal())
                        .collect::<Vec<_>>();
                    if active.len() > 1 {
                        return Err(PersistenceError::ToolEntityIntegrity);
                    }
                    if let Some(active) = active.first() {
                        if active.record.dispatch_prepared_at.is_none()
                            || active.dispatch_prepared_event_id.is_none()
                        {
                            return Err(PersistenceError::ToolEntityIntegrity);
                        }
                        let recovered = reconcile_dispatch_unknown_attempt(
                            &transaction,
                            DispatchUnknownReconciliationInputV1 {
                                artifact_root: &artifact_root,
                                workspace_id,
                                batch: &batch,
                                call,
                                active,
                                correlation_id,
                                recovered_at,
                            },
                        )?;
                        changed_attempts.push(recovered);
                        changed_call_ids.insert(call.call.tool_call_id);
                        report.dispatch_unknown_attempts_terminalized += 1;
                    } else if call.attempts.is_empty() {
                        terminalize_pre_dispatch_call(
                            &transaction,
                            workspace_id,
                            &batch,
                            call,
                            correlation_id,
                            recovered_at,
                        )?;
                        changed_call_ids.insert(call.call.tool_call_id);
                        report.pre_dispatch_calls_terminalized += 1;
                    }
                }

                if changed_call_ids.is_empty() {
                    continue;
                }
                let completed =
                    complete_recovered_batch_if_terminal(&transaction, &batch, recovered_at)?;
                if completed {
                    release_batch_payload_leases(&transaction, batch.tool_batch_id, recovered_at)?;
                    report.completed_batches += 1;
                }
                let drafts = build_execution_recovery_projections(
                    &transaction,
                    &batch,
                    &changed_call_ids,
                    &changed_attempts,
                )?;
                let _ = insert_tool_projections(&transaction, runtime_instance_id, drafts)?;
            }

            transaction.commit()?;
            if report.historical_started_only_attempts > 0 {
                return Err(PersistenceError::ToolExecutionRecoveryRequired);
            }
            Ok(report)
        })
    }
}

#[derive(Clone)]
struct RecoveryAttemptRow {
    record: ToolExecutionAttemptRecordV1,
    dispatch_prepared_event_id: Option<EventId>,
    progress: Option<aura_tool_projection::ToolAttemptProgressView>,
}

#[derive(Clone)]
struct RecoveryCallRow {
    tool_batch_id: aura_domain_common::ToolBatchId,
    call: aura_tool_contract::NormalizedToolCallV1,
    status: ToolCallStatusV1,
    display_name: String,
    started_at: Option<UtcTimestamp>,
    result: Option<ToolResultV1>,
    attempts: Vec<RecoveryAttemptRow>,
}

fn load_recoverable_batch_ids(
    transaction: &Transaction<'_>,
) -> Result<Vec<aura_domain_common::ToolBatchId>, PersistenceError> {
    transaction
        .prepare(
            "SELECT tool_batch_id FROM tool_batches
             WHERE status IN ('admitted', 'running', 'cancelling')
             ORDER BY created_at, tool_batch_id",
        )?
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|value| {
            value?.parse::<aura_domain_common::ToolBatchId>().map_err(PersistenceError::from)
        })
        .collect()
}

fn load_batch_request(
    transaction: &Transaction<'_>,
    tool_batch_id: aura_domain_common::ToolBatchId,
) -> Result<ToolBatchRequestV1, PersistenceError> {
    let encoded = transaction
        .query_row(
            "SELECT request_json FROM tool_batches WHERE tool_batch_id=?1",
            [tool_batch_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let batch = decode_json::<ToolBatchRequestV1>(&encoded)?;
    if batch.tool_batch_id != tool_batch_id {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(batch)
}

fn load_recovery_calls(
    transaction: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
) -> Result<Vec<RecoveryCallRow>, PersistenceError> {
    let mut calls = Vec::with_capacity(batch.calls.len());
    for expected in &batch.calls {
        let row = transaction
            .query_row(
                "SELECT normalized_call_json, normalized_call_hash_json, status, started_at
                 FROM tool_calls WHERE tool_call_id=?1 AND tool_batch_id=?2",
                params![expected.tool_call_id.to_string(), batch.tool_batch_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or(PersistenceError::ToolEntityNotFound)?;
        let call =
            decode_verified_record::<aura_tool_contract::NormalizedToolCallV1>(&row.0, &row.1)?;
        if call != *expected {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        let result = transaction
            .query_row(
                "SELECT result_json, result_hash_json FROM tool_results
                 WHERE tool_call_id=?1 AND tool_batch_id=?2",
                params![call.tool_call_id.to_string(), batch.tool_batch_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .map(|(json, hash)| decode_verified_record::<ToolResultV1>(&json, &hash))
            .transpose()?;
        let display_name = transaction
            .query_row(
                "SELECT record_json, record_hash_json FROM tool_definitions
                 WHERE tool_id=?1 AND tool_version_json=?2",
                params![
                    resolved_tool(&call)?.tool_id.to_string(),
                    encode_json(&resolved_tool(&call)?.tool_version)?,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .map(|(json, hash)| {
                decode_verified_record::<ToolDefinitionRecordV1>(&json, &hash)
                    .map(|definition| definition.display_name)
            })
            .transpose()?
            .unwrap_or_else(|| "Unavailable tool".to_owned());
        let attempt_rows = {
            let mut statement = transaction.prepare(
                "SELECT a.record_json, a.record_hash_json, a.dispatch_prepared_event_id,
                        o.progress_json
                 FROM tool_execution_attempts a
                 LEFT JOIN tool_attempt_operational_state o
                   USING (tool_execution_attempt_id)
                 WHERE a.tool_call_id=?1 ORDER BY a.attempt_number",
            )?;
            statement
                .query_map([call.tool_call_id.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let attempts = attempt_rows
            .into_iter()
            .map(|row| {
                Ok(RecoveryAttemptRow {
                    record: decode_verified_record::<ToolExecutionAttemptRecordV1>(&row.0, &row.1)?,
                    dispatch_prepared_event_id: row
                        .2
                        .as_deref()
                        .map(str::parse::<EventId>)
                        .transpose()?,
                    progress: row
                        .3
                        .as_deref()
                        .map(decode_json::<aura_tool_projection::ToolAttemptProgressView>)
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>, PersistenceError>>()?;
        calls.push(RecoveryCallRow {
            tool_batch_id: batch.tool_batch_id,
            call,
            status: parse_call_status(&row.2)?,
            display_name,
            started_at: row.3.as_deref().map(parse_timestamp).transpose()?,
            result,
            attempts,
        });
    }
    Ok(calls)
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ExecutionRecoveryEvidenceV1 {
    recovery_code: &'static str,
    tool_execution_attempt_id: aura_domain_common::ToolExecutionAttemptId,
    tool_call_id: ToolCallId,
    original_status: ToolExecutionAttemptStatusV1,
    dispatch_prepared_event_id: Option<EventId>,
    last_progress_sequence: u64,
    detected_at: UtcTimestamp,
}

fn insert_execution_recovery_diagnostic(
    transaction: &Transaction<'_>,
    attempt: &ToolExecutionAttemptRecordV1,
    recovery_code: &'static str,
    detected_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let dispatch_prepared_event_id = transaction
        .query_row(
            "SELECT dispatch_prepared_event_id FROM tool_execution_attempts
             WHERE tool_execution_attempt_id=?1",
            [attempt.tool_execution_attempt_id.to_string()],
            |row| row.get::<_, Option<String>>(0),
        )?
        .as_deref()
        .map(str::parse::<EventId>)
        .transpose()?;
    let evidence = ExecutionRecoveryEvidenceV1 {
        recovery_code,
        tool_execution_attempt_id: attempt.tool_execution_attempt_id,
        tool_call_id: attempt.tool_call_id,
        original_status: attempt.status,
        dispatch_prepared_event_id,
        last_progress_sequence: attempt.last_progress_sequence,
        detected_at,
    };
    let evidence_hash = aura_domain_common::jcs::canonical_sha256(&evidence)
        .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    transaction.execute(
        "INSERT INTO tool_execution_recovery_diagnostics(
            tool_execution_attempt_id, recovery_code, evidence_hash_json, detected_at
         ) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(tool_execution_attempt_id) DO UPDATE SET
            recovery_code=excluded.recovery_code,
            evidence_hash_json=excluded.evidence_hash_json,
            detected_at=excluded.detected_at",
        params![
            attempt.tool_execution_attempt_id.to_string(),
            recovery_code,
            encode_json(&evidence_hash)?,
            detected_at.to_string(),
        ],
    )?;
    Ok(())
}

fn mark_execution_authority_recovery_required(
    transaction: &Transaction<'_>,
    tool_batch_id: aura_domain_common::ToolBatchId,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "UPDATE tool_calls SET status='recovery_required',
            entity_revision=entity_revision+1, updated_at=?1
         WHERE tool_batch_id=?2 AND status <> 'finalized'",
        params![UtcTimestamp::now().to_string(), tool_batch_id.to_string()],
    )?;
    let changed = transaction.execute(
        "UPDATE tool_batches SET status='recovery_required',
            entity_revision=entity_revision+1, updated_at=?1
         WHERE tool_batch_id=?2 AND status IN ('admitted', 'running', 'cancelling')",
        params![UtcTimestamp::now().to_string(), tool_batch_id.to_string()],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(())
}

struct DispatchUnknownReconciliationInputV1<'a> {
    artifact_root: &'a std::path::Path,
    workspace_id: WorkspaceId,
    batch: &'a ToolBatchRequestV1,
    call: &'a RecoveryCallRow,
    active: &'a RecoveryAttemptRow,
    correlation_id: CorrelationId,
    recovered_at: UtcTimestamp,
}

fn reconcile_dispatch_unknown_attempt(
    transaction: &Transaction<'_>,
    input: DispatchUnknownReconciliationInputV1<'_>,
) -> Result<ToolExecutionAttemptProjection, PersistenceError> {
    let DispatchUnknownReconciliationInputV1 {
        artifact_root,
        workspace_id,
        batch,
        call,
        active,
        correlation_id,
        recovered_at,
    } = input;
    let dispatch_event_id =
        active.dispatch_prepared_event_id.ok_or(PersistenceError::ToolEntityIntegrity)?;
    mark_dispatch_binding_recovery_required(
        transaction,
        active.record.tool_execution_attempt_id,
        recovered_at,
    )?;
    let evidence = ExecutionRecoveryEvidenceV1 {
        recovery_code: "dispatch_outcome_unknown",
        tool_execution_attempt_id: active.record.tool_execution_attempt_id,
        tool_call_id: active.record.tool_call_id,
        original_status: active.record.status,
        dispatch_prepared_event_id: Some(dispatch_event_id),
        last_progress_sequence: active.record.last_progress_sequence,
        detected_at: recovered_at,
    };
    let evidence_bytes = serde_json::to_vec(&evidence)?;
    let attempt_evidence_id = ArtifactId::new();
    insert_artifact_in_transaction(
        transaction,
        artifact_root,
        attempt_evidence_id,
        ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/vnd.aura.tool-execution-recovery+json".to_owned(),
            access: ArtifactAccess::NONE,
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ExecutionAttempt {
                    tool_execution_attempt_id: active.record.tool_execution_attempt_id,
                },
            },
            bytes: evidence_bytes,
        },
    )?;
    let aggregate_evidence_id = ArtifactId::new();
    let attempt_ids = call
        .attempts
        .iter()
        .map(|attempt| attempt.record.tool_execution_attempt_id.to_string())
        .collect::<Vec<_>>();
    insert_artifact_in_transaction(
        transaction,
        artifact_root,
        aggregate_evidence_id,
        ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/vnd.aura.tool-side-effect-reconciliation+json".to_owned(),
            access: ArtifactAccess::NONE,
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ToolCall { tool_call_id: call.call.tool_call_id },
            },
            bytes: serde_json::to_vec(&serde_json::json!({
                "aggregate": "unknown",
                "reason": "dispatch_outcome_unknown",
                "attempt_ids": attempt_ids,
                "reconciled_at": recovered_at,
            }))?,
        },
    )?;

    let causation_id = CausationId::from_ulid(dispatch_event_id.ulid());
    let error = ErrorEnvelope::new(
        aura_tool_contract::ToolRuntimeErrorCode::PersistenceOutcomeUnknown.error_code(),
        ErrorCategory::Internal,
        "工具运行在持久化 DispatchPrepared 后中断，实际执行结果无法可靠确认。",
        false,
        correlation_id,
        Some(causation_id),
    )
    .with_detail(
        "tool_execution_attempt_id",
        serde_json::Value::String(active.record.tool_execution_attempt_id.to_string()),
    );
    let mut terminal_attempt = active.record.clone();
    terminal_attempt.status = ToolExecutionAttemptStatusV1::Indeterminate;
    terminal_attempt.side_effect_state = SideEffectStateV1::Unknown;
    terminal_attempt.evidence_ref = Some(attempt_evidence_id);
    terminal_attempt.completed_at = Some(recovered_at);
    terminal_attempt.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;

    let attempts = call
        .attempts
        .iter()
        .map(|attempt| {
            let record = if attempt.record.tool_execution_attempt_id
                == terminal_attempt.tool_execution_attempt_id
            {
                &terminal_attempt
            } else {
                &attempt.record
            };
            if !record.status.is_terminal() {
                return Err(PersistenceError::ToolEntityStateConflict);
            }
            Ok(aura_tool_contract::ToolExecutionAttemptOutcomeRefV1 {
                tool_execution_attempt_id: record.tool_execution_attempt_id,
                attempt_number: record.attempt_number,
                side_effect_state: record.side_effect_state,
                side_effect_evidence_ref: record.evidence_ref,
                dispatched: true,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    let result = ToolResultV1 {
        tool_call_id: call.call.tool_call_id,
        outcome: ToolResultOutcomeV1::Indeterminate {
            reason: ToolIndeterminateReasonV1::DispatchOutcomeUnknown,
            error: error.clone(),
        },
        side_effect_state: SideEffectStateV1::Unknown,
        attempts,
        model_visible_result_ref: None,
        user_audit_result_ref: None,
        internal_diagnostic_ref: Some(attempt_evidence_id),
        aggregate_side_effect_evidence_ref: Some(aggregate_evidence_id),
        started_at: call.started_at,
        completed_at: recovered_at,
    };
    result.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;

    let attempt_event_id = EventId::new();
    let attempt_event = execution_event(
        attempt_event_id,
        workspace_id,
        batch,
        correlation_id,
        Some(causation_id),
        ToolLifecycleEventV1::ToolExecutionAttemptIndeterminate(
            ToolExecutionAttemptIndeterminatePayloadV1 {
                tool_execution_attempt_id: terminal_attempt.tool_execution_attempt_id,
                reason: ToolIndeterminateReasonV1::DispatchOutcomeUnknown,
                side_effect_state: SideEffectStateV1::Unknown,
                error: error.clone(),
                evidence_ref: attempt_evidence_id,
                completed_at: recovered_at,
            },
        ),
        recovered_at,
    )?;
    insert_event(transaction, &attempt_event)?;
    finalize_attempt(transaction, &terminal_attempt, attempt_event_id)?;

    let result_event_id = EventId::new();
    let result_event = execution_event(
        result_event_id,
        workspace_id,
        batch,
        correlation_id,
        Some(CausationId::from_ulid(attempt_event_id.ulid())),
        ToolLifecycleEventV1::ToolResultFinalized(ToolResultFinalizedPayloadV1 {
            tool_batch_id: batch.tool_batch_id,
            result: result.clone(),
        }),
        recovered_at,
    )?;
    insert_event(transaction, &result_event)?;
    insert_tool_result(transaction, batch, &result, result_event_id)?;
    finalize_recovered_call(transaction, call.call.tool_call_id, recovered_at)?;
    insert_execution_recovery_diagnostic(
        transaction,
        &active.record,
        "dispatch_outcome_unknown",
        recovered_at,
    )?;

    Ok(ToolExecutionAttemptProjection {
        tool_execution_attempt_id: terminal_attempt.tool_execution_attempt_id,
        tool_call_id: terminal_attempt.tool_call_id,
        attempt_number: terminal_attempt.attempt_number,
        status: terminal_attempt.status,
        side_effect_state: terminal_attempt.side_effect_state,
        progress: active.progress.clone(),
        started_at: terminal_attempt.started_at,
        dispatch_prepared_at: terminal_attempt.dispatch_prepared_at,
        completed_at: terminal_attempt.completed_at,
        safe_error: Some(projection_error(&error)),
        user_readable_artifacts: Vec::new(),
        can_cancel: false,
    })
}

fn mark_dispatch_binding_recovery_required(
    transaction: &Transaction<'_>,
    tool_execution_attempt_id: aura_domain_common::ToolExecutionAttemptId,
    recovered_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "UPDATE tool_sandbox_dispatch_bindings
         SET state='recovery_required', updated_at=?1
         WHERE tool_execution_attempt_id=?2
           AND state IN ('dispatch_prepared', 'consumed')",
        params![recovered_at.to_string(), tool_execution_attempt_id.to_string()],
    )?;
    Ok(())
}

fn terminalize_pre_dispatch_call(
    transaction: &Transaction<'_>,
    workspace_id: WorkspaceId,
    batch: &ToolBatchRequestV1,
    call: &RecoveryCallRow,
    correlation_id: CorrelationId,
    recovered_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    if matches!(call.status, ToolCallStatusV1::Finalized | ToolCallStatusV1::RecoveryRequired) {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    let result = ToolResultV1 {
        tool_call_id: call.call.tool_call_id,
        outcome: ToolResultOutcomeV1::NotExecuted {
            reason: aura_tool_contract::ToolNotExecutedReasonV1::CancelledBeforeDispatch,
            error: None,
        },
        side_effect_state: SideEffectStateV1::NotStarted,
        attempts: Vec::new(),
        model_visible_result_ref: None,
        user_audit_result_ref: None,
        internal_diagnostic_ref: None,
        aggregate_side_effect_evidence_ref: None,
        started_at: None,
        completed_at: recovered_at,
    };
    result.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    let result_event_id = EventId::new();
    let event = execution_event(
        result_event_id,
        workspace_id,
        batch,
        correlation_id,
        None,
        ToolLifecycleEventV1::ToolResultFinalized(ToolResultFinalizedPayloadV1 {
            tool_batch_id: batch.tool_batch_id,
            result: result.clone(),
        }),
        recovered_at,
    )?;
    insert_event(transaction, &event)?;
    insert_tool_result(transaction, batch, &result, result_event_id)?;
    finalize_recovered_call(transaction, call.call.tool_call_id, recovered_at)
}

fn finalize_recovered_call(
    transaction: &Transaction<'_>,
    tool_call_id: ToolCallId,
    completed_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let changed = transaction.execute(
        "UPDATE tool_calls SET status='finalized', entity_revision=entity_revision+1,
            completed_at=?1, updated_at=?1
         WHERE tool_call_id=?2 AND status NOT IN ('finalized', 'recovery_required')",
        params![completed_at.to_string(), tool_call_id.to_string()],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(())
}

fn complete_recovered_batch_if_terminal(
    transaction: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    completed_at: UtcTimestamp,
) -> Result<bool, PersistenceError> {
    let results = load_batch_results(transaction, batch.tool_batch_id)?;
    if results.len() != batch.calls.len() {
        transaction.execute(
            "UPDATE tool_batches SET entity_revision=entity_revision+1, updated_at=?1
             WHERE tool_batch_id=?2 AND status IN ('admitted', 'running', 'cancelling')",
            params![completed_at.to_string(), batch.tool_batch_id.to_string()],
        )?;
        return Ok(false);
    }
    let batch_result = ToolBatchResultV1 {
        tool_batch_id: batch.tool_batch_id,
        status: ToolBatchStatusV1::Completed,
        results,
        completed_at,
    };
    batch_result.validate(batch).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    let (result_json, result_hash_json) = encode_record(&batch_result)?;
    let changed = transaction.execute(
        "UPDATE tool_batches SET status='completed', result_json=?1, result_hash_json=?2,
            entity_revision=entity_revision+1, completed_at=?3, updated_at=?3
         WHERE tool_batch_id=?4 AND status IN ('admitted', 'running', 'cancelling')
           AND result_json IS NULL",
        params![
            result_json,
            result_hash_json,
            completed_at.to_string(),
            batch.tool_batch_id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    finalize_tool_history_atomic_group_if_pending(transaction, batch.tool_batch_id, completed_at)?;
    Ok(true)
}

fn load_batch_results(
    transaction: &Transaction<'_>,
    tool_batch_id: aura_domain_common::ToolBatchId,
) -> Result<Vec<ToolResultV1>, PersistenceError> {
    transaction
        .prepare(
            "SELECT result_json, result_hash_json FROM tool_results
             WHERE tool_batch_id=?1 ORDER BY ordinal",
        )?
        .query_map([tool_batch_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .map(|row| {
            let (json, hash) = row?;
            decode_verified_record::<ToolResultV1>(&json, &hash)
        })
        .collect()
}

fn build_execution_recovery_projections(
    transaction: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    changed_call_ids: &BTreeSet<ToolCallId>,
    changed_attempts: &[ToolExecutionAttemptProjection],
) -> Result<Vec<ToolProjectionDraft>, PersistenceError> {
    let calls = load_recovery_calls(transaction, batch)?;
    let (batch_status, started_at, completed_at) = transaction.query_row(
        "SELECT status, started_at, completed_at FROM tool_batches WHERE tool_batch_id=?1",
        [batch.tool_batch_id.to_string()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    let batch_status = parse_batch_status(&batch_status)?;
    let started_at = started_at.as_deref().map(parse_timestamp).transpose()?;
    let completed_at = completed_at.as_deref().map(parse_timestamp).transpose()?;
    let summaries = calls.iter().map(recovery_call_summary).collect::<Result<Vec<_>, _>>()?;
    let completed_call_count = u32::try_from(
        summaries.iter().filter(|call| call.status == ToolCallStatusV1::Finalized).count(),
    )
    .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    let can_cancel =
        !matches!(batch_status, ToolBatchStatusV1::Completed | ToolBatchStatusV1::RecoveryRequired);
    let batch_projection = ToolBatchProjection {
        tool_batch_id: batch.tool_batch_id,
        batch_content_hash: batch.batch_content_hash.clone(),
        session_id: batch.invocation_identity.session_id,
        turn_task_id: batch.invocation_identity.turn_task_id,
        agent_run_id: batch.invocation_identity.agent_run_id,
        model_step_id: batch.invocation_identity.model_step_id,
        registry_snapshot_ref: batch.registry_snapshot_ref.clone(),
        control_state_revision: batch.p08_state_revision,
        control_state_hash: batch.p08_state_hash.clone(),
        status: batch_status,
        calls: summaries.clone(),
        started_at,
        completed_at,
        safe_error: None,
        can_cancel,
    };
    let active_batches = if can_cancel {
        vec![ToolBatchActivitySummaryView {
            tool_batch_id: batch.tool_batch_id,
            status: batch_status,
            call_count: u32::try_from(calls.len())
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
            completed_call_count,
            started_at,
            completed_at,
            can_cancel,
        }]
    } else {
        Vec::new()
    };
    let session_projection = SessionToolActivityProjection {
        session_id: batch.invocation_identity.session_id,
        active_batches,
        recent_calls: summaries,
        older_activity_cursor: None,
    };
    let mut drafts = vec![
        ToolProjectionDraft::ToolBatch(Box::new(batch_projection)),
        ToolProjectionDraft::SessionToolActivity(attach_older_activity_cursor(
            transaction,
            session_projection,
        )?),
    ];
    for call in &calls {
        if changed_call_ids.contains(&call.call.tool_call_id) {
            drafts.push(ToolProjectionDraft::ToolCall(Box::new(recovery_call_projection(call)?)));
        }
    }
    for attempt in changed_attempts {
        drafts.push(ToolProjectionDraft::ToolExecutionAttempt(Box::new(attempt.clone())));
    }
    Ok(drafts)
}

fn projection_error(error: &ErrorEnvelope) -> ToolProjectionErrorView {
    ToolProjectionErrorView {
        code: error.code.clone(),
        category: error.category,
        message: error.message.clone(),
        retryable: error.retryable,
    }
}

fn parse_call_status(value: &str) -> Result<ToolCallStatusV1, PersistenceError> {
    match value {
        "pending_admission" => Ok(ToolCallStatusV1::PendingAdmission),
        "waiting_dependency" => Ok(ToolCallStatusV1::WaitingDependency),
        "waiting_permission" => Ok(ToolCallStatusV1::WaitingPermission),
        "waiting_approval" => Ok(ToolCallStatusV1::WaitingApproval),
        "waiting_resource" => Ok(ToolCallStatusV1::WaitingResource),
        "queued" => Ok(ToolCallStatusV1::Queued),
        "running" => Ok(ToolCallStatusV1::Running),
        "cancelling" => Ok(ToolCallStatusV1::Cancelling),
        "finalized" => Ok(ToolCallStatusV1::Finalized),
        "recovery_required" => Ok(ToolCallStatusV1::RecoveryRequired),
        _ => Err(PersistenceError::ToolEntityIntegrity),
    }
}

fn parse_batch_status(value: &str) -> Result<ToolBatchStatusV1, PersistenceError> {
    match value {
        "pending_admission" => Ok(ToolBatchStatusV1::PendingAdmission),
        "admitted" => Ok(ToolBatchStatusV1::Admitted),
        "running" => Ok(ToolBatchStatusV1::Running),
        "cancelling" => Ok(ToolBatchStatusV1::Cancelling),
        "completed" => Ok(ToolBatchStatusV1::Completed),
        "recovery_required" => Ok(ToolBatchStatusV1::RecoveryRequired),
        _ => Err(PersistenceError::ToolEntityIntegrity),
    }
}

struct AttemptStartAuthority {
    workspace_id: WorkspaceId,
    batch: ToolBatchRequestV1,
}

struct CancellationAuthority {
    workspace_id: WorkspaceId,
    batch: ToolBatchRequestV1,
}

struct AttemptFinalizationAuthority {
    workspace_id: WorkspaceId,
    batch: ToolBatchRequestV1,
}

struct RetryableFailureAuthority {
    workspace_id: WorkspaceId,
    batch: ToolBatchRequestV1,
}

fn validate_dispatch_prepared_commit_request(
    request: &ToolExecutionAttemptDispatchPreparedCommitRequest,
) -> Result<(), PersistenceError> {
    request.attempt.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if request.attempt.status != ToolExecutionAttemptStatusV1::DispatchPrepared
        || request.attempt.side_effect_state != SideEffectStateV1::NotStarted
        || request.attempt.last_progress_sequence != 0
        || request.expected_batch_revision.value() == 0
        || request.expected_call_revision.value() == 0
        || request.dispatch_evidence_hash.algorithm() != ContentHashAlgorithm::Sha256
        || request.attempt.dispatch_prepared_at.is_none()
        || request.attempt.dispatch_prepared_at < Some(request.attempt.started_at)
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let attempt_projection = &request.attempt_projection;
    if attempt_projection.tool_execution_attempt_id != request.attempt.tool_execution_attempt_id
        || attempt_projection.tool_call_id != request.attempt.tool_call_id
        || attempt_projection.attempt_number != request.attempt.attempt_number
        || attempt_projection.status != ToolExecutionAttemptStatusV1::DispatchPrepared
        || attempt_projection.side_effect_state != SideEffectStateV1::NotStarted
        || attempt_projection.progress.is_some()
        || attempt_projection.started_at != request.attempt.started_at
        || attempt_projection.dispatch_prepared_at != request.attempt.dispatch_prepared_at
        || attempt_projection.completed_at.is_some()
        || attempt_projection.safe_error.is_some()
        || !attempt_projection.user_readable_artifacts.is_empty()
        || !attempt_projection.can_cancel
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    if request.call_projection.tool_call_id != request.attempt.tool_call_id
        || request.call_projection.status != ToolCallStatusV1::Running
        || request.call_projection.outcome.is_some()
        || request.call_projection.side_effect_state.is_some()
        || request.call_projection.completed_at.is_some()
        || request.call_projection.started_at.is_none()
        || request.call_projection.attempt_ids.last()
            != Some(&request.attempt.tool_execution_attempt_id)
        || !request.call_projection.can_cancel
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let batch_projection = &request.batch_projection;
    if batch_projection.tool_batch_id != request.call_projection.tool_batch_id
        || batch_projection.status != ToolBatchStatusV1::Running
        || batch_projection.completed_at.is_some()
        || batch_projection.started_at.is_none()
        || !batch_projection.can_cancel
        || !batch_projection.calls.iter().any(|summary| {
            summary.tool_call_id == request.attempt.tool_call_id
                && summary.status == ToolCallStatusV1::Running
                && summary.started_at == request.call_projection.started_at
                && summary.outcome.is_none()
                && summary.side_effect_state.is_none()
        })
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    if request.session_activity_projection.session_id != batch_projection.session_id
        || !request.session_activity_projection.active_batches.iter().any(|summary| {
            summary.tool_batch_id == batch_projection.tool_batch_id
                && summary.status == ToolBatchStatusV1::Running
                && summary.call_count
                    == u32::try_from(batch_projection.calls.len()).unwrap_or(u32::MAX)
        })
        || !request.session_activity_projection.recent_calls.iter().any(|summary| {
            summary.tool_call_id == request.attempt.tool_call_id
                && summary.status == ToolCallStatusV1::Running
        })
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn validate_authorized_dispatch_prepared_request(
    request: &AuthorizedToolExecutionAttemptDispatchPreparedCommitRequestV1,
) -> Result<(), PersistenceError> {
    let attempt = &request.attempt_commit.attempt;
    let binding = &request.sandbox_dispatch_binding;
    binding.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if binding.tool_execution_attempt_id != attempt.tool_execution_attempt_id
        || binding.tool_call_id != attempt.tool_call_id
        || binding.tool_batch_id != request.attempt_commit.batch_projection.tool_batch_id
        || binding.execution_plan_hash != request.attempt_commit.execution_plan_hash
        || binding.prepared_at
            != attempt.dispatch_prepared_at.ok_or(PersistenceError::ToolEntityIntegrity)?
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn validate_authorized_builtin_dispatch_prepared_request(
    request: &AuthorizedBuiltinCoreHandlerAttemptDispatchPreparedCommitRequestV1,
) -> Result<(), PersistenceError> {
    let attempt = &request.attempt_commit.attempt;
    let binding = &request.builtin_dispatch_binding;
    binding.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if binding.tool_execution_attempt_id != attempt.tool_execution_attempt_id
        || binding.tool_call_id != attempt.tool_call_id
        || binding.tool_batch_id != request.attempt_commit.batch_projection.tool_batch_id
        || binding.execution_plan_hash != request.attempt_commit.execution_plan_hash
        || binding.prepared_at
            != attempt.dispatch_prepared_at.ok_or(PersistenceError::ToolEntityIntegrity)?
        || request.attempt_commit.dispatch_evidence_hash != binding.binding_hash
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn validate_runtime_started_request(
    request: &ToolExecutionAttemptRuntimeStartedRequest,
) -> Result<(), PersistenceError> {
    request.attempt.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if request.attempt.status != ToolExecutionAttemptStatusV1::Running
        || request.attempt.side_effect_state == SideEffectStateV1::NotStarted
        || request.updated_at < request.attempt.started_at
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    validate_attempt_projection_against_record(&request.attempt_projection, &request.attempt)?;
    if request.attempt_projection.progress.is_some()
        || request.attempt_projection.completed_at.is_some()
        || !request.attempt_projection.can_cancel
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn validate_progress_request(
    request: &ToolExecutionAttemptProgressRequest,
) -> Result<(), PersistenceError> {
    let projection = &request.attempt_projection;
    let progress = projection.progress.as_ref().ok_or(PersistenceError::ToolEntityIntegrity)?;
    if progress.sequence == 0
        || progress.updated_at < projection.started_at
        || !matches!(
            projection.status,
            ToolExecutionAttemptStatusV1::Running
                | ToolExecutionAttemptStatusV1::WaitingExternal
                | ToolExecutionAttemptStatusV1::Cancelling
        )
        || projection.side_effect_state == SideEffectStateV1::NotStarted
        || projection.dispatch_prepared_at.is_none()
        || projection.completed_at.is_some()
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn validate_cancellation_request(
    request: &ToolExecutionAttemptCancellationRequest,
) -> Result<(), PersistenceError> {
    request.attempt.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if request.attempt.status != ToolExecutionAttemptStatusV1::Cancelling
        || request.requested_at < request.attempt.started_at
        || request
            .attempt
            .dispatch_prepared_at
            .as_ref()
            .is_none_or(|prepared_at| request.requested_at < *prepared_at)
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    validate_attempt_projection_against_record(&request.attempt_projection, &request.attempt)?;
    if request.attempt_projection.completed_at.is_some() || request.attempt_projection.can_cancel {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn validate_retryable_failure_request(
    request: &ToolExecutionAttemptRetryableFailureRequest,
) -> Result<(), PersistenceError> {
    request.attempt.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if request.attempt.status != ToolExecutionAttemptStatusV1::Failed {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    validate_attempt_projection_against_record(&request.attempt_projection, &request.attempt)?;
    let failed = ToolResultOutcomeV1::Failed { error: request.error.clone() };
    if request.attempt_projection.progress.as_ref().map_or(0, |progress| progress.sequence)
        != request.attempt.last_progress_sequence
        || !projection_error_matches(&failed, request.attempt_projection.safe_error.as_ref())
        || request.attempt_projection.can_cancel
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    validate_terminal_artifact_draft(
        &request.attempt_evidence_artifact,
        request.attempt.evidence_ref.ok_or(PersistenceError::ToolEntityIntegrity)?,
        ArtifactOwnerRef::Tool {
            owner: ToolArtifactOwnerRef::ExecutionAttempt {
                tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
            },
        },
        "application/vnd.aura.tool-attempt-evidence.v1+json",
        ArtifactAccess::NONE,
        request.attempt.completed_at.ok_or(PersistenceError::ToolEntityIntegrity)?,
    )?;
    Ok(())
}

fn validate_attempt_finalization_request(
    request: &ToolExecutionAttemptFinalizationRequest,
) -> Result<(), PersistenceError> {
    request.attempt.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    request.result.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if !request.attempt.status.is_terminal()
        || request.expected_batch_revision.value() == 0
        || request.expected_call_revision.value() == 0
        || request.result.tool_call_id != request.attempt.tool_call_id
        || request.result.completed_at
            != request.attempt.completed_at.ok_or(PersistenceError::ToolEntityIntegrity)?
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    if request.result_artifact_access_links.len() < 2
        || request.result.model_visible_result_ref.is_none()
        || request.result.user_audit_result_ref.is_none()
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    validate_finalization_artifact_drafts(request)?;
    validate_terminal_fact_alignment(request)?;

    let expected_outcome = outcome_summary(&request.result.outcome);
    validate_attempt_projection_against_record(&request.attempt_projection, &request.attempt)?;
    if request.attempt_projection.progress.as_ref().map_or(0, |progress| progress.sequence)
        != request.attempt.last_progress_sequence
        || request.attempt_projection.can_cancel
        || !projection_error_matches(
            &request.result.outcome,
            request.attempt_projection.safe_error.as_ref(),
        )
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let expected_attempt_ids = request
        .result
        .attempts
        .iter()
        .map(|attempt| attempt.tool_execution_attempt_id)
        .collect::<Vec<_>>();
    if request.call_projection.tool_call_id != request.attempt.tool_call_id
        || request.call_projection.status != ToolCallStatusV1::Finalized
        || request.call_projection.outcome != Some(expected_outcome)
        || request.call_projection.side_effect_state != Some(request.result.side_effect_state)
        || request.call_projection.attempt_ids != expected_attempt_ids
        || request.call_projection.started_at != request.result.started_at
        || request.call_projection.completed_at != Some(request.result.completed_at)
        || !projection_error_matches(
            &request.result.outcome,
            request.call_projection.safe_error.as_ref(),
        )
        || request.call_projection.can_cancel
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }

    let batch_call = request
        .batch_projection
        .calls
        .iter()
        .find(|summary| summary.tool_call_id == request.result.tool_call_id)
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    if batch_call.tool_batch_id != request.batch_projection.tool_batch_id
        || batch_call.status != ToolCallStatusV1::Finalized
        || batch_call.outcome != Some(expected_outcome)
        || batch_call.side_effect_state != Some(request.result.side_effect_state)
        || batch_call.started_at != request.result.started_at
        || batch_call.completed_at != Some(request.result.completed_at)
        || batch_call.can_cancel
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    match &request.batch_result {
        Some(batch_result)
            if batch_result.tool_batch_id == request.batch_projection.tool_batch_id
                && request.batch_projection.status == ToolBatchStatusV1::Completed
                && request.batch_projection.completed_at == Some(batch_result.completed_at)
                && !request.batch_projection.can_cancel
                && !request
                    .session_activity_projection
                    .active_batches
                    .iter()
                    .any(|summary| summary.tool_batch_id == batch_result.tool_batch_id) => {}
        None if request.batch_projection.status == ToolBatchStatusV1::Running
            && request.batch_projection.completed_at.is_none()
            && request.batch_projection.can_cancel
            && request.session_activity_projection.active_batches.iter().any(|summary| {
                summary.tool_batch_id == request.batch_projection.tool_batch_id
                    && summary.status == ToolBatchStatusV1::Running
            }) => {}
        _ => return Err(PersistenceError::ToolEntityIntegrity),
    }
    let session_call = request
        .session_activity_projection
        .recent_calls
        .iter()
        .find(|summary| summary.tool_call_id == request.result.tool_call_id)
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    if session_call.status != ToolCallStatusV1::Finalized
        || session_call.outcome != Some(expected_outcome)
        || session_call.side_effect_state != Some(request.result.side_effect_state)
        || session_call.started_at != request.result.started_at
        || session_call.completed_at != Some(request.result.completed_at)
        || session_call.can_cancel
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn validate_finalization_artifact_drafts(
    request: &ToolExecutionAttemptFinalizationRequest,
) -> Result<(), PersistenceError> {
    let completed_at = request.result.completed_at;
    let tool_call_owner = ArtifactOwnerRef::Tool {
        owner: ToolArtifactOwnerRef::ToolCall { tool_call_id: request.result.tool_call_id },
    };
    validate_terminal_artifact_draft(
        &request.attempt_evidence_artifact,
        request.attempt.evidence_ref.ok_or(PersistenceError::ToolEntityIntegrity)?,
        ArtifactOwnerRef::Tool {
            owner: ToolArtifactOwnerRef::ExecutionAttempt {
                tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
            },
        },
        "application/vnd.aura.tool-attempt-evidence.v1+json",
        ArtifactAccess::NONE,
        completed_at,
    )?;
    validate_terminal_artifact_draft(
        &request.aggregate_side_effect_evidence_artifact,
        request
            .result
            .aggregate_side_effect_evidence_ref
            .ok_or(PersistenceError::ToolEntityIntegrity)?,
        tool_call_owner.clone(),
        "application/vnd.aura.tool-side-effect-aggregate.v1+json",
        ArtifactAccess::NONE,
        completed_at,
    )?;
    validate_terminal_artifact_draft(
        &request.model_visible_result_artifact,
        request.result.model_visible_result_ref.ok_or(PersistenceError::ToolEntityIntegrity)?,
        tool_call_owner.clone(),
        "application/vnd.aura.model-visible-tool-result.v1+json",
        ArtifactAccess::MODEL_READ,
        completed_at,
    )?;
    validate_terminal_artifact_draft(
        &request.user_audit_result_artifact,
        request.result.user_audit_result_ref.ok_or(PersistenceError::ToolEntityIntegrity)?,
        tool_call_owner,
        "application/vnd.aura.user-audit-tool-result.v1+json",
        ArtifactAccess::USER_READ,
        completed_at,
    )?;
    let ids = [
        request.attempt_evidence_artifact.artifact_id,
        request.aggregate_side_effect_evidence_artifact.artifact_id,
        request.model_visible_result_artifact.artifact_id,
        request.user_audit_result_artifact.artifact_id,
    ];
    if ids.iter().enumerate().any(|(index, id)| ids[..index].contains(id)) {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn validate_terminal_artifact_draft(
    draft: &ArtifactWriteDraftV1,
    expected_artifact_id: ArtifactId,
    expected_owner: ArtifactOwnerRef,
    expected_media_type: &str,
    expected_access: ArtifactAccess,
    expected_created_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let record = draft.record()?;
    if record.artifact_id != expected_artifact_id
        || record.owner != expected_owner
        || record.scope != ArtifactScope::ToolExecution
        || record.media_type != expected_media_type
        || record.access != expected_access
        || record.created_at != expected_created_at
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn insert_result_artifact_access_links(
    transaction: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    request: &ToolExecutionAttemptFinalizationRequest,
) -> Result<(), PersistenceError> {
    let model_artifact_id =
        request.result.model_visible_result_ref.ok_or(PersistenceError::ToolEntityIntegrity)?;
    let audit_artifact_id =
        request.result.user_audit_result_ref.ok_or(PersistenceError::ToolEntityIntegrity)?;
    if model_artifact_id == audit_artifact_id {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let mut by_artifact = BTreeMap::new();
    for link in &request.result_artifact_access_links {
        link.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let ArtifactAccessSubjectV1::Session { session_id } = link.subject else {
            return Err(PersistenceError::ToolEntityIntegrity);
        };
        if session_id != batch.invocation_identity.session_id
            || link.created_at != request.result.completed_at
            || by_artifact.insert(link.artifact_id, link).is_some()
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
    }
    let model_link =
        by_artifact.remove(&model_artifact_id).ok_or(PersistenceError::ToolEntityIntegrity)?;
    let audit_link =
        by_artifact.remove(&audit_artifact_id).ok_or(PersistenceError::ToolEntityIntegrity)?;
    if model_link.access != ArtifactAccess::MODEL_READ
        || audit_link.access != ArtifactAccess::USER_READ
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    verify_tool_result_envelope_artifact(
        transaction,
        model_artifact_id,
        request.result.tool_call_id,
        "application/vnd.aura.model-visible-tool-result.v1+json",
        ArtifactAccess::MODEL_READ,
    )?;
    verify_tool_result_envelope_artifact(
        transaction,
        audit_artifact_id,
        request.result.tool_call_id,
        "application/vnd.aura.user-audit-tool-result.v1+json",
        ArtifactAccess::USER_READ,
    )?;
    crate::artifact::insert_artifact_access_link_in_transaction(transaction, model_link)?;
    crate::artifact::insert_artifact_access_link_in_transaction(transaction, audit_link)?;
    if !by_artifact.is_empty() && !matches!(request.result.outcome, ToolResultOutcomeV1::Succeeded)
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let output_access = ArtifactAccess::MODEL_READ.union(ArtifactAccess::USER_READ);
    for link in by_artifact.into_values() {
        if link.access != output_access {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        verify_tool_result_output_artifact(
            transaction,
            link.artifact_id,
            request.attempt.tool_execution_attempt_id,
            output_access,
        )?;
        crate::artifact::insert_artifact_access_link_in_transaction(transaction, link)?;
    }
    Ok(())
}

fn verify_tool_result_output_artifact(
    transaction: &Transaction<'_>,
    artifact_id: ArtifactId,
    tool_execution_attempt_id: aura_domain_common::ToolExecutionAttemptId,
    expected_access: ArtifactAccess,
) -> Result<(), PersistenceError> {
    let row = transaction
        .query_row(
            "SELECT artifact.scope, artifact.media_type, artifact.access_flags,
                    owner.owner_kind, owner.owner_type, owner.owner_id
             FROM artifacts AS artifact
             JOIN artifact_owners AS owner USING (artifact_id)
             WHERE artifact.artifact_id=?1",
            [artifact_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ArtifactNotFound)?;
    let owner = parse_owner(&row.3, &row.4, row.5.as_deref())?;
    if row.0 != "tool_execution"
        || row.1.trim().is_empty()
        || row.2 != i64::from(expected_access.bits())
        || owner
            != (ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ExecutionAttempt { tool_execution_attempt_id },
            })
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn verify_tool_result_envelope_artifact(
    transaction: &Transaction<'_>,
    artifact_id: ArtifactId,
    tool_call_id: ToolCallId,
    expected_media_type: &str,
    expected_access: ArtifactAccess,
) -> Result<(), PersistenceError> {
    let row = transaction
        .query_row(
            "SELECT artifact.scope, artifact.media_type, artifact.access_flags,
                    owner.owner_kind, owner.owner_type, owner.owner_id
             FROM artifacts AS artifact
             JOIN artifact_owners AS owner USING (artifact_id)
             WHERE artifact.artifact_id=?1",
            [artifact_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ArtifactNotFound)?;
    let owner = parse_owner(&row.3, &row.4, row.5.as_deref())?;
    if row.0 != "tool_execution"
        || row.1 != expected_media_type
        || row.2 != i64::from(expected_access.bits())
        || owner
            != (ArtifactOwnerRef::Tool { owner: ToolArtifactOwnerRef::ToolCall { tool_call_id } })
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn validate_terminal_fact_alignment(
    request: &ToolExecutionAttemptFinalizationRequest,
) -> Result<(), PersistenceError> {
    let aligned = match (&request.terminal, request.attempt.status, &request.result.outcome) {
        (
            ToolExecutionAttemptTerminalFact::Succeeded,
            ToolExecutionAttemptStatusV1::Succeeded,
            ToolResultOutcomeV1::Succeeded,
        ) => true,
        (
            ToolExecutionAttemptTerminalFact::Failed { error: left },
            ToolExecutionAttemptStatusV1::Failed,
            ToolResultOutcomeV1::Failed { error: right },
        ) => left == right,
        (
            ToolExecutionAttemptTerminalFact::Cancelled { reason: left },
            ToolExecutionAttemptStatusV1::Cancelled,
            ToolResultOutcomeV1::Cancelled { reason: right },
        ) => left == right,
        (
            ToolExecutionAttemptTerminalFact::Indeterminate {
                reason: left_reason,
                error: left_error,
            },
            ToolExecutionAttemptStatusV1::Indeterminate,
            ToolResultOutcomeV1::Indeterminate { reason: right_reason, error: right_error },
        ) => left_reason == right_reason && left_error == right_error,
        _ => false,
    };
    if !aligned {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let current_attempt =
        request.result.attempts.last().ok_or(PersistenceError::ToolEntityIntegrity)?;
    if current_attempt.tool_execution_attempt_id != request.attempt.tool_execution_attempt_id
        || current_attempt.attempt_number != request.attempt.attempt_number
        || current_attempt.side_effect_state != request.attempt.side_effect_state
        || current_attempt.side_effect_evidence_ref != request.attempt.evidence_ref
        || !current_attempt.dispatched
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

/// The reason class of a stored outcome, from its error category.
///
/// Both activity surfaces — the live page and the recovery projection — name the same
/// reason, and they name it from the category the Store recorded rather than from the
/// message text. A `not_found` that read as a permission refusal on one surface and not
/// the other is exactly what the four Shell cleanup checks hit.
pub(crate) fn error_class(outcome: &ToolResultOutcomeV1) -> Option<ToolCallErrorClassV1> {
    let error = match outcome {
        ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::OperationRejected { error }
        | ToolResultOutcomeV1::Indeterminate { error, .. } => Some(error),
        ToolResultOutcomeV1::NotExecuted { error, .. } => {
            return Some(
                error
                    .as_ref()
                    .map_or(ToolCallErrorClassV1::InvalidRequest, |error| classify(error.category)),
            );
        }
        ToolResultOutcomeV1::Succeeded | ToolResultOutcomeV1::Cancelled { .. } => None,
    };
    error.map(|error| classify(error.category))
}

fn classify(category: aura_domain_common::ErrorCategory) -> ToolCallErrorClassV1 {
    use aura_domain_common::ErrorCategory as Category;
    match category {
        Category::NotFound => ToolCallErrorClassV1::NotFound,
        Category::Permission => ToolCallErrorClassV1::Permission,
        Category::Conflict => ToolCallErrorClassV1::Conflict,
        Category::Validation => ToolCallErrorClassV1::InvalidRequest,
        Category::Unavailable | Category::Compatibility => ToolCallErrorClassV1::Unavailable,
        Category::Cancelled | Category::Transport | Category::Protocol | Category::Internal => {
            ToolCallErrorClassV1::Other
        }
    }
}

fn outcome_summary(outcome: &ToolResultOutcomeV1) -> ToolResultOutcomeSummaryView {
    match outcome {
        ToolResultOutcomeV1::Succeeded => ToolResultOutcomeSummaryView::Succeeded,
        ToolResultOutcomeV1::OperationRejected { .. } => {
            ToolResultOutcomeSummaryView::OperationRejected
        }
        ToolResultOutcomeV1::Failed { .. } => ToolResultOutcomeSummaryView::Failed,
        ToolResultOutcomeV1::Cancelled { reason } => {
            ToolResultOutcomeSummaryView::Cancelled { reason: *reason }
        }
        ToolResultOutcomeV1::NotExecuted { reason, .. } => {
            ToolResultOutcomeSummaryView::NotExecuted { reason: *reason }
        }
        ToolResultOutcomeV1::Indeterminate { reason, .. } => {
            ToolResultOutcomeSummaryView::Indeterminate { reason: *reason }
        }
    }
}

fn projection_error_matches(
    outcome: &ToolResultOutcomeV1,
    error: Option<&aura_tool_projection::ToolProjectionErrorView>,
) -> bool {
    match (outcome, error) {
        (ToolResultOutcomeV1::Succeeded | ToolResultOutcomeV1::Cancelled { .. }, None) => true,
        (
            ToolResultOutcomeV1::Failed { error: expected }
            | ToolResultOutcomeV1::Indeterminate { error: expected, .. },
            Some(actual),
        ) => {
            actual.code == expected.code
                && actual.category == expected.category
                && actual.message == expected.message
                && actual.retryable == expected.retryable
        }
        _ => false,
    }
}

fn validate_attempt_projection_against_record(
    projection: &ToolExecutionAttemptProjection,
    attempt: &ToolExecutionAttemptRecordV1,
) -> Result<(), PersistenceError> {
    if projection.tool_execution_attempt_id != attempt.tool_execution_attempt_id
        || projection.tool_call_id != attempt.tool_call_id
        || projection.attempt_number != attempt.attempt_number
        || projection.status != attempt.status
        || projection.side_effect_state != attempt.side_effect_state
        || projection.started_at != attempt.started_at
        || projection.dispatch_prepared_at != attempt.dispatch_prepared_at
        || projection.completed_at != attempt.completed_at
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn verify_attempt_start_authority(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptDispatchPreparedCommitRequest,
) -> Result<AttemptStartAuthority, PersistenceError> {
    let batch_row = transaction
        .query_row(
            "SELECT request_json, execution_plan_json, execution_plan_hash_json,
                    status, entity_revision
             FROM tool_batches WHERE tool_batch_id=?1",
            [request.batch_projection.tool_batch_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let batch = decode_json::<ToolBatchRequestV1>(&batch_row.0)?;
    let plan = decode_json::<ToolExecutionPlanV1>(&batch_row.1)?;
    plan.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if batch.tool_batch_id != request.batch_projection.tool_batch_id
        || batch_row.2 != encode_json(&request.execution_plan_hash)?
        || plan.execution_plan_hash != request.execution_plan_hash
        || !matches!(batch_row.3.as_str(), "admitted" | "running")
        || batch_row.4 != sqlite_revision(request.expected_batch_revision)?
        || request.batch_projection.batch_content_hash != batch.batch_content_hash
        || request.batch_projection.session_id != batch.invocation_identity.session_id
        || request.batch_projection.turn_task_id != batch.invocation_identity.turn_task_id
        || request.batch_projection.agent_run_id != batch.invocation_identity.agent_run_id
        || request.batch_projection.model_step_id != batch.invocation_identity.model_step_id
        || request.batch_projection.registry_snapshot_ref != batch.registry_snapshot_ref
        || request.batch_projection.control_state_revision != batch.p08_state_revision
        || request.batch_projection.control_state_hash != batch.p08_state_hash
        || request.batch_projection.calls.len() != batch.calls.len()
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    for (call, summary) in batch.calls.iter().zip(&request.batch_projection.calls) {
        if summary.tool_call_id != call.tool_call_id
            || summary.tool_batch_id != batch.tool_batch_id
            || summary.ordinal != call.ordinal
            || summary.tool_id != call.definition_ref.as_ref().map(|r| r.tool_id)
            || summary.tool_version != call.definition_ref.as_ref().map(|r| r.tool_version.clone())
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
    }
    let plan_call = plan
        .calls
        .iter()
        .find(|call| call.tool_call_id == request.attempt.tool_call_id)
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    let normalized_call = batch
        .calls
        .iter()
        .find(|call| call.tool_call_id == request.attempt.tool_call_id)
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    if plan_call.ordinal != normalized_call.ordinal
        || request.call_projection.ordinal != normalized_call.ordinal
        || request.call_projection.definition_ref != normalized_call.definition_ref
        || request.call_projection.tool_batch_id != batch.tool_batch_id
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    match (
        &normalized_call.input,
        &request.call_projection.purpose,
        &request.call_projection.invalid_input_issue,
    ) {
        (NormalizedToolCallInputStateV1::Ready { purpose, .. }, Some(actual), None)
            if actual == purpose.as_str() => {}
        (NormalizedToolCallInputStateV1::InvalidInput { issue, .. }, None, Some(actual))
            if actual == issue => {}
        _ => return Err(PersistenceError::ToolEntityIntegrity),
    }

    let call_row = transaction
        .query_row(
            "SELECT normalized_call_json, normalized_call_hash_json,
                    execution_plan_call_json, status, entity_revision, started_at
             FROM tool_calls WHERE tool_call_id=?1 AND tool_batch_id=?2",
            params![request.attempt.tool_call_id.to_string(), batch.tool_batch_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let persisted_call = decode_verified_record::<aura_tool_contract::NormalizedToolCallV1>(
        &call_row.0,
        &call_row.1,
    )?;
    let persisted_plan_call = call_row
        .2
        .as_deref()
        .map(decode_json::<ToolExecutionPlanCallV1>)
        .transpose()?
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    let expected_call_status =
        if request.attempt.attempt_number == 1 { "queued" } else { "running" };
    if persisted_call != *normalized_call
        || persisted_plan_call != *plan_call
        || call_row.3 != expected_call_status
        || call_row.4 != sqlite_revision(request.expected_call_revision)?
        || transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tool_results WHERE tool_call_id=?1)",
            [request.attempt.tool_call_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    validate_attempt_sequence(transaction, request)?;
    let prior_attempt_ids = transaction
        .prepare(
            "SELECT tool_execution_attempt_id FROM tool_execution_attempts
             WHERE tool_call_id=?1 ORDER BY attempt_number",
        )?
        .query_map([request.attempt.tool_call_id.to_string()], |row| row.get::<_, String>(0))?
        .map(|row| {
            row.and_then(|value| {
                aura_domain_common::ToolExecutionAttemptId::from_str(&value).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut expected_attempt_ids = prior_attempt_ids;
    expected_attempt_ids.push(request.attempt.tool_execution_attempt_id);
    let existing_start = call_row.5.as_deref().map(parse_timestamp).transpose()?;
    let expected_started_at = existing_start.unwrap_or(request.attempt.started_at);
    if request.call_projection.attempt_ids != expected_attempt_ids
        || request.call_projection.started_at != Some(expected_started_at)
        || request.batch_projection.started_at
            != Some(
                read_batch_started_at(transaction, batch.tool_batch_id)?
                    .unwrap_or(request.attempt.started_at),
            )
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    verify_admission_causation(transaction, request.admitted_event_id, batch.tool_batch_id)?;
    let workspace_id = workspace_for_session(transaction, batch.invocation_identity.session_id)?;
    Ok(AttemptStartAuthority { workspace_id, batch })
}

fn validate_attempt_sequence(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptDispatchPreparedCommitRequest,
) -> Result<(), PersistenceError> {
    let (attempt_count, nonterminal_count) = transaction.query_row(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN status NOT IN
            ('succeeded', 'failed', 'cancelled', 'indeterminate') THEN 1 ELSE 0 END), 0)
         FROM tool_execution_attempts WHERE tool_call_id=?1",
        [request.attempt.tool_call_id.to_string()],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    if nonterminal_count != 0
        || attempt_count + 1 != i64::from(request.attempt.attempt_number)
        || transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tool_execution_attempts
             WHERE tool_execution_attempt_id=?1)",
            [request.attempt.tool_execution_attempt_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(())
}

fn verify_admission_causation(
    transaction: &Transaction<'_>,
    event_id: EventId,
    tool_batch_id: aura_domain_common::ToolBatchId,
) -> Result<(), PersistenceError> {
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM events
             WHERE event_id=?1 AND event_type='ToolBatchAdmitted'",
            [event_id.to_string()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    let payload = decode_json::<ToolBatchAdmittedPayloadV1>(&payload)?;
    if payload.tool_batch_id != tool_batch_id {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn read_batch_started_at(
    transaction: &Transaction<'_>,
    tool_batch_id: aura_domain_common::ToolBatchId,
) -> Result<Option<UtcTimestamp>, PersistenceError> {
    transaction
        .query_row(
            "SELECT started_at FROM tool_batches WHERE tool_batch_id=?1",
            [tool_batch_id.to_string()],
            |row| row.get::<_, Option<String>>(0),
        )?
        .as_deref()
        .map(parse_timestamp)
        .transpose()
}

fn workspace_for_session(
    transaction: &Transaction<'_>,
    session_id: aura_domain_common::SessionId,
) -> Result<WorkspaceId, PersistenceError> {
    transaction
        .query_row(
            "SELECT workspace_id FROM sessions WHERE session_id=?1",
            [session_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)
        .and_then(|value| WorkspaceId::from_str(&value).map_err(PersistenceError::from))
}

fn parse_timestamp(value: &str) -> Result<UtcTimestamp, PersistenceError> {
    UtcTimestamp::from_str(value).map_err(|_| PersistenceError::ToolEntityIntegrity)
}

fn insert_dispatch_prepared_attempt(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptDispatchPreparedCommitRequest,
    tool_batch_id: aura_domain_common::ToolBatchId,
    started_event_id: EventId,
    dispatch_prepared_event_id: EventId,
) -> Result<(), PersistenceError> {
    let (record_json, record_hash_json) = encode_record(&request.attempt)?;
    transaction.execute(
        "INSERT INTO tool_execution_attempts(
            tool_execution_attempt_id, tool_call_id, tool_batch_id, attempt_number,
            execution_plan_hash_json, status, side_effect_state, record_json,
            record_hash_json, evidence_artifact_id, started_event_id,
            dispatch_prepared_event_id, cancellation_requested_event_id,
            terminal_event_id, started_at, dispatch_prepared_at,
            cancellation_requested_at, completed_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'dispatch_prepared', 'not_started', ?6, ?7,
                   NULL, ?8, ?9, NULL, NULL, ?10, ?11, NULL, NULL, ?11)",
        params![
            request.attempt.tool_execution_attempt_id.to_string(),
            request.attempt.tool_call_id.to_string(),
            tool_batch_id.to_string(),
            i64::from(request.attempt.attempt_number),
            encode_json(&request.execution_plan_hash)?,
            record_json,
            record_hash_json,
            started_event_id.to_string(),
            dispatch_prepared_event_id.to_string(),
            request.attempt.started_at.to_string(),
            request
                .attempt
                .dispatch_prepared_at
                .ok_or(PersistenceError::ToolEntityIntegrity)?
                .to_string(),
        ],
    )?;
    transaction.execute(
        "INSERT INTO tool_attempt_operational_state(
            tool_execution_attempt_id, sequence, percentage, progress_json, updated_at
         ) VALUES (?1, 0, NULL, NULL, ?2)",
        params![
            request.attempt.tool_execution_attempt_id.to_string(),
            request.attempt.started_at.to_string(),
        ],
    )?;
    Ok(())
}

fn insert_tool_sandbox_dispatch_binding(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptDispatchPreparedCommitRequest,
    authority: &AttemptStartAuthority,
    binding: &ToolSandboxDispatchBindingV1,
) -> Result<(), PersistenceError> {
    if binding.tool_batch_id != authority.batch.tool_batch_id
        || binding.tool_call_id != request.attempt.tool_call_id
        || binding.tool_execution_attempt_id != request.attempt.tool_execution_attempt_id
        || binding.execution_plan_hash != request.execution_plan_hash
        || binding.permission_snapshot.session_id != authority.batch.invocation_identity.session_id
        || binding.operation_identity.session_id
            != Some(authority.batch.invocation_identity.session_id)
        || binding.operation_identity.turn_task_id
            != Some(authority.batch.invocation_identity.turn_task_id)
        || binding.operation_identity.agent_run_id
            != Some(authority.batch.invocation_identity.agent_run_id)
        || binding.source.definition_ref.registry_snapshot_ref
            != authority.batch.registry_snapshot_ref
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let normalized = authority
        .batch
        .calls
        .iter()
        .find(|call| call.tool_call_id == binding.tool_call_id)
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    if normalized.definition_ref.as_ref() != Some(&binding.source.definition_ref) {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let registry_json = transaction
        .query_row(
            "SELECT snapshot_json FROM tool_registry_snapshots
             WHERE tool_registry_snapshot_id=?1",
            [binding
                .source
                .definition_ref
                .registry_snapshot_ref
                .tool_registry_snapshot_id
                .to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let registry: ToolRegistrySnapshotV1 = decode_json(&registry_json)?;
    registry.validate().map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
    if registry.snapshot_ref != binding.source.definition_ref.registry_snapshot_ref {
        return Err(PersistenceError::ToolRegistryIntegrity);
    }
    let mut matching_bindings = registry.effective_bindings.iter().filter(|candidate| {
        candidate.tool_id == binding.source.definition_ref.tool_id
            && candidate.tool_version == binding.source.definition_ref.tool_version
    });
    let Some(effective) = matching_bindings.next() else {
        return Err(PersistenceError::ToolRegistryIntegrity);
    };
    if matching_bindings.next().is_some()
        || effective.tool_instance_id != binding.source.tool_instance_id
        || effective.source_kind != binding.source.source_kind
        || effective.definition_fingerprint != binding.source.definition_fingerprint
        || effective.source_contract_fingerprint != binding.source.source_contract_fingerprint
    {
        return Err(PersistenceError::ToolRegistryIntegrity);
    }
    if binding.permission_snapshot.workspace_id != authority.workspace_id {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    verify_permission_snapshot_and_dispatch_authorization(
        transaction,
        binding,
        binding.prepared_at,
    )?;
    transaction.execute(
        "INSERT INTO tool_sandbox_dispatch_bindings(
            sandbox_execution_id, security_operation_id, tool_execution_attempt_id,
            tool_batch_id, tool_call_id, permission_snapshot_id,
            execution_plan_hash_json, runtime_binding_hash_json,
            binding_json, binding_hash_json, state, prepared_at, consumed_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                   'dispatch_prepared', ?11, NULL, ?11)",
        params![
            binding.sandbox_execution_id.to_string(),
            binding.operation_id.to_string(),
            binding.tool_execution_attempt_id.to_string(),
            binding.tool_batch_id.to_string(),
            binding.tool_call_id.to_string(),
            binding.permission_snapshot.id.to_string(),
            encode_json(&binding.execution_plan_hash)?,
            encode_json(&binding.runtime_binding_hash)?,
            encode_json(binding)?,
            encode_json(&binding.binding_hash)?,
            binding.prepared_at.to_string(),
        ],
    )?;
    Ok(())
}

fn insert_builtin_core_handler_dispatch_binding(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptDispatchPreparedCommitRequest,
    authority: &AttemptStartAuthority,
    binding: &BuiltinCoreHandlerDispatchBindingV1,
) -> Result<(), PersistenceError> {
    binding.validate().map_err(|_| PersistenceError::BuiltinCoreHandlerDispatchBindingMismatch)?;
    if binding.tool_batch_id != authority.batch.tool_batch_id
        || binding.tool_call_id != request.attempt.tool_call_id
        || binding.tool_execution_attempt_id != request.attempt.tool_execution_attempt_id
        || binding.execution_plan_hash != request.execution_plan_hash
        || binding.binding_hash != request.dispatch_evidence_hash
        || binding.permission_snapshot.session_id != authority.batch.invocation_identity.session_id
        || binding.operation_identity.session_id
            != Some(authority.batch.invocation_identity.session_id)
        || binding.operation_identity.turn_task_id
            != Some(authority.batch.invocation_identity.turn_task_id)
        || binding.operation_identity.agent_run_id
            != Some(authority.batch.invocation_identity.agent_run_id)
        || binding.source.definition_ref.registry_snapshot_ref
            != authority.batch.registry_snapshot_ref
        || binding.permission_snapshot.workspace_id != authority.workspace_id
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let normalized = authority
        .batch
        .calls
        .iter()
        .find(|call| call.tool_call_id == binding.tool_call_id)
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    if normalized.definition_ref.as_ref() != Some(&binding.source.definition_ref) {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    verify_tool_source_in_exact_registry(transaction, &binding.source)?;
    verify_builtin_permission_snapshot_and_dispatch_authorization(
        transaction,
        binding,
        binding.prepared_at,
    )?;
    transaction.execute(
        "INSERT INTO tool_builtin_core_handler_dispatch_bindings(
            tool_execution_attempt_id, security_operation_id, binding_json,
            binding_hash_json, state, prepared_at, consumed_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, 'dispatch_prepared', ?5, NULL, ?5)",
        params![
            binding.tool_execution_attempt_id.to_string(),
            binding.operation_id.to_string(),
            encode_json(binding)?,
            encode_json(&binding.binding_hash)?,
            binding.prepared_at.to_string(),
        ],
    )?;
    Ok(())
}

fn verify_tool_source_in_exact_registry(
    transaction: &Transaction<'_>,
    source: &aura_permission_domain::ToolSecuritySourceRefV1,
) -> Result<(), PersistenceError> {
    let registry_json = transaction
        .query_row(
            "SELECT snapshot_json FROM tool_registry_snapshots
             WHERE tool_registry_snapshot_id=?1",
            [source.definition_ref.registry_snapshot_ref.tool_registry_snapshot_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let registry: ToolRegistrySnapshotV1 = decode_json(&registry_json)?;
    registry.validate().map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
    if registry.snapshot_ref != source.definition_ref.registry_snapshot_ref {
        return Err(PersistenceError::ToolRegistryIntegrity);
    }
    let mut matching_bindings = registry.effective_bindings.iter().filter(|candidate| {
        candidate.tool_id == source.definition_ref.tool_id
            && candidate.tool_version == source.definition_ref.tool_version
    });
    let Some(effective) = matching_bindings.next() else {
        return Err(PersistenceError::ToolRegistryIntegrity);
    };
    if matching_bindings.next().is_some()
        || effective.tool_instance_id != source.tool_instance_id
        || effective.source_kind != source.source_kind
        || effective.definition_fingerprint != source.definition_fingerprint
        || effective.source_contract_fingerprint != source.source_contract_fingerprint
    {
        return Err(PersistenceError::ToolRegistryIntegrity);
    }
    Ok(())
}

fn verify_builtin_permission_snapshot_and_dispatch_authorization(
    transaction: &Transaction<'_>,
    binding: &BuiltinCoreHandlerDispatchBindingV1,
    checked_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let persisted_snapshot = transaction
        .query_row(
            "SELECT session_id, workspace_id, mode, session_permission_revision,
                    active_grant_ids_json, created_at
             FROM permission_snapshots WHERE permission_snapshot_id=?1",
            [binding.permission_snapshot.id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::PermissionSnapshotNotFound)?;
    let active_grant_ids: Vec<PermissionGrantId> = decode_json(&persisted_snapshot.4)?;
    if persisted_snapshot.0 != binding.permission_snapshot.session_id.to_string()
        || persisted_snapshot.1 != binding.permission_snapshot.workspace_id.to_string()
        || persisted_snapshot.2 != permission_mode_wire(binding.permission_snapshot.mode)
        || persisted_snapshot.3
            != i64::try_from(binding.permission_snapshot.session_permission_revision)
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?
        || active_grant_ids != binding.permission_snapshot.active_grant_ids
        || persisted_snapshot.5 != binding.permission_snapshot.created_at.to_string()
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let current_permission = transaction
        .query_row(
            "SELECT workspace_id, mode, revision
             FROM session_permission_states WHERE session_id=?1",
            [binding.permission_snapshot.session_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)),
        )
        .optional()?
        .ok_or(PersistenceError::BuiltinCoreHandlerDispatchBindingUnavailable)?;
    if current_permission.0 != binding.permission_snapshot.workspace_id.to_string()
        || current_permission.1 != permission_mode_wire(binding.permission_snapshot.mode)
        || current_permission.2 != persisted_snapshot.3
    {
        return Err(PersistenceError::BuiltinCoreHandlerDispatchBindingUnavailable);
    }
    match &binding.authorization_basis {
        aura_security_contract::PermissionAllowAuthorizationBasisV1::PermissionMode { mode } => {
            if *mode != binding.permission_snapshot.mode {
                return Err(PersistenceError::BuiltinCoreHandlerDispatchBindingUnavailable);
            }
        }
        aura_security_contract::PermissionAllowAuthorizationBasisV1::PermissionGrant {
            permission_grant_id,
            lifetime,
        } => {
            let grant_json = transaction
                .query_row(
                    "SELECT grant_json FROM permission_grants WHERE permission_grant_id=?1",
                    [permission_grant_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or(PersistenceError::BuiltinCoreHandlerDispatchBindingUnavailable)?;
            let grant: PermissionGrant = decode_json(&grant_json)?;
            let expected_source = SecuritySource::Tool { tool: Box::new(binding.source.clone()) };
            let operation_fingerprint = binding.operation_fingerprint.clone();
            let lifetime_identity_matches = match grant.lifetime {
                PermissionGrantLifetime::Once => {
                    grant.session_id == binding.operation_identity.session_id
                        && grant.operation_fingerprint.as_ref() == Some(&operation_fingerprint)
                }
                PermissionGrantLifetime::TurnTask => {
                    grant.session_id == binding.operation_identity.session_id
                        && grant.turn_task_id == binding.operation_identity.turn_task_id
                }
                PermissionGrantLifetime::Session => {
                    grant.session_id == binding.operation_identity.session_id
                }
                PermissionGrantLifetime::Workspace => true,
            };
            if grant.id != *permission_grant_id
                || grant.lifetime != *lifetime
                || !binding.permission_snapshot.active_grant_ids.contains(permission_grant_id)
                || grant.workspace_id != binding.permission_snapshot.workspace_id
                || grant.source != expected_source
                || grant.source_fingerprint.as_ref() != Some(&binding.source_security_fingerprint)
                || grant.revoked_at.is_some()
                || grant.consumed_at.is_some()
                || grant.expires_at.is_some_and(|expires_at| expires_at <= checked_at)
                || !lifetime_identity_matches
            {
                return Err(PersistenceError::BuiltinCoreHandlerDispatchBindingUnavailable);
            }
        }
    }
    Ok(())
}

fn verify_permission_snapshot_and_dispatch_authorization(
    transaction: &Transaction<'_>,
    binding: &ToolSandboxDispatchBindingV1,
    checked_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let persisted_snapshot = transaction
        .query_row(
            "SELECT session_id, workspace_id, mode, session_permission_revision,
                    active_grant_ids_json, created_at
             FROM permission_snapshots WHERE permission_snapshot_id=?1",
            [binding.permission_snapshot.id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::PermissionSnapshotNotFound)?;
    let active_grant_ids: Vec<PermissionGrantId> = decode_json(&persisted_snapshot.4)?;
    if persisted_snapshot.0 != binding.permission_snapshot.session_id.to_string()
        || persisted_snapshot.1 != binding.permission_snapshot.workspace_id.to_string()
        || persisted_snapshot.2 != permission_mode_wire(binding.permission_snapshot.mode)
        || persisted_snapshot.3
            != i64::try_from(binding.permission_snapshot.session_permission_revision)
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?
        || active_grant_ids != binding.permission_snapshot.active_grant_ids
        || persisted_snapshot.5 != binding.permission_snapshot.created_at.to_string()
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }

    // The final pre-spawn guard rejects a snapshot that has been superseded.
    // This makes permission-mode changes and revocation effective for a queued
    // operation without rewriting the frozen operation or execution plan.
    let current_permission = transaction
        .query_row(
            "SELECT workspace_id, mode, revision
             FROM session_permission_states WHERE session_id=?1",
            [binding.permission_snapshot.session_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)),
        )
        .optional()?
        .ok_or(PersistenceError::ToolSandboxDispatchBindingUnavailable)?;
    if current_permission.0 != binding.permission_snapshot.workspace_id.to_string()
        || current_permission.1 != permission_mode_wire(binding.permission_snapshot.mode)
        || current_permission.2 != persisted_snapshot.3
    {
        return Err(PersistenceError::ToolSandboxDispatchBindingUnavailable);
    }

    match &binding.authorization_basis {
        aura_security_contract::PermissionAllowAuthorizationBasisV1::PermissionMode { mode } => {
            if *mode != binding.permission_snapshot.mode {
                return Err(PersistenceError::ToolSandboxDispatchBindingUnavailable);
            }
        }
        aura_security_contract::PermissionAllowAuthorizationBasisV1::PermissionGrant {
            permission_grant_id,
            lifetime,
        } => {
            let grant_json = transaction
                .query_row(
                    "SELECT grant_json FROM permission_grants WHERE permission_grant_id=?1",
                    [permission_grant_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or(PersistenceError::ToolSandboxDispatchBindingUnavailable)?;
            let grant: PermissionGrant = decode_json(&grant_json)?;
            let expected_source = SecuritySource::Tool { tool: Box::new(binding.source.clone()) };
            let operation_fingerprint = binding.operation_fingerprint.as_content_hash();
            let lifetime_identity_matches = match grant.lifetime {
                PermissionGrantLifetime::Once => {
                    grant.session_id == binding.operation_identity.session_id
                        && grant.operation_fingerprint.as_ref() == Some(&operation_fingerprint)
                }
                PermissionGrantLifetime::TurnTask => {
                    grant.session_id == binding.operation_identity.session_id
                        && grant.turn_task_id == binding.operation_identity.turn_task_id
                }
                PermissionGrantLifetime::Session => {
                    grant.session_id == binding.operation_identity.session_id
                }
                PermissionGrantLifetime::Workspace => true,
            };
            if grant.id != *permission_grant_id
                || grant.lifetime != *lifetime
                || !binding.permission_snapshot.active_grant_ids.contains(permission_grant_id)
                || grant.workspace_id != binding.permission_snapshot.workspace_id
                || grant.source != expected_source
                || grant.source_fingerprint.as_ref() != Some(&binding.source_security_fingerprint)
                || grant.revoked_at.is_some()
                || grant.consumed_at.is_some()
                || grant.expires_at.is_some_and(|expires_at| expires_at <= checked_at)
                || !lifetime_identity_matches
            {
                return Err(PersistenceError::ToolSandboxDispatchBindingUnavailable);
            }
        }
    }
    Ok(())
}

const fn permission_mode_wire(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::ReadOnly => "read_only",
        PermissionMode::WorkspaceReadWrite => "workspace_read_write",
        PermissionMode::FullAccess => "full_access",
    }
}

fn transition_call_and_batch_to_running(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptDispatchPreparedCommitRequest,
) -> Result<(), PersistenceError> {
    let call_changed = transaction.execute(
        "UPDATE tool_calls
         SET status='running', entity_revision=entity_revision+1,
             started_at=COALESCE(started_at, ?1), updated_at=?1
         WHERE tool_call_id=?2 AND status=?3 AND entity_revision=?4",
        params![
            request.attempt.started_at.to_string(),
            request.attempt.tool_call_id.to_string(),
            if request.attempt.attempt_number == 1 { "queued" } else { "running" },
            sqlite_revision(request.expected_call_revision)?,
        ],
    )?;
    let batch_changed = transaction.execute(
        "UPDATE tool_batches
         SET status='running', entity_revision=entity_revision+1,
             started_at=COALESCE(started_at, ?1), updated_at=?1
         WHERE tool_batch_id=?2 AND status IN ('admitted', 'running')
           AND entity_revision=?3",
        params![
            request.attempt.started_at.to_string(),
            request.batch_projection.tool_batch_id.to_string(),
            sqlite_revision(request.expected_batch_revision)?,
        ],
    )?;
    if call_changed != 1 || batch_changed != 1 {
        return Err(PersistenceError::ToolEntityRevisionConflict);
    }
    Ok(())
}

fn transition_attempt_to_running(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptRuntimeStartedRequest,
) -> Result<(), PersistenceError> {
    let row = transaction
        .query_row(
            "SELECT record_json, record_hash_json, status,
                    cancellation_requested_event_id, terminal_event_id
             FROM tool_execution_attempts WHERE tool_execution_attempt_id=?1",
            [request.attempt.tool_execution_attempt_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let previous = decode_verified_record::<ToolExecutionAttemptRecordV1>(&row.0, &row.1)?;
    if row.2 != "dispatch_prepared"
        || row.3.is_some()
        || row.4.is_some()
        || previous.status != ToolExecutionAttemptStatusV1::DispatchPrepared
        || !same_attempt_identity_and_timeline(&previous, &request.attempt)
        || previous.last_progress_sequence != 0
        || request.attempt.last_progress_sequence != 0
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    let (record_json, record_hash_json) = encode_record(&request.attempt)?;
    let changed = transaction.execute(
        "UPDATE tool_execution_attempts
         SET status='running', side_effect_state=?1,
             record_json=?2, record_hash_json=?3, updated_at=?4
         WHERE tool_execution_attempt_id=?5 AND status='dispatch_prepared'
           AND cancellation_requested_event_id IS NULL AND terminal_event_id IS NULL",
        params![
            side_effect_state_name(request.attempt.side_effect_state),
            record_json,
            record_hash_json,
            request.updated_at.to_string(),
            request.attempt.tool_execution_attempt_id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(())
}

fn update_attempt_progress(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptProgressRequest,
) -> Result<(), PersistenceError> {
    let projection = &request.attempt_projection;
    let progress = projection.progress.as_ref().ok_or(PersistenceError::ToolEntityIntegrity)?;
    let row = transaction
        .query_row(
            "SELECT record_json, record_hash_json, status, sequence
             FROM tool_execution_attempts AS attempt
             JOIN tool_attempt_operational_state AS operational
               USING (tool_execution_attempt_id)
             WHERE attempt.tool_execution_attempt_id=?1",
            [projection.tool_execution_attempt_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let attempt = decode_verified_record::<ToolExecutionAttemptRecordV1>(&row.0, &row.1)?;
    if attempt.status.is_terminal()
        || row.2 != attempt_status_name(projection.status)
        || i64::try_from(progress.sequence).map_err(|_| PersistenceError::ToolEntityIntegrity)?
            <= row.3
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    validate_attempt_projection_against_record(projection, &attempt)?;
    let changed = transaction.execute(
        "UPDATE tool_attempt_operational_state
         SET sequence=?1, percentage=?2, progress_json=?3, updated_at=?4
         WHERE tool_execution_attempt_id=?5 AND sequence < ?1",
        params![
            i64::try_from(progress.sequence).map_err(|_| PersistenceError::ToolEntityIntegrity)?,
            progress.percentage.map(i64::from),
            encode_json(progress)?,
            progress.updated_at.to_string(),
            projection.tool_execution_attempt_id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(())
}

fn verify_cancellation_authority(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptCancellationRequest,
) -> Result<CancellationAuthority, PersistenceError> {
    let row = transaction
        .query_row(
            "SELECT attempt.record_json, attempt.record_hash_json,
                    attempt.tool_batch_id, attempt.status,
                    attempt.cancellation_requested_event_id, attempt.terminal_event_id,
                    operational.progress_json
             FROM tool_execution_attempts AS attempt
             JOIN tool_attempt_operational_state AS operational
               USING (tool_execution_attempt_id)
             WHERE attempt.tool_execution_attempt_id=?1",
            [request.attempt.tool_execution_attempt_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let previous = decode_verified_record::<ToolExecutionAttemptRecordV1>(&row.0, &row.1)?;
    if !matches!(
        previous.status,
        ToolExecutionAttemptStatusV1::Running | ToolExecutionAttemptStatusV1::WaitingExternal
    ) || row.3 != attempt_status_name(previous.status)
        || row.4.is_some()
        || row.5.is_some()
        || !same_attempt_identity_and_timeline(&previous, &request.attempt)
        || previous.side_effect_state != request.attempt.side_effect_state
        || previous.last_progress_sequence != request.attempt.last_progress_sequence
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    let expected_progress = row
        .6
        .as_deref()
        .map(decode_json::<aura_tool_projection::ToolAttemptProgressView>)
        .transpose()?;
    if request.attempt_projection.progress != expected_progress {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    let batch_id = aura_domain_common::ToolBatchId::from_str(&row.2)?;
    let batch_json = transaction
        .query_row(
            "SELECT request_json FROM tool_batches
             WHERE tool_batch_id=?1 AND status IN ('running', 'cancelling')",
            [batch_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityStateConflict)?;
    let batch = decode_json::<ToolBatchRequestV1>(&batch_json)?;
    if batch.tool_batch_id != batch_id
        || !batch.calls.iter().any(|call| call.tool_call_id == request.attempt.tool_call_id)
        || !transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM tool_calls
                WHERE tool_call_id=?1 AND tool_batch_id=?2 AND status='running'
            )",
            params![request.attempt.tool_call_id.to_string(), batch_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    let workspace_id = workspace_for_session(transaction, batch.invocation_identity.session_id)?;
    Ok(CancellationAuthority { workspace_id, batch })
}

fn transition_attempt_to_cancelling(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptCancellationRequest,
    cancellation_requested_event_id: EventId,
) -> Result<(), PersistenceError> {
    let (record_json, record_hash_json) = encode_record(&request.attempt)?;
    let changed = transaction.execute(
        "UPDATE tool_execution_attempts
         SET status='cancelling', record_json=?1, record_hash_json=?2,
             cancellation_requested_event_id=?3, cancellation_requested_at=?4,
             updated_at=?4
         WHERE tool_execution_attempt_id=?5
           AND status IN ('running', 'waiting_external')
           AND cancellation_requested_event_id IS NULL AND terminal_event_id IS NULL",
        params![
            record_json,
            record_hash_json,
            cancellation_requested_event_id.to_string(),
            request.requested_at.to_string(),
            request.attempt.tool_execution_attempt_id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(())
}

fn verify_retryable_failure_authority(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptRetryableFailureRequest,
) -> Result<RetryableFailureAuthority, PersistenceError> {
    let row = transaction
        .query_row(
            "SELECT attempt.record_json, attempt.record_hash_json,
                    attempt.tool_batch_id, attempt.status,
                    attempt.cancellation_requested_event_id, attempt.terminal_event_id,
                    operational.sequence, operational.progress_json
             FROM tool_execution_attempts AS attempt
             JOIN tool_attempt_operational_state AS operational
               USING (tool_execution_attempt_id)
             WHERE attempt.tool_execution_attempt_id=?1",
            [request.attempt.tool_execution_attempt_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let previous = decode_verified_record::<ToolExecutionAttemptRecordV1>(&row.0, &row.1)?;
    if !matches!(
        previous.status,
        ToolExecutionAttemptStatusV1::Running
            | ToolExecutionAttemptStatusV1::WaitingExternal
            | ToolExecutionAttemptStatusV1::Cancelling
    ) || row.3 != attempt_status_name(previous.status)
        || row.5.is_some()
        || !same_attempt_identity_and_timeline(&previous, &request.attempt)
        || i64::try_from(request.attempt.last_progress_sequence)
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?
            != row.6
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    let progress = row
        .7
        .as_deref()
        .map(decode_json::<aura_tool_projection::ToolAttemptProgressView>)
        .transpose()?;
    if request.attempt_projection.progress != progress {
        return Err(PersistenceError::ToolEntityStateConflict);
    }

    let batch_id = aura_domain_common::ToolBatchId::from_str(&row.2)?;
    let batch_row = transaction
        .query_row(
            "SELECT request_json, execution_plan_json FROM tool_batches
             WHERE tool_batch_id=?1 AND status='running' AND result_json IS NULL",
            [batch_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityStateConflict)?;
    let batch = decode_json::<ToolBatchRequestV1>(&batch_row.0)?;
    let plan = decode_json::<ToolExecutionPlanV1>(&batch_row.1)?;
    plan.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if batch.tool_batch_id != batch_id || plan.tool_batch_id != batch_id {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let call = batch
        .calls
        .iter()
        .find(|call| call.tool_call_id == request.attempt.tool_call_id)
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    let plan_call = plan
        .calls
        .iter()
        .find(|call| call.tool_call_id == request.attempt.tool_call_id)
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    let active_call = transaction.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM tool_calls
            WHERE tool_call_id=?1 AND tool_batch_id=?2 AND status='running'
              AND completed_at IS NULL
        )",
        params![request.attempt.tool_call_id.to_string(), batch_id.to_string()],
        |row| row.get::<_, bool>(0),
    )?;
    let result_exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM tool_results WHERE tool_call_id=?1)",
        [request.attempt.tool_call_id.to_string()],
        |row| row.get::<_, bool>(0),
    )?;
    if !active_call || result_exists || plan_call.ordinal != call.ordinal {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    let definition = transaction
        .query_row(
            "SELECT record_json, record_hash_json FROM tool_definitions
             WHERE tool_id=?1 AND tool_version_json=?2",
            params![
                resolved_tool(call)?.tool_id.to_string(),
                encode_json(&resolved_tool(call)?.tool_version)?,
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .map(|(record, hash)| decode_verified_record::<ToolDefinitionRecordV1>(&record, &hash))
        .transpose()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    if definition.tool_id != resolved_tool(call)?.tool_id
        || definition.tool_version != resolved_tool(call)?.tool_version
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let decision = assess_failed_attempt_retry(ToolRetryAssessmentV1 {
        error: &request.error,
        // This persistence entry point is private to the Core completion
        // service. The Core has already persisted the closed Gateway retry
        // source in the exact attempt evidence Artifact.
        trusted_source_retryable: true,
        execution_semantics: &definition.execution_semantics,
        side_effect_state: request.attempt.side_effect_state,
        attempt_number: request.attempt.attempt_number,
        max_retries: plan_call.max_retries,
        cancellation_requested: row.4.is_some(),
    })
    .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if decision
        != (ToolRetryDecisionV1::Retry { next_attempt_number: request.attempt.attempt_number + 1 })
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    verify_internal_tool_artifact(
        transaction,
        request.attempt.evidence_ref.ok_or(PersistenceError::ToolEntityIntegrity)?,
        ToolArtifactOwnerRef::ExecutionAttempt {
            tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
        },
    )?;
    let workspace_id = workspace_for_session(transaction, batch.invocation_identity.session_id)?;
    Ok(RetryableFailureAuthority { workspace_id, batch })
}

fn verify_attempt_finalization_authority(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptFinalizationRequest,
) -> Result<AttemptFinalizationAuthority, PersistenceError> {
    let row = transaction
        .query_row(
            "SELECT attempt.record_json, attempt.record_hash_json,
                    attempt.tool_batch_id, attempt.status,
                    attempt.cancellation_requested_event_id, attempt.terminal_event_id,
                    operational.sequence, operational.progress_json
             FROM tool_execution_attempts AS attempt
             JOIN tool_attempt_operational_state AS operational
               USING (tool_execution_attempt_id)
             WHERE attempt.tool_execution_attempt_id=?1",
            [request.attempt.tool_execution_attempt_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let previous = decode_verified_record::<ToolExecutionAttemptRecordV1>(&row.0, &row.1)?;
    if !matches!(
        previous.status,
        ToolExecutionAttemptStatusV1::Running
            | ToolExecutionAttemptStatusV1::WaitingExternal
            | ToolExecutionAttemptStatusV1::Cancelling
    ) || row.3 != attempt_status_name(previous.status)
        || row.5.is_some()
        || !same_attempt_identity_and_timeline(&previous, &request.attempt)
        || i64::try_from(request.attempt.last_progress_sequence)
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?
            != row.6
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    let persisted_progress = row
        .7
        .as_deref()
        .map(decode_json::<aura_tool_projection::ToolAttemptProgressView>)
        .transpose()?;
    if request.attempt_projection.progress != persisted_progress {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    verify_cancellation_terminal_alignment(transaction, request, row.4.as_deref())?;

    let batch_id = aura_domain_common::ToolBatchId::from_str(&row.2)?;
    let batch_row = transaction
        .query_row(
            "SELECT request_json, status, entity_revision, result_json
             FROM tool_batches WHERE tool_batch_id=?1",
            [batch_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let batch = decode_json::<ToolBatchRequestV1>(&batch_row.0)?;
    if batch.tool_batch_id != batch_id
        || !matches!(batch_row.1.as_str(), "running" | "cancelling")
        || batch_row.2 != sqlite_revision(request.expected_batch_revision)?
        || batch_row.3.is_some()
        || request.batch_projection.tool_batch_id != batch_id
        || request.batch_projection.batch_content_hash != batch.batch_content_hash
        || request.batch_projection.session_id != batch.invocation_identity.session_id
        || request.batch_projection.turn_task_id != batch.invocation_identity.turn_task_id
        || request.batch_projection.agent_run_id != batch.invocation_identity.agent_run_id
        || request.batch_projection.model_step_id != batch.invocation_identity.model_step_id
        || request.batch_projection.registry_snapshot_ref != batch.registry_snapshot_ref
        || request.batch_projection.control_state_revision != batch.p08_state_revision
        || request.batch_projection.control_state_hash != batch.p08_state_hash
        || request.batch_projection.calls.len() != batch.calls.len()
        || request.session_activity_projection.session_id != batch.invocation_identity.session_id
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }

    let call = batch
        .calls
        .iter()
        .find(|call| call.tool_call_id == request.attempt.tool_call_id)
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    let call_row = transaction
        .query_row(
            "SELECT normalized_call_json, normalized_call_hash_json,
                    status, entity_revision, started_at, completed_at
             FROM tool_calls WHERE tool_call_id=?1 AND tool_batch_id=?2",
            params![request.attempt.tool_call_id.to_string(), batch_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let persisted_call = decode_verified_record::<aura_tool_contract::NormalizedToolCallV1>(
        &call_row.0,
        &call_row.1,
    )?;
    let started_at = call_row
        .4
        .as_deref()
        .map(parse_timestamp)
        .transpose()?
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    if persisted_call != *call
        || !matches!(call_row.2.as_str(), "running" | "cancelling")
        || call_row.3 != sqlite_revision(request.expected_call_revision)?
        || call_row.5.is_some()
        || request.call_projection.tool_batch_id != batch_id
        || request.call_projection.ordinal != call.ordinal
        || request.call_projection.definition_ref != call.definition_ref
        || request.result.started_at != Some(started_at)
        || transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tool_results WHERE tool_call_id=?1)",
            [request.attempt.tool_call_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    match (
        &call.input,
        &request.call_projection.purpose,
        &request.call_projection.invalid_input_issue,
    ) {
        (NormalizedToolCallInputStateV1::Ready { purpose, .. }, Some(actual), None)
            if actual == purpose.as_str() => {}
        _ => return Err(PersistenceError::ToolEntityIntegrity),
    }

    verify_result_attempts(transaction, request)?;
    verify_internal_tool_artifact(
        transaction,
        request.attempt.evidence_ref.ok_or(PersistenceError::ToolEntityIntegrity)?,
        ToolArtifactOwnerRef::ExecutionAttempt {
            tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
        },
    )?;
    verify_internal_tool_artifact(
        transaction,
        request
            .result
            .aggregate_side_effect_evidence_ref
            .ok_or(PersistenceError::ToolEntityIntegrity)?,
        ToolArtifactOwnerRef::ToolCall { tool_call_id: request.result.tool_call_id },
    )?;
    verify_batch_completion(transaction, &batch, request)?;

    let workspace_id = workspace_for_session(transaction, batch.invocation_identity.session_id)?;
    Ok(AttemptFinalizationAuthority { workspace_id, batch })
}

fn verify_cancellation_terminal_alignment(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptFinalizationRequest,
    cancellation_event_id: Option<&str>,
) -> Result<(), PersistenceError> {
    let ToolExecutionAttemptTerminalFact::Cancelled { reason } = &request.terminal else {
        return Ok(());
    };
    let event_id = cancellation_event_id.ok_or(PersistenceError::ToolEntityStateConflict)?;
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM events
             WHERE event_id=?1 AND event_type='ToolExecutionAttemptCancellationRequested'",
            [event_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    let payload = decode_json::<ToolExecutionAttemptCancellationRequestedPayloadV1>(&payload)?;
    if payload.tool_execution_attempt_id != request.attempt.tool_execution_attempt_id
        || payload.reason != *reason
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn verify_result_attempts(
    transaction: &Transaction<'_>,
    request: &ToolExecutionAttemptFinalizationRequest,
) -> Result<(), PersistenceError> {
    let rows = transaction
        .prepare(
            "SELECT record_json, record_hash_json
             FROM tool_execution_attempts
             WHERE tool_call_id=?1 ORDER BY attempt_number",
        )?
        .query_map([request.attempt.tool_call_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() != request.result.attempts.len() {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    for ((record_json, record_hash), outcome_ref) in rows.iter().zip(&request.result.attempts) {
        let persisted =
            decode_verified_record::<ToolExecutionAttemptRecordV1>(record_json, record_hash)?;
        let record =
            if persisted.tool_execution_attempt_id == request.attempt.tool_execution_attempt_id {
                &request.attempt
            } else {
                if !persisted.status.is_terminal() {
                    return Err(PersistenceError::ToolEntityStateConflict);
                }
                &persisted
            };
        if outcome_ref.tool_execution_attempt_id != record.tool_execution_attempt_id
            || outcome_ref.attempt_number != record.attempt_number
            || outcome_ref.side_effect_state != record.side_effect_state
            || outcome_ref.side_effect_evidence_ref != record.evidence_ref
            || !outcome_ref.dispatched
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
    }
    Ok(())
}

fn verify_internal_tool_artifact(
    transaction: &Transaction<'_>,
    artifact_id: aura_domain_common::ArtifactId,
    expected_owner: ToolArtifactOwnerRef,
) -> Result<(), PersistenceError> {
    let row = transaction
        .query_row(
            "SELECT artifact.access_flags, owner.owner_kind, owner.owner_type, owner.owner_id
             FROM artifacts AS artifact
             JOIN artifact_owners AS owner USING (artifact_id)
             WHERE artifact.artifact_id=?1",
            [artifact_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ArtifactNotFound)?;
    let owner = parse_owner(&row.1, &row.2, row.3.as_deref())?;
    if row.0 != i64::from(ArtifactAccess::NONE.bits())
        || owner != (ArtifactOwnerRef::Tool { owner: expected_owner })
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn verify_batch_completion(
    transaction: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    request: &ToolExecutionAttemptFinalizationRequest,
) -> Result<(), PersistenceError> {
    let mut results = Vec::with_capacity(batch.calls.len());
    let mut complete = true;
    for call in &batch.calls {
        if call.tool_call_id == request.result.tool_call_id {
            results.push(request.result.clone());
            continue;
        }
        let stored = transaction
            .query_row(
                "SELECT result_json, result_hash_json FROM tool_results
                 WHERE tool_call_id=?1 AND tool_batch_id=?2",
                params![call.tool_call_id.to_string(), batch.tool_batch_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .map(|(json, hash)| decode_verified_record::<ToolResultV1>(&json, &hash))
            .transpose()?;
        if let Some(stored) = stored {
            results.push(stored);
        } else {
            complete = false;
        }
    }
    match (&request.batch_result, complete) {
        (Some(batch_result), true) => {
            batch_result.validate(batch).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            if batch_result.results != results {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
        }
        (None, false) => {}
        _ => return Err(PersistenceError::ToolEntityStateConflict),
    }
    let completed_count =
        u32::try_from(results.len()).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if let Some(active) = request
        .session_activity_projection
        .active_batches
        .iter()
        .find(|summary| summary.tool_batch_id == batch.tool_batch_id)
        && (active.call_count != u32::try_from(batch.calls.len()).unwrap_or(u32::MAX)
            || active.completed_call_count != completed_count)
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn terminal_lifecycle_event(
    request: &ToolExecutionAttemptFinalizationRequest,
) -> Result<ToolLifecycleEventV1, PersistenceError> {
    let evidence_ref = request.attempt.evidence_ref.ok_or(PersistenceError::ToolEntityIntegrity)?;
    let completed_at = request.attempt.completed_at.ok_or(PersistenceError::ToolEntityIntegrity)?;
    Ok(match &request.terminal {
        ToolExecutionAttemptTerminalFact::Succeeded => {
            ToolLifecycleEventV1::ToolExecutionAttemptSucceeded(
                ToolExecutionAttemptSucceededPayloadV1 {
                    tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
                    side_effect_state: request.attempt.side_effect_state,
                    evidence_ref,
                    completed_at,
                },
            )
        }
        ToolExecutionAttemptTerminalFact::Failed { error } => {
            ToolLifecycleEventV1::ToolExecutionAttemptFailed(ToolExecutionAttemptFailedPayloadV1 {
                tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
                side_effect_state: request.attempt.side_effect_state,
                error: error.clone(),
                evidence_ref,
                completed_at,
            })
        }
        ToolExecutionAttemptTerminalFact::Cancelled { reason } => {
            ToolLifecycleEventV1::ToolExecutionAttemptCancelled(
                ToolExecutionAttemptCancelledPayloadV1 {
                    tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
                    reason: *reason,
                    side_effect_state: request.attempt.side_effect_state,
                    evidence_ref,
                    completed_at,
                },
            )
        }
        ToolExecutionAttemptTerminalFact::Indeterminate { reason, error } => {
            ToolLifecycleEventV1::ToolExecutionAttemptIndeterminate(
                ToolExecutionAttemptIndeterminatePayloadV1 {
                    tool_execution_attempt_id: request.attempt.tool_execution_attempt_id,
                    reason: *reason,
                    side_effect_state: request.attempt.side_effect_state,
                    error: error.clone(),
                    evidence_ref,
                    completed_at,
                },
            )
        }
    })
}

fn finalize_attempt(
    transaction: &Transaction<'_>,
    attempt: &ToolExecutionAttemptRecordV1,
    terminal_event_id: EventId,
) -> Result<(), PersistenceError> {
    let completed_at = attempt.completed_at.ok_or(PersistenceError::ToolEntityIntegrity)?;
    let evidence_ref = attempt.evidence_ref.ok_or(PersistenceError::ToolEntityIntegrity)?;
    let (record_json, record_hash_json) = encode_record(attempt)?;
    let changed = transaction.execute(
        "UPDATE tool_execution_attempts
         SET status=?1, side_effect_state=?2, record_json=?3, record_hash_json=?4,
             evidence_artifact_id=?5, terminal_event_id=?6,
             completed_at=?7, updated_at=?7
         WHERE tool_execution_attempt_id=?8
           AND status IN ('dispatch_prepared', 'running', 'waiting_external', 'cancelling')
           AND terminal_event_id IS NULL",
        params![
            attempt_status_name(attempt.status),
            side_effect_state_name(attempt.side_effect_state),
            record_json,
            record_hash_json,
            evidence_ref.to_string(),
            terminal_event_id.to_string(),
            completed_at.to_string(),
            attempt.tool_execution_attempt_id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(())
}

fn finalize_tool_call_and_batch(
    transaction: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    request: &ToolExecutionAttemptFinalizationRequest,
) -> Result<(), PersistenceError> {
    let call_changed = transaction.execute(
        "UPDATE tool_calls
         SET status='finalized', entity_revision=entity_revision+1,
             completed_at=?1, updated_at=?1
         WHERE tool_call_id=?2 AND tool_batch_id=?3
           AND status IN ('running', 'cancelling') AND entity_revision=?4",
        params![
            request.result.completed_at.to_string(),
            request.result.tool_call_id.to_string(),
            batch.tool_batch_id.to_string(),
            sqlite_revision(request.expected_call_revision)?,
        ],
    )?;
    if call_changed != 1 {
        return Err(PersistenceError::ToolEntityRevisionConflict);
    }

    let batch_changed = if let Some(batch_result) = &request.batch_result {
        let (result_json, result_hash_json) = encode_record(batch_result)?;
        transaction.execute(
            "UPDATE tool_batches
             SET status='completed', result_json=?1, result_hash_json=?2,
                 entity_revision=entity_revision+1, completed_at=?3, updated_at=?3
             WHERE tool_batch_id=?4 AND status IN ('running', 'cancelling')
               AND entity_revision=?5 AND result_json IS NULL",
            params![
                result_json,
                result_hash_json,
                batch_result.completed_at.to_string(),
                batch.tool_batch_id.to_string(),
                sqlite_revision(request.expected_batch_revision)?,
            ],
        )?
    } else {
        transaction.execute(
            "UPDATE tool_batches
             SET entity_revision=entity_revision+1, updated_at=?1
             WHERE tool_batch_id=?2 AND status IN ('running', 'cancelling')
               AND entity_revision=?3 AND result_json IS NULL",
            params![
                request.result.completed_at.to_string(),
                batch.tool_batch_id.to_string(),
                sqlite_revision(request.expected_batch_revision)?,
            ],
        )?
    };
    if batch_changed != 1 {
        return Err(PersistenceError::ToolEntityRevisionConflict);
    }
    Ok(())
}

fn same_attempt_identity_and_timeline(
    previous: &ToolExecutionAttemptRecordV1,
    next: &ToolExecutionAttemptRecordV1,
) -> bool {
    previous.tool_execution_attempt_id == next.tool_execution_attempt_id
        && previous.tool_call_id == next.tool_call_id
        && previous.attempt_number == next.attempt_number
        && previous.started_at == next.started_at
        && previous.dispatch_prepared_at == next.dispatch_prepared_at
}

const fn side_effect_state_name(value: SideEffectStateV1) -> &'static str {
    match value {
        SideEffectStateV1::NotStarted => "not_started",
        SideEffectStateV1::NoEffect => "no_effect",
        SideEffectStateV1::FullyApplied => "fully_applied",
        SideEffectStateV1::PartiallyApplied => "partially_applied",
        SideEffectStateV1::Compensated => "compensated",
        SideEffectStateV1::Unknown => "unknown",
        SideEffectStateV1::Unassessed => "unassessed",
    }
}

const fn attempt_status_name(value: ToolExecutionAttemptStatusV1) -> &'static str {
    match value {
        ToolExecutionAttemptStatusV1::Started => "started",
        ToolExecutionAttemptStatusV1::DispatchPrepared => "dispatch_prepared",
        ToolExecutionAttemptStatusV1::Running => "running",
        ToolExecutionAttemptStatusV1::WaitingExternal => "waiting_external",
        ToolExecutionAttemptStatusV1::Cancelling => "cancelling",
        ToolExecutionAttemptStatusV1::Succeeded => "succeeded",
        ToolExecutionAttemptStatusV1::Failed => "failed",
        ToolExecutionAttemptStatusV1::Cancelled => "cancelled",
        ToolExecutionAttemptStatusV1::Indeterminate => "indeterminate",
    }
}

struct AdmissionPartition {
    plan_calls: BTreeMap<ToolCallId, ToolExecutionPlanCallV1>,
    not_executed: BTreeMap<ToolCallId, ToolResultV1>,
}

fn validate_admission_request(
    request: &ToolBatchAdmissionCommitRequest,
) -> Result<AdmissionPartition, PersistenceError> {
    request.batch.validate_hash().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if request.batch.has_duplicate_tool_call_id() {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    request.execution_plan.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if request.execution_plan.tool_batch_id != request.batch.tool_batch_id
        || request.execution_plan.registry_snapshot_ref != request.batch.registry_snapshot_ref
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }

    let batch_calls = request
        .batch
        .calls
        .iter()
        .map(|call| (call.tool_call_id, call))
        .collect::<BTreeMap<_, _>>();
    let plan_calls = request
        .execution_plan
        .calls
        .iter()
        .map(|call| (call.tool_call_id, call.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut not_executed = BTreeMap::new();
    let mut previous_ordinal = None;
    for result in &request.not_executed_results {
        result.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        if !matches!(result.outcome, ToolResultOutcomeV1::NotExecuted { .. })
            || result.side_effect_state != SideEffectStateV1::NotStarted
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        let call =
            batch_calls.get(&result.tool_call_id).ok_or(PersistenceError::ToolEntityIntegrity)?;
        if previous_ordinal.is_some_and(|previous| previous >= call.ordinal)
            || not_executed.insert(result.tool_call_id, result.clone()).is_some()
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        previous_ordinal = Some(call.ordinal);
    }

    let mut covered = BTreeSet::new();
    for plan_call in request.execution_plan.calls.iter() {
        let batch_call = batch_calls
            .get(&plan_call.tool_call_id)
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        if plan_call.ordinal != batch_call.ordinal || !covered.insert(plan_call.tool_call_id) {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
    }
    for result in &request.not_executed_results {
        if !covered.insert(result.tool_call_id) {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
    }
    if covered.len() != request.batch.calls.len() {
        return Err(PersistenceError::ToolEntityIntegrity);
    }

    validate_admission_projections(request, &plan_calls, &not_executed)?;
    Ok(AdmissionPartition { plan_calls, not_executed })
}

fn validate_admission_projections(
    request: &ToolBatchAdmissionCommitRequest,
    plan_calls: &BTreeMap<ToolCallId, ToolExecutionPlanCallV1>,
    not_executed: &BTreeMap<ToolCallId, ToolResultV1>,
) -> Result<(), PersistenceError> {
    let batch_projection = &request.batch_projection;
    if batch_projection.tool_batch_id != request.batch.tool_batch_id
        || batch_projection.batch_content_hash != request.batch.batch_content_hash
        || batch_projection.session_id != request.batch.invocation_identity.session_id
        || batch_projection.turn_task_id != request.batch.invocation_identity.turn_task_id
        || batch_projection.agent_run_id != request.batch.invocation_identity.agent_run_id
        || batch_projection.model_step_id != request.batch.invocation_identity.model_step_id
        || batch_projection.registry_snapshot_ref != request.batch.registry_snapshot_ref
        || batch_projection.control_state_revision != request.batch.p08_state_revision
        || batch_projection.control_state_hash != request.batch.p08_state_hash
        || batch_projection.status != ToolBatchStatusV1::Admitted
        || batch_projection.calls.len() != request.batch.calls.len()
        || request.call_projections.len() != request.batch.calls.len()
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }

    for ((call, projection), summary) in
        request.batch.calls.iter().zip(&request.call_projections).zip(&batch_projection.calls)
    {
        if projection.tool_call_id != call.tool_call_id
            || projection.tool_batch_id != request.batch.tool_batch_id
            || projection.ordinal != call.ordinal
            || projection.definition_ref != call.definition_ref
            || summary.tool_call_id != call.tool_call_id
            || summary.tool_batch_id != request.batch.tool_batch_id
            || summary.ordinal != call.ordinal
            || summary.tool_id != call.definition_ref.as_ref().map(|r| r.tool_id)
            || summary.tool_version != call.definition_ref.as_ref().map(|r| r.tool_version.clone())
            || summary.status != projection.status
            || !projection.attempt_ids.is_empty()
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }

        match (&call.input, &projection.purpose, &projection.invalid_input_issue) {
            (NormalizedToolCallInputStateV1::Ready { purpose, .. }, Some(actual), None)
                if actual == purpose.as_str() => {}
            (NormalizedToolCallInputStateV1::InvalidInput { issue, .. }, None, Some(actual))
                if actual == issue => {}
            _ => return Err(PersistenceError::ToolEntityIntegrity),
        }

        if plan_calls.contains_key(&call.tool_call_id) {
            if projection.status != ToolCallStatusV1::Queued
                || projection.outcome.is_some()
                || projection.side_effect_state.is_some()
                || projection.completed_at.is_some()
            {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
        } else {
            let result = not_executed
                .get(&call.tool_call_id)
                .ok_or(PersistenceError::ToolEntityIntegrity)?;
            let ToolResultOutcomeV1::NotExecuted { reason, .. } = result.outcome else {
                return Err(PersistenceError::ToolEntityIntegrity);
            };
            if projection.status != ToolCallStatusV1::Finalized
                || projection.outcome != Some(ToolResultOutcomeSummaryView::NotExecuted { reason })
                || projection.side_effect_state != Some(SideEffectStateV1::NotStarted)
                || projection.completed_at != Some(result.completed_at)
            {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
        }
    }

    if request.session_activity_projection.session_id
        != request.batch.invocation_identity.session_id
        || !request.session_activity_projection.active_batches.iter().any(|summary| {
            summary.tool_batch_id == request.batch.tool_batch_id
                && summary.status == ToolBatchStatusV1::Admitted
                && summary.call_count
                    == u32::try_from(request.batch.calls.len()).unwrap_or(u32::MAX)
        })
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn verify_admission_authority(
    transaction: &Transaction<'_>,
    request: &ToolBatchAdmissionCommitRequest,
) -> Result<WorkspaceId, PersistenceError> {
    let identity = &request.batch.invocation_identity;
    let workspace_id = transaction
        .query_row(
            "SELECT workspace_id FROM sessions WHERE session_id=?1",
            [identity.session_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)
        .and_then(|value| WorkspaceId::from_str(&value).map_err(PersistenceError::from))?;
    let scope_matches = transaction.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM turn_tasks AS turn
            JOIN agent_runs AS agent ON agent.turn_task_id=turn.turn_task_id
            WHERE turn.turn_task_id=?1
              AND turn.session_id=?2
              AND agent.agent_run_id=?3
        )",
        params![
            identity.turn_task_id.to_string(),
            identity.session_id.to_string(),
            identity.agent_run_id.to_string(),
        ],
        |row| row.get::<_, bool>(0),
    )?;
    if !scope_matches {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    let registry_snapshot_json = transaction
        .query_row(
            "SELECT snapshot_json FROM tool_registry_snapshots
             WHERE tool_registry_snapshot_id=?1
               AND registry_revision=?2
               AND content_hash_json=?3",
            params![
                request.batch.registry_snapshot_ref.tool_registry_snapshot_id.to_string(),
                sqlite_revision(request.batch.registry_snapshot_ref.registry_revision)?,
                encode_json(&request.batch.registry_snapshot_ref.content_hash)?,
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(PersistenceError::ToolRegistryIntegrity)?;
    let registry = decode_json::<ToolRegistrySnapshotV1>(&registry_snapshot_json)?;
    registry.validate().map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
    if registry.snapshot_ref != request.batch.registry_snapshot_ref {
        return Err(PersistenceError::ToolRegistryIntegrity);
    }
    for plan_call in &request.execution_plan.calls {
        let call = request
            .batch
            .calls
            .iter()
            .find(|call| call.tool_call_id == plan_call.tool_call_id)
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        let catalog_entry = registry
            .catalog_entries
            .iter()
            .find(|entry| {
                call.definition_ref.as_ref().is_some_and(|r| {
                    entry.tool_id == r.tool_id && entry.tool_version == r.tool_version
                })
            })
            .ok_or(PersistenceError::ToolRegistryIntegrity)?;
        let binding = registry
            .effective_bindings
            .iter()
            .find(|binding| {
                call.definition_ref.as_ref().is_some_and(|r| {
                    binding.tool_id == r.tool_id && binding.tool_version == r.tool_version
                })
            })
            .ok_or(PersistenceError::ToolRegistryIntegrity)?;
        if catalog_entry.tool_instance_id != binding.tool_instance_id
            || catalog_entry.definition_fingerprint != binding.definition_fingerprint
        {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
        let definition = transaction
            .query_row(
                "SELECT record_json, record_hash_json FROM tool_definitions
                 WHERE tool_id=?1 AND tool_version_json=?2",
                params![
                    resolved_tool(call)?.tool_id.to_string(),
                    encode_json(&resolved_tool(call)?.tool_version)?,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .map(|(record, hash)| decode_verified_record::<ToolDefinitionRecordV1>(&record, &hash))
            .transpose()?
            .ok_or(PersistenceError::ToolEntityNotFound)?;
        let instance = transaction
            .query_row(
                "SELECT record_json, record_hash_json FROM tool_instances
                 WHERE tool_instance_id=?1",
                [binding.tool_instance_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .map(|(record, hash)| decode_verified_record::<ToolInstanceRecordV1>(&record, &hash))
            .transpose()?
            .ok_or(PersistenceError::ToolEntityNotFound)?;
        if definition.tool_id != resolved_tool(call)?.tool_id
            || definition.tool_version != resolved_tool(call)?.tool_version
            || definition.definition_fingerprint != binding.definition_fingerprint
            || instance.tool_instance_id != binding.tool_instance_id
            || instance.tool_id != binding.tool_id
            || instance.tool_version != binding.tool_version
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
    }
    Ok(workspace_id)
}

fn ensure_new_batch_and_calls(
    transaction: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
) -> Result<(), PersistenceError> {
    let batch_exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM tool_batches WHERE tool_batch_id=?1)",
        [batch.tool_batch_id.to_string()],
        |row| row.get::<_, bool>(0),
    )?;
    if batch_exists {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    for call in &batch.calls {
        let call_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tool_calls WHERE tool_call_id=?1)",
            [call.tool_call_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?;
        if call_exists {
            return Err(PersistenceError::ToolEntityStateConflict);
        }
    }
    Ok(())
}

fn insert_admitted_batch(
    transaction: &Transaction<'_>,
    request: &ToolBatchAdmissionCommitRequest,
) -> Result<(), PersistenceError> {
    let batch_json = encode_json(&request.batch)?;
    let batch_hash_json = encode_json(&request.batch.batch_content_hash)?;
    let execution_plan_json = encode_json(&request.execution_plan)?;
    let execution_plan_hash_json = encode_json(&request.execution_plan.execution_plan_hash)?;
    transaction.execute(
        "INSERT INTO tool_batches(
            tool_batch_id, batch_content_hash_json, session_id, turn_task_id,
            agent_run_id, model_step_id, registry_snapshot_id, registry_revision,
            registry_content_hash_json, p08_state_revision, p08_state_hash_json,
            status, request_json, request_hash_json, execution_plan_json,
            execution_plan_hash_json, entity_revision, created_at, admitted_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
            'admitted', ?12, ?2, ?13, ?14, 1, ?15, ?15, ?15
         )",
        params![
            request.batch.tool_batch_id.to_string(),
            batch_hash_json,
            request.batch.invocation_identity.session_id.to_string(),
            request.batch.invocation_identity.turn_task_id.to_string(),
            request.batch.invocation_identity.agent_run_id.to_string(),
            request.batch.invocation_identity.model_step_id.to_string(),
            request.batch.registry_snapshot_ref.tool_registry_snapshot_id.to_string(),
            sqlite_revision(request.batch.registry_snapshot_ref.registry_revision)?,
            encode_json(&request.batch.registry_snapshot_ref.content_hash)?,
            sqlite_revision(request.batch.p08_state_revision)?,
            encode_json(&request.batch.p08_state_hash)?,
            batch_json,
            execution_plan_json,
            execution_plan_hash_json,
            request.admitted_at.to_string(),
        ],
    )?;
    Ok(())
}

fn insert_admitted_calls(
    transaction: &Transaction<'_>,
    request: &ToolBatchAdmissionCommitRequest,
    partition: &AdmissionPartition,
) -> Result<(), PersistenceError> {
    for call in &request.batch.calls {
        let (call_json, call_hash_json) = encode_record(call)?;
        let plan_call_json =
            partition.plan_calls.get(&call.tool_call_id).map(encode_json).transpose()?;
        let result = partition.not_executed.get(&call.tool_call_id);
        let status = if result.is_some() { "finalized" } else { "queued" };
        transaction.execute(
            "INSERT INTO tool_calls(
                tool_call_id, tool_batch_id, ordinal, tool_id, tool_version_json,
                status, normalized_call_json, normalized_call_hash_json,
                execution_plan_call_json, entity_revision, created_at, completed_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?11, ?10)",
            params![
                call.tool_call_id.to_string(),
                request.batch.tool_batch_id.to_string(),
                i64::from(call.ordinal),
                resolved_tool(call)?.tool_id.to_string(),
                encode_json(&resolved_tool(call)?.tool_version)?,
                status,
                call_json,
                call_hash_json,
                plan_call_json,
                request.admitted_at.to_string(),
                result.map(|result| result.completed_at.to_string()),
            ],
        )?;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RegistryPayloadBindingHashInputV1<'a> {
    tool_registry_snapshot_id: aura_domain_common::ToolRegistrySnapshotId,
    tool_id: aura_domain_common::ToolId,
    tool_package_id: aura_domain_common::ToolPackageId,
    package_version: &'a aura_domain_common::CanonicalSemVer,
}

fn acquire_batch_payload_leases(
    transaction: &Transaction<'_>,
    request: &ToolBatchAdmissionCommitRequest,
) -> Result<(), PersistenceError> {
    let snapshot_id = request.batch.registry_snapshot_ref.tool_registry_snapshot_id;
    let mut payloads = BTreeMap::new();
    for plan_call in &request.execution_plan.calls {
        let normalized = request
            .batch
            .calls
            .iter()
            .find(|call| call.tool_call_id == plan_call.tool_call_id)
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        let reference = resolved_tool(normalized)?;
        let row = transaction
            .query_row(
                "SELECT d.definition_json, d.definition_hash_json,
                        b.tool_package_id, b.package_version_json, b.binding_hash_json
                 FROM tool_registry_definition_records d
                 LEFT JOIN tool_registry_package_payload_bindings b
                   ON b.tool_registry_snapshot_id=d.tool_registry_snapshot_id
                  AND b.tool_id=d.tool_id
                 WHERE d.tool_registry_snapshot_id=?1 AND d.tool_id=?2",
                params![snapshot_id.to_string(), reference.tool_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or(PersistenceError::ToolRegistryIntegrity)?;
        let definition = decode_verified_record::<ToolDefinitionRecordV1>(&row.0, &row.1)?;
        if definition.tool_id != reference.tool_id
            || definition.tool_version != reference.tool_version
        {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
        match definition.tool_package_id {
            None if row.2.is_none() && row.3.is_none() && row.4.is_none() => {}
            None => return Err(PersistenceError::ToolRegistryIntegrity),
            Some(tool_package_id) => {
                let package_id = row
                    .2
                    .as_deref()
                    .ok_or(PersistenceError::ToolRegistryIntegrity)?
                    .parse::<aura_domain_common::ToolPackageId>()?;
                let package_version = row
                    .3
                    .as_deref()
                    .map(decode_json::<aura_domain_common::CanonicalSemVer>)
                    .transpose()?
                    .ok_or(PersistenceError::ToolRegistryIntegrity)?;
                let binding_hash = row
                    .4
                    .as_deref()
                    .map(decode_json::<aura_domain_common::jcs::CanonicalHashV1>)
                    .transpose()?
                    .ok_or(PersistenceError::ToolRegistryIntegrity)?;
                let expected_hash =
                    aura_domain_common::jcs::canonical_sha256(&RegistryPayloadBindingHashInputV1 {
                        tool_registry_snapshot_id: snapshot_id,
                        tool_id: definition.tool_id,
                        tool_package_id,
                        package_version: &package_version,
                    })
                    .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
                if package_id != tool_package_id || binding_hash != expected_hash {
                    return Err(PersistenceError::ToolRegistryIntegrity);
                }
                let eligible = transaction.query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM tool_packages p
                        JOIN tool_package_versions v ON v.tool_package_id=p.tool_package_id
                        JOIN tool_package_payloads payload
                          ON payload.tool_package_id=v.tool_package_id
                         AND payload.package_version_json=v.package_version_json
                        WHERE p.tool_package_id=?1 AND p.lifecycle='installed'
                          AND p.deletion_pending=0
                          AND v.package_version_json=?2
                          AND payload.state='published'
                    )",
                    params![package_id.to_string(), encode_json(&package_version)?],
                    |row| row.get::<_, bool>(0),
                )?;
                if !eligible {
                    return Err(PersistenceError::ToolEntityStateConflict);
                }
                payloads.insert((package_id, encode_json(&package_version)?), binding_hash);
            }
        }
    }

    for ((tool_package_id, package_version_json), binding_hash) in payloads {
        let strong_reference_id =
            format!("{}:{}:{}", request.batch.tool_batch_id, tool_package_id, binding_hash.hex());
        transaction.execute(
            "INSERT INTO tool_package_payload_strong_references(
                reference_kind, reference_id, tool_package_id, package_version_json,
                created_at, released_at
             ) VALUES ('tool_batch', ?1, ?2, ?3, ?4, NULL)",
            params![
                strong_reference_id,
                tool_package_id.to_string(),
                package_version_json,
                request.admitted_at.to_string(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO tool_batch_payload_leases(
                tool_batch_id, tool_package_id, package_version_json,
                strong_reference_id, acquired_at, released_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
            params![
                request.batch.tool_batch_id.to_string(),
                tool_package_id.to_string(),
                package_version_json,
                strong_reference_id,
                request.admitted_at.to_string(),
            ],
        )?;
        refresh_payload_reference_count(
            transaction,
            tool_package_id,
            &package_version_json,
            request.admitted_at,
        )?;
    }
    Ok(())
}

fn release_batch_payload_leases(
    transaction: &Transaction<'_>,
    tool_batch_id: aura_domain_common::ToolBatchId,
    released_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let leases = {
        let mut statement = transaction.prepare(
            "SELECT tool_package_id, package_version_json, strong_reference_id
             FROM tool_batch_payload_leases
             WHERE tool_batch_id=?1 AND released_at IS NULL
             ORDER BY tool_package_id, package_version_json",
        )?;
        statement
            .query_map([tool_batch_id.to_string()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (package_id, package_version_json, reference_id) in leases {
        let package_id = package_id.parse::<aura_domain_common::ToolPackageId>()?;
        let strong_changed = transaction.execute(
            "UPDATE tool_package_payload_strong_references SET released_at=?1
             WHERE reference_kind='tool_batch' AND reference_id=?2 AND released_at IS NULL",
            params![released_at.to_string(), reference_id],
        )?;
        let lease_changed = transaction.execute(
            "UPDATE tool_batch_payload_leases SET released_at=?1
             WHERE tool_batch_id=?2 AND tool_package_id=?3
               AND package_version_json=?4 AND released_at IS NULL",
            params![
                released_at.to_string(),
                tool_batch_id.to_string(),
                package_id.to_string(),
                package_version_json,
            ],
        )?;
        if strong_changed != 1 || lease_changed != 1 {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        refresh_payload_reference_count(
            transaction,
            package_id,
            &package_version_json,
            released_at,
        )?;
    }
    Ok(())
}

fn refresh_payload_reference_count(
    transaction: &Transaction<'_>,
    tool_package_id: aura_domain_common::ToolPackageId,
    package_version_json: &str,
    changed_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let row = transaction
        .query_row(
            "SELECT record_json, record_hash_json FROM tool_package_versions
             WHERE tool_package_id=?1 AND package_version_json=?2",
            params![tool_package_id.to_string(), package_version_json],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let mut version =
        decode_verified_record::<aura_tool_domain::ToolPackageVersionRecordV1>(&row.0, &row.1)?;
    let live_count = transaction.query_row(
        "SELECT COUNT(*) FROM tool_package_payload_strong_references
         WHERE tool_package_id=?1 AND package_version_json=?2 AND released_at IS NULL",
        params![tool_package_id.to_string(), package_version_json],
        |row| row.get::<_, i64>(0),
    )?;
    version.payload_strong_reference_count =
        u64::try_from(live_count).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    version.payload_gc_eligible_at = (live_count == 0).then(|| {
        UtcTimestamp::from_value(changed_at.value() + time::Duration::days(PAYLOAD_RETENTION_DAYS))
    });
    let (record_json, record_hash_json) = encode_record(&version)?;
    let changed = transaction.execute(
        "UPDATE tool_package_versions SET record_json=?1, record_hash_json=?2
         WHERE tool_package_id=?3 AND package_version_json=?4",
        params![record_json, record_hash_json, tool_package_id.to_string(), package_version_json],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn insert_tool_result(
    transaction: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    result: &ToolResultV1,
    finalized_event_id: EventId,
) -> Result<(), PersistenceError> {
    let ordinal = batch
        .calls
        .iter()
        .find(|call| call.tool_call_id == result.tool_call_id)
        .map(|call| call.ordinal)
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    let (result_json, result_hash_json) = encode_record(result)?;
    transaction.execute(
        "INSERT INTO tool_results(
            tool_call_id, tool_batch_id, ordinal, outcome_kind, side_effect_state,
            result_json, result_hash_json, finalized_event_id,
            model_visible_result_artifact_id, user_audit_result_artifact_id,
            internal_diagnostic_artifact_id, aggregate_side_effect_evidence_artifact_id,
            completed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                   ?9, ?10, ?11, ?12, ?13)",
        params![
            result.tool_call_id.to_string(),
            batch.tool_batch_id.to_string(),
            i64::from(ordinal),
            result_outcome_name(&result.outcome),
            side_effect_state_name(result.side_effect_state),
            result_json,
            result_hash_json,
            finalized_event_id.to_string(),
            result.model_visible_result_ref.map(|value| value.to_string()),
            result.user_audit_result_ref.map(|value| value.to_string()),
            result.internal_diagnostic_ref.map(|value| value.to_string()),
            result.aggregate_side_effect_evidence_ref.map(|value| value.to_string()),
            result.completed_at.to_string(),
        ],
    )?;
    Ok(())
}

const fn result_outcome_name(value: &ToolResultOutcomeV1) -> &'static str {
    match value {
        ToolResultOutcomeV1::Succeeded => "succeeded",
        ToolResultOutcomeV1::OperationRejected { .. } => "operation_rejected",
        ToolResultOutcomeV1::Failed { .. } => "failed",
        ToolResultOutcomeV1::Cancelled { .. } => "cancelled",
        ToolResultOutcomeV1::NotExecuted { .. } => "not_executed",
        ToolResultOutcomeV1::Indeterminate { .. } => "indeterminate",
    }
}

fn execution_event(
    event_id: EventId,
    workspace_id: WorkspaceId,
    batch: &ToolBatchRequestV1,
    correlation_id: CorrelationId,
    causation_id: Option<CausationId>,
    lifecycle: ToolLifecycleEventV1,
    created_at: UtcTimestamp,
) -> Result<PersistentEventDraft, PersistenceError> {
    let event_type = lifecycle.event_type().to_owned();
    let mut envelope = serde_json::to_value(lifecycle)?;
    let payload = envelope
        .as_object_mut()
        .and_then(|object| object.remove("payload"))
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    Ok(PersistentEventDraft {
        event_id,
        workspace_id: Some(workspace_id),
        session_id: Some(batch.invocation_identity.session_id),
        turn_task_id: Some(batch.invocation_identity.turn_task_id),
        agent_run_id: Some(batch.invocation_identity.agent_run_id),
        event_type,
        event_version: 1,
        actor: EventActor::SystemRuntime,
        correlation_id,
        causation_id,
        atomic_group_id: None,
        payload_json: Some(payload),
        payload_artifact_ref: None,
        searchable_text: None,
        created_at,
    })
}
