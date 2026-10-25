use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::ToolCallId;
use aura_domain_common::{ErrorEnvelope, UtcTimestamp};
use aura_execution_contract::SideEffectStateV1;
use aura_tool_contract::{
    NormalizedToolCallInputStateV1, ToolBatchRequestV1, ToolBatchResultV1, ToolBatchStatusV1,
    ToolCallStatusV1, ToolExecutionAttemptRecordV1, ToolExecutionAttemptStatusV1,
    ToolExecutionPlanV1, ToolResultOutcomeV1, ToolResultV1,
};
use thiserror::Error;

use crate::{
    SessionToolActivityProjection, ToolAttemptProgressView, ToolBatchActivitySummaryView,
    ToolBatchProjection, ToolCallActivitySummaryView, ToolCallErrorClassV1, ToolCallProjection,
    ToolExecutionAttemptProjection, ToolProjectionErrorView, ToolResultOutcomeSummaryView,
    UserReadableToolArtifactView,
};

/// Core-owned, in-memory projection state for one admitted ToolBatch.
///
/// The persistence layer independently validates every emitted projection.
/// This builder keeps production callers from hand-assembling four correlated
/// views for each lifecycle transition.
#[derive(Clone, Debug)]
pub struct ToolExecutionProjectionStateV1 {
    batch: ToolBatchProjection,
    calls: BTreeMap<ToolCallId, ToolCallProjection>,
    session: SessionToolActivityProjection,
    attempts: BTreeMap<aura_domain_common::ToolExecutionAttemptId, ToolExecutionAttemptProjection>,
    results: BTreeMap<ToolCallId, ToolResultV1>,
}

#[derive(Clone, Debug)]
pub struct ToolAdmissionProjectionSetV1 {
    pub batch: ToolBatchProjection,
    pub calls: Vec<ToolCallProjection>,
    pub session: SessionToolActivityProjection,
}

#[derive(Clone, Debug)]
pub struct ToolDispatchPreparedProjectionSetV1 {
    pub batch: ToolBatchProjection,
    pub call: ToolCallProjection,
    pub attempt: ToolExecutionAttemptProjection,
    pub session: SessionToolActivityProjection,
}

#[derive(Clone, Debug)]
pub struct ToolAttemptFinalizationProjectionSetV1 {
    pub batch: ToolBatchProjection,
    pub call: ToolCallProjection,
    pub attempt: ToolExecutionAttemptProjection,
    pub session: SessionToolActivityProjection,
    pub batch_result: Option<ToolBatchResultV1>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolExecutionProjectionErrorV1 {
    #[error("ToolBatch, execution plan, display names, or preflight results are inconsistent")]
    AdmissionMismatch,
    #[error("the requested ToolCall projection transition is invalid")]
    InvalidCallTransition,
    #[error("the requested ToolExecutionAttempt projection transition is invalid")]
    InvalidAttemptTransition,
    #[error("the ToolResult is inconsistent with the tracked call or attempts")]
    InvalidResult,
}

impl ToolExecutionProjectionStateV1 {
    pub fn admitted(
        batch: &ToolBatchRequestV1,
        execution_plan: &ToolExecutionPlanV1,
        display_names: &[(ToolCallId, String)],
        not_executed_results: &[ToolResultV1],
    ) -> Result<(Self, ToolAdmissionProjectionSetV1), ToolExecutionProjectionErrorV1> {
        batch.validate_hash().map_err(|_| ToolExecutionProjectionErrorV1::AdmissionMismatch)?;
        execution_plan.validate().map_err(|_| ToolExecutionProjectionErrorV1::AdmissionMismatch)?;
        if execution_plan.tool_batch_id != batch.tool_batch_id
            || execution_plan.registry_snapshot_ref != batch.registry_snapshot_ref
        {
            return Err(ToolExecutionProjectionErrorV1::AdmissionMismatch);
        }

        let call_ids = batch.calls.iter().map(|call| call.tool_call_id).collect::<BTreeSet<_>>();
        let planned_ids =
            execution_plan.calls.iter().map(|call| call.tool_call_id).collect::<BTreeSet<_>>();
        let names = display_names.iter().cloned().collect::<BTreeMap<_, _>>();
        let results = not_executed_results
            .iter()
            .cloned()
            .map(|result| (result.tool_call_id, result))
            .collect::<BTreeMap<_, _>>();
        if names.len() != batch.calls.len()
            || names.keys().copied().collect::<BTreeSet<_>>() != call_ids
            || planned_ids.len() != execution_plan.calls.len()
            || !planned_ids.is_subset(&call_ids)
            || results.len() != not_executed_results.len()
            || results.keys().any(|id| planned_ids.contains(id) || !call_ids.contains(id))
            || planned_ids.len() + results.len() != call_ids.len()
        {
            return Err(ToolExecutionProjectionErrorV1::AdmissionMismatch);
        }
        for result in results.values() {
            if result.validate().is_err()
                || !matches!(result.outcome, ToolResultOutcomeV1::NotExecuted { .. })
            {
                return Err(ToolExecutionProjectionErrorV1::AdmissionMismatch);
            }
        }

        let mut calls = BTreeMap::new();
        let mut summaries = Vec::with_capacity(batch.calls.len());
        for call in &batch.calls {
            let result = results.get(&call.tool_call_id);
            let status = result.map_or(ToolCallStatusV1::Queued, |_| ToolCallStatusV1::Finalized);
            let outcome = result.map(|result| outcome_summary(&result.outcome));
            let side_effect_state = result.map(|result| result.side_effect_state);
            let completed_at = result.map(|result| result.completed_at);
            let safe_error = result.and_then(result_safe_error);
            // The admission-time projection has no error envelope of its own: it reads
            // the call's input state, so the only class it can honestly report is the
            // invalid-request one it just observed. A result that carries a stored error
            // is classified where that error is read, by the activity projection.
            let error_class = result.and_then(|result| match &result.outcome {
                ToolResultOutcomeV1::Failed { .. }
                | ToolResultOutcomeV1::OperationRejected { .. }
                | ToolResultOutcomeV1::Indeterminate { .. } => Some(ToolCallErrorClassV1::Other),
                ToolResultOutcomeV1::NotExecuted { .. } => {
                    Some(ToolCallErrorClassV1::InvalidRequest)
                }
                ToolResultOutcomeV1::Succeeded | ToolResultOutcomeV1::Cancelled { .. } => None,
            });
            let (purpose, invalid_input_issue) = match &call.input {
                NormalizedToolCallInputStateV1::Ready { purpose, .. } => {
                    (Some(purpose.as_str().to_owned()), None)
                }
                NormalizedToolCallInputStateV1::InvalidInput { issue, .. } => (None, Some(*issue)),
            };
            let display_name = names
                .get(&call.tool_call_id)
                .cloned()
                .ok_or(ToolExecutionProjectionErrorV1::AdmissionMismatch)?;
            summaries.push(ToolCallActivitySummaryView {
                tool_call_id: call.tool_call_id,
                tool_batch_id: batch.tool_batch_id,
                ordinal: call.ordinal,
                tool_id: call.definition_ref.as_ref().map(|r| r.tool_id),
                tool_version: call.definition_ref.as_ref().map(|r| r.tool_version.clone()),
                display_name,
                status,
                outcome,
                error_class,
                side_effect_state,
                started_at: None,
                completed_at,
                can_cancel: result.is_none(),
            });
            calls.insert(
                call.tool_call_id,
                ToolCallProjection {
                    tool_call_id: call.tool_call_id,
                    tool_batch_id: batch.tool_batch_id,
                    ordinal: call.ordinal,
                    definition_ref: call.definition_ref.clone(),
                    purpose,
                    invalid_input_issue,
                    status,
                    outcome,
                    side_effect_state,
                    attempt_ids: Vec::new(),
                    started_at: None,
                    completed_at,
                    safe_error,
                    model_visible_result: None,
                    user_audit_result: None,
                    can_cancel: result.is_none(),
                },
            );
        }
        let completed_call_count = u32::try_from(results.len())
            .map_err(|_| ToolExecutionProjectionErrorV1::AdmissionMismatch)?;
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
            status: ToolBatchStatusV1::Admitted,
            calls: summaries.clone(),
            started_at: None,
            completed_at: None,
            safe_error: None,
            can_cancel: !planned_ids.is_empty(),
        };
        let session = SessionToolActivityProjection {
            session_id: batch.invocation_identity.session_id,
            active_batches: vec![ToolBatchActivitySummaryView {
                tool_batch_id: batch.tool_batch_id,
                status: ToolBatchStatusV1::Admitted,
                call_count: u32::try_from(batch.calls.len())
                    .map_err(|_| ToolExecutionProjectionErrorV1::AdmissionMismatch)?,
                completed_call_count,
                started_at: None,
                completed_at: None,
                can_cancel: !planned_ids.is_empty(),
            }],
            recent_calls: summaries,
            older_activity_cursor: None,
        };
        let state = Self {
            batch: batch_projection.clone(),
            calls,
            session: session.clone(),
            attempts: BTreeMap::new(),
            results,
        };
        let set = ToolAdmissionProjectionSetV1 {
            batch: batch_projection,
            calls: state.calls_in_batch_order(batch)?,
            session,
        };
        Ok((state, set))
    }

    pub fn dispatch_prepared(
        &mut self,
        attempt: &ToolExecutionAttemptRecordV1,
    ) -> Result<ToolDispatchPreparedProjectionSetV1, ToolExecutionProjectionErrorV1> {
        attempt.validate().map_err(|_| ToolExecutionProjectionErrorV1::InvalidAttemptTransition)?;
        let call = self
            .calls
            .get_mut(&attempt.tool_call_id)
            .ok_or(ToolExecutionProjectionErrorV1::InvalidCallTransition)?;
        if attempt.status != ToolExecutionAttemptStatusV1::DispatchPrepared
            || attempt.side_effect_state != SideEffectStateV1::NotStarted
            || attempt.attempt_number != u8::try_from(call.attempt_ids.len() + 1).unwrap_or(0)
            || call.status == ToolCallStatusV1::Finalized
            || call.attempt_ids.contains(&attempt.tool_execution_attempt_id)
            || self.attempts.contains_key(&attempt.tool_execution_attempt_id)
        {
            return Err(ToolExecutionProjectionErrorV1::InvalidAttemptTransition);
        }
        let batch_started_at = self.batch.started_at.get_or_insert(attempt.started_at).to_owned();
        self.batch.status = ToolBatchStatusV1::Running;
        self.batch.can_cancel = true;
        call.status = ToolCallStatusV1::Running;
        call.started_at.get_or_insert(attempt.started_at);
        call.attempt_ids.push(attempt.tool_execution_attempt_id);
        call.can_cancel = true;

        let attempt_projection = ToolExecutionAttemptProjection {
            tool_execution_attempt_id: attempt.tool_execution_attempt_id,
            tool_call_id: attempt.tool_call_id,
            attempt_number: attempt.attempt_number,
            status: attempt.status,
            side_effect_state: attempt.side_effect_state,
            progress: None,
            started_at: attempt.started_at,
            dispatch_prepared_at: attempt.dispatch_prepared_at,
            completed_at: None,
            safe_error: None,
            user_readable_artifacts: Vec::new(),
            can_cancel: true,
        };
        self.attempts.insert(attempt.tool_execution_attempt_id, attempt_projection.clone());
        update_summary_running(
            &mut self.batch.calls,
            attempt.tool_call_id,
            call.started_at.expect("started_at was assigned"),
        )?;
        let active = self
            .session
            .active_batches
            .iter_mut()
            .find(|value| value.tool_batch_id == self.batch.tool_batch_id)
            .ok_or(ToolExecutionProjectionErrorV1::InvalidCallTransition)?;
        active.status = ToolBatchStatusV1::Running;
        active.started_at = Some(batch_started_at);
        active.can_cancel = true;
        update_summary_running(
            &mut self.session.recent_calls,
            attempt.tool_call_id,
            call.started_at.expect("started_at was assigned"),
        )?;

        Ok(ToolDispatchPreparedProjectionSetV1 {
            batch: self.batch.clone(),
            call: call.clone(),
            attempt: attempt_projection,
            session: self.session.clone(),
        })
    }

    pub fn runtime_started(
        &mut self,
        attempt: &ToolExecutionAttemptRecordV1,
    ) -> Result<ToolExecutionAttemptProjection, ToolExecutionProjectionErrorV1> {
        attempt.validate().map_err(|_| ToolExecutionProjectionErrorV1::InvalidAttemptTransition)?;
        let projection = self
            .attempts
            .get_mut(&attempt.tool_execution_attempt_id)
            .ok_or(ToolExecutionProjectionErrorV1::InvalidAttemptTransition)?;
        if attempt.status != ToolExecutionAttemptStatusV1::Running
            || attempt.side_effect_state == SideEffectStateV1::NotStarted
            || projection.status != ToolExecutionAttemptStatusV1::DispatchPrepared
            || projection.tool_call_id != attempt.tool_call_id
        {
            return Err(ToolExecutionProjectionErrorV1::InvalidAttemptTransition);
        }
        projection.status = attempt.status;
        projection.side_effect_state = attempt.side_effect_state;
        Ok(projection.clone())
    }

    /// Applies Core's recorded cancellation intent to the mutable attempt view.
    ///
    /// A `Cancelled` terminal is only accepted by persistence when the exact
    /// cancellation request was recorded first, so this is the transition that
    /// carries an attempt from `Running`/`WaitingExternal` into `Cancelling`. It
    /// does not stop anything: the runtime keeps running until the Host reports the
    /// real stop, and it does not alter the logical ToolCall or Batch lifecycle.
    pub fn cancellation_requested(
        &mut self,
        attempt: &ToolExecutionAttemptRecordV1,
    ) -> Result<ToolExecutionAttemptProjection, ToolExecutionProjectionErrorV1> {
        attempt.validate().map_err(|_| ToolExecutionProjectionErrorV1::InvalidAttemptTransition)?;
        let projection = self
            .attempts
            .get_mut(&attempt.tool_execution_attempt_id)
            .ok_or(ToolExecutionProjectionErrorV1::InvalidAttemptTransition)?;
        if attempt.status != ToolExecutionAttemptStatusV1::Cancelling
            || !matches!(
                projection.status,
                ToolExecutionAttemptStatusV1::Running
                    | ToolExecutionAttemptStatusV1::WaitingExternal
            )
            || projection.tool_call_id != attempt.tool_call_id
            || projection.attempt_number != attempt.attempt_number
            || projection.completed_at.is_some()
        {
            return Err(ToolExecutionProjectionErrorV1::InvalidAttemptTransition);
        }
        projection.status = ToolExecutionAttemptStatusV1::Cancelling;
        projection.can_cancel = false;
        Ok(projection.clone())
    }

    /// Applies one already validated Runtime progress frame to the mutable
    /// operational view. Progress is monotonic, attempt-local, and deliberately
    /// does not alter the logical ToolCall or Batch lifecycle.
    pub fn record_progress(
        &mut self,
        attempt: &mut ToolExecutionAttemptRecordV1,
        progress: ToolAttemptProgressView,
    ) -> Result<ToolExecutionAttemptProjection, ToolExecutionProjectionErrorV1> {
        let projection = self
            .attempts
            .get_mut(&attempt.tool_execution_attempt_id)
            .ok_or(ToolExecutionProjectionErrorV1::InvalidAttemptTransition)?;
        if !matches!(
            attempt.status,
            ToolExecutionAttemptStatusV1::Running
                | ToolExecutionAttemptStatusV1::WaitingExternal
                | ToolExecutionAttemptStatusV1::Cancelling
        ) || projection.status != attempt.status
            || projection.tool_call_id != attempt.tool_call_id
            || projection.attempt_number != attempt.attempt_number
            || progress.sequence <= attempt.last_progress_sequence
            || progress.percentage.is_some_and(|percentage| percentage > 100)
            || progress.updated_at < attempt.started_at
            || projection.completed_at.is_some()
        {
            return Err(ToolExecutionProjectionErrorV1::InvalidAttemptTransition);
        }
        attempt.last_progress_sequence = progress.sequence;
        projection.progress = Some(progress);
        Ok(projection.clone())
    }

    /// Closes one failed Attempt while leaving its logical ToolCall and Batch
    /// running so a separately admitted retry may acquire fresh resources and
    /// create a new `tea_`. The stable error is attempt-local; no ToolResult is
    /// exposed until the logical call reaches its single terminal outcome.
    pub fn failed_for_retry(
        &mut self,
        attempt: &ToolExecutionAttemptRecordV1,
        error: &ErrorEnvelope,
    ) -> Result<ToolExecutionAttemptProjection, ToolExecutionProjectionErrorV1> {
        attempt.validate().map_err(|_| ToolExecutionProjectionErrorV1::InvalidAttemptTransition)?;
        let projection = self
            .attempts
            .get_mut(&attempt.tool_execution_attempt_id)
            .ok_or(ToolExecutionProjectionErrorV1::InvalidAttemptTransition)?;
        if attempt.status != ToolExecutionAttemptStatusV1::Failed
            || !matches!(
                projection.status,
                ToolExecutionAttemptStatusV1::Running
                    | ToolExecutionAttemptStatusV1::WaitingExternal
                    | ToolExecutionAttemptStatusV1::Cancelling
            )
            || projection.tool_call_id != attempt.tool_call_id
            || projection.attempt_number != attempt.attempt_number
            || projection.progress.as_ref().map_or(0, |progress| progress.sequence)
                != attempt.last_progress_sequence
        {
            return Err(ToolExecutionProjectionErrorV1::InvalidAttemptTransition);
        }
        projection.status = attempt.status;
        projection.side_effect_state = attempt.side_effect_state;
        projection.completed_at = attempt.completed_at;
        projection.safe_error = Some(error_view(error));
        projection.can_cancel = false;
        Ok(projection.clone())
    }

    pub fn finalize_attempt(
        &mut self,
        attempt: &ToolExecutionAttemptRecordV1,
        result: ToolResultV1,
        model_visible_result: Option<UserReadableToolArtifactView>,
        user_audit_result: Option<UserReadableToolArtifactView>,
    ) -> Result<ToolAttemptFinalizationProjectionSetV1, ToolExecutionProjectionErrorV1> {
        attempt.validate().map_err(|_| ToolExecutionProjectionErrorV1::InvalidAttemptTransition)?;
        result.validate().map_err(|_| ToolExecutionProjectionErrorV1::InvalidResult)?;
        if !attempt.status.is_terminal()
            || result.tool_call_id != attempt.tool_call_id
            || result.attempts.last().is_none_or(|value| {
                value.tool_execution_attempt_id != attempt.tool_execution_attempt_id
                    || value.attempt_number != attempt.attempt_number
                    || value.side_effect_state != attempt.side_effect_state
                    || value.side_effect_evidence_ref != attempt.evidence_ref
                    || !value.dispatched
            })
        {
            return Err(ToolExecutionProjectionErrorV1::InvalidResult);
        }
        let projection = self
            .attempts
            .get_mut(&attempt.tool_execution_attempt_id)
            .ok_or(ToolExecutionProjectionErrorV1::InvalidAttemptTransition)?;
        if !matches!(
            projection.status,
            ToolExecutionAttemptStatusV1::Running
                | ToolExecutionAttemptStatusV1::WaitingExternal
                | ToolExecutionAttemptStatusV1::Cancelling
        ) {
            return Err(ToolExecutionProjectionErrorV1::InvalidAttemptTransition);
        }
        projection.status = attempt.status;
        projection.side_effect_state = attempt.side_effect_state;
        projection.completed_at = attempt.completed_at;
        projection.safe_error = result_safe_error(&result);
        projection.can_cancel = false;
        let attempt_projection = projection.clone();

        let call = self
            .calls
            .get_mut(&attempt.tool_call_id)
            .ok_or(ToolExecutionProjectionErrorV1::InvalidCallTransition)?;
        if call.attempt_ids
            != result
                .attempts
                .iter()
                .map(|value| value.tool_execution_attempt_id)
                .collect::<Vec<_>>()
        {
            return Err(ToolExecutionProjectionErrorV1::InvalidResult);
        }
        call.status = ToolCallStatusV1::Finalized;
        call.outcome = Some(outcome_summary(&result.outcome));
        call.side_effect_state = Some(result.side_effect_state);
        call.started_at = result.started_at;
        call.completed_at = Some(result.completed_at);
        call.safe_error = result_safe_error(&result);
        call.model_visible_result = model_visible_result;
        call.user_audit_result = user_audit_result;
        call.can_cancel = false;
        let call_projection = call.clone();
        update_summary_finalized(&mut self.batch.calls, &result, call_projection.started_at)?;
        update_summary_finalized(
            &mut self.session.recent_calls,
            &result,
            call_projection.started_at,
        )?;
        self.results.insert(result.tool_call_id, result.clone());

        let all_complete = self.results.len() == self.calls.len();
        let batch_result = if all_complete {
            let results = self
                .batch
                .calls
                .iter()
                .map(|summary| {
                    self.results
                        .get(&summary.tool_call_id)
                        .cloned()
                        .ok_or(ToolExecutionProjectionErrorV1::InvalidResult)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let completed_at = results
                .iter()
                .map(|value| value.completed_at)
                .max()
                .ok_or(ToolExecutionProjectionErrorV1::InvalidResult)?;
            self.batch.status = ToolBatchStatusV1::Completed;
            self.batch.completed_at = Some(completed_at);
            self.batch.can_cancel = false;
            self.session
                .active_batches
                .retain(|value| value.tool_batch_id != self.batch.tool_batch_id);
            Some(ToolBatchResultV1 {
                tool_batch_id: self.batch.tool_batch_id,
                status: ToolBatchStatusV1::Completed,
                results,
                completed_at,
            })
        } else {
            let active = self
                .session
                .active_batches
                .iter_mut()
                .find(|value| value.tool_batch_id == self.batch.tool_batch_id)
                .ok_or(ToolExecutionProjectionErrorV1::InvalidCallTransition)?;
            active.status = ToolBatchStatusV1::Running;
            active.completed_call_count = u32::try_from(self.results.len())
                .map_err(|_| ToolExecutionProjectionErrorV1::InvalidResult)?;
            None
        };
        if let Some(batch_result) = batch_result.as_ref() {
            batch_result
                .results
                .iter()
                .all(|value| self.results.contains_key(&value.tool_call_id))
                .then_some(())
                .ok_or(ToolExecutionProjectionErrorV1::InvalidResult)?;
        }
        Ok(ToolAttemptFinalizationProjectionSetV1 {
            batch: self.batch.clone(),
            call: call_projection,
            attempt: attempt_projection,
            session: self.session.clone(),
            batch_result,
        })
    }

    fn calls_in_batch_order(
        &self,
        batch: &ToolBatchRequestV1,
    ) -> Result<Vec<ToolCallProjection>, ToolExecutionProjectionErrorV1> {
        batch
            .calls
            .iter()
            .map(|call| {
                self.calls
                    .get(&call.tool_call_id)
                    .cloned()
                    .ok_or(ToolExecutionProjectionErrorV1::AdmissionMismatch)
            })
            .collect()
    }
}

fn update_summary_running(
    summaries: &mut [ToolCallActivitySummaryView],
    tool_call_id: ToolCallId,
    started_at: UtcTimestamp,
) -> Result<(), ToolExecutionProjectionErrorV1> {
    let summary = summaries
        .iter_mut()
        .find(|value| value.tool_call_id == tool_call_id)
        .ok_or(ToolExecutionProjectionErrorV1::InvalidCallTransition)?;
    summary.status = ToolCallStatusV1::Running;
    summary.started_at = Some(started_at);
    summary.can_cancel = true;
    Ok(())
}

fn update_summary_finalized(
    summaries: &mut [ToolCallActivitySummaryView],
    result: &ToolResultV1,
    started_at: Option<UtcTimestamp>,
) -> Result<(), ToolExecutionProjectionErrorV1> {
    let summary = summaries
        .iter_mut()
        .find(|value| value.tool_call_id == result.tool_call_id)
        .ok_or(ToolExecutionProjectionErrorV1::InvalidCallTransition)?;
    summary.status = ToolCallStatusV1::Finalized;
    summary.outcome = Some(outcome_summary(&result.outcome));
    summary.side_effect_state = Some(result.side_effect_state);
    summary.started_at = started_at;
    summary.completed_at = Some(result.completed_at);
    summary.can_cancel = false;
    Ok(())
}

fn result_safe_error(result: &ToolResultV1) -> Option<ToolProjectionErrorView> {
    match &result.outcome {
        ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::Indeterminate { error, .. }
        | ToolResultOutcomeV1::NotExecuted { error: Some(error), .. } => Some(error_view(error)),
        _ => None,
    }
}

fn error_view(error: &ErrorEnvelope) -> ToolProjectionErrorView {
    ToolProjectionErrorView {
        code: error.code.clone(),
        category: error.category,
        message: error.message.clone(),
        retryable: error.retryable,
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
    use aura_domain_common::{
        AgentRunId, ArtifactId, CommandInvocationId, CorrelationId, ErrorCategory, ErrorCode,
        ErrorEnvelope, ModelStepId, Revision, SessionId, TurnTaskId, UtcTimestamp,
    };
    use aura_domain_common::{
        ToolBatchId, ToolCallId, ToolExecutionAttemptId, ToolId, ToolRegistrySnapshotId,
    };
    use aura_execution_contract::SideEffectStateV1;
    use aura_tool_contract::{
        CoreInvocationIdentityV1, NormalizedToolCallV1, ToolDefinitionSnapshotRef,
        ToolExecutionAttemptOutcomeRefV1, ToolExecutionAttemptRecordV1,
        ToolExecutionAttemptStatusV1, ToolExecutionPlanCallV1, ToolExecutionPlanV1,
        ToolNotExecutedReasonV1, ToolPurposeTextV1, ToolRegistrySnapshotRef, ToolResultOutcomeV1,
        ToolResultV1, ToolVersion,
    };
    use serde_json::json;

    use super::ToolExecutionProjectionStateV1;

    fn timestamp(value: &str) -> UtcTimestamp {
        UtcTimestamp::from_str(value).expect("fixture timestamp")
    }

    fn fixture() -> (aura_tool_contract::ToolBatchRequestV1, ToolExecutionPlanV1, ToolResultV1) {
        let registry = ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::from_bytes([1; 16]),
            registry_revision: Revision::INITIAL,
            content_hash: CanonicalHashV1::sha256(b"registry"),
        };
        let version = ToolVersion::new(None, Revision::INITIAL).expect("version");
        let ready_id = ToolCallId::from_bytes([2; 16]);
        let rejected_id = ToolCallId::from_bytes([3; 16]);
        let ready_arguments = json!({"value": "ready"});
        let calls = vec![
            NormalizedToolCallV1 {
                tool_call_id: ready_id,
                ordinal: 1,
                definition_ref: Some(ToolDefinitionSnapshotRef {
                    tool_id: ToolId::from_bytes([4; 16]),
                    tool_version: version.clone(),
                    registry_snapshot_ref: registry.clone(),
                }),
                input: aura_tool_contract::NormalizedToolCallInputStateV1::Ready {
                    purpose: ToolPurposeTextV1::new("Run fixture").expect("purpose"),
                    arguments_hash: canonical_sha256(&ready_arguments).expect("arguments hash"),
                    arguments: ready_arguments,
                },
                provider_arguments_envelope_hash: CanonicalHashV1::sha256(b"ready envelope"),
            },
            NormalizedToolCallV1 {
                tool_call_id: rejected_id,
                ordinal: 2,
                definition_ref: Some(ToolDefinitionSnapshotRef {
                    tool_id: ToolId::from_bytes([5; 16]),
                    tool_version: version,
                    registry_snapshot_ref: registry.clone(),
                }),
                input: aura_tool_contract::NormalizedToolCallInputStateV1::InvalidInput {
                    issue: aura_tool_contract::NormalizedToolCallInputIssueV1::PurposeMissing,
                    detail: None,
                },
                provider_arguments_envelope_hash: CanonicalHashV1::sha256(b"bad envelope"),
            },
        ];
        let batch = aura_tool_contract::ToolBatchRequestV1::new(
            ToolBatchId::from_bytes([6; 16]),
            CoreInvocationIdentityV1 {
                command_invocation_id: CommandInvocationId::from_bytes([7; 16]),
                session_id: SessionId::from_bytes([8; 16]),
                turn_task_id: TurnTaskId::from_bytes([9; 16]),
                agent_run_id: AgentRunId::from_bytes([10; 16]),
                model_step_id: ModelStepId::from_bytes([11; 16]),
            },
            Revision::INITIAL,
            CanonicalHashV1::sha256(b"control"),
            registry.clone(),
            calls,
        )
        .expect("batch");
        let plan = ToolExecutionPlanV1::new(
            batch.tool_batch_id,
            registry,
            Revision::INITIAL,
            CanonicalHashV1::sha256(b"settings"),
            vec![ToolExecutionPlanCallV1 {
                tool_call_id: ready_id,
                ordinal: 1,
                dependency_tool_call_ids: Vec::new(),
                execution_atomic_group: None,
                resolved_resource_claims: Vec::new(),
                timeout_ms: 1_000,
                max_retries: 0,
                local_concurrency_limit: None,
                entity_revision: Revision::INITIAL,
            }],
        )
        .expect("plan");
        let rejected = ToolResultV1 {
            tool_call_id: rejected_id,
            outcome: ToolResultOutcomeV1::NotExecuted {
                reason: ToolNotExecutedReasonV1::PurposeRequired,
                error: None,
            },
            side_effect_state: SideEffectStateV1::NotStarted,
            attempts: Vec::new(),
            model_visible_result_ref: None,
            user_audit_result_ref: None,
            internal_diagnostic_ref: None,
            aggregate_side_effect_evidence_ref: None,
            started_at: None,
            completed_at: timestamp("2026-09-05T00:00:00Z"),
        };
        (batch, plan, rejected)
    }

    #[test]
    fn projection_state_drives_admission_dispatch_and_terminal_views_together() {
        let (batch, plan, rejected) = fixture();
        let ready_id = batch.calls[0].tool_call_id;
        let (mut state, admitted) = ToolExecutionProjectionStateV1::admitted(
            &batch,
            &plan,
            &[
                (ready_id, "Fixture".to_owned()),
                (batch.calls[1].tool_call_id, "Rejected".to_owned()),
            ],
            std::slice::from_ref(&rejected),
        )
        .expect("admission projections");
        assert_eq!(admitted.calls[0].status, aura_tool_contract::ToolCallStatusV1::Queued);
        assert_eq!(admitted.calls[1].completed_at, Some(rejected.completed_at));

        let attempt_id = ToolExecutionAttemptId::from_bytes([12; 16]);
        let mut attempt = ToolExecutionAttemptRecordV1 {
            tool_execution_attempt_id: attempt_id,
            tool_call_id: ready_id,
            attempt_number: 1,
            status: ToolExecutionAttemptStatusV1::DispatchPrepared,
            side_effect_state: SideEffectStateV1::NotStarted,
            evidence_ref: None,
            started_at: timestamp("2026-09-05T00:00:01Z"),
            dispatch_prepared_at: Some(timestamp("2026-09-05T00:00:02Z")),
            completed_at: None,
            last_progress_sequence: 0,
        };
        let dispatch = state.dispatch_prepared(&attempt).expect("dispatch projections");
        assert_eq!(dispatch.call.attempt_ids, vec![attempt_id]);
        assert_eq!(dispatch.batch.status, aura_tool_contract::ToolBatchStatusV1::Running);

        attempt.status = ToolExecutionAttemptStatusV1::Running;
        attempt.side_effect_state = SideEffectStateV1::Unknown;
        let running = state.runtime_started(&attempt).expect("running projection");
        assert_eq!(running.side_effect_state, SideEffectStateV1::Unknown);
        let progress = state
            .record_progress(
                &mut attempt,
                crate::ToolAttemptProgressView {
                    sequence: 2,
                    percentage: Some(50),
                    message: Some("fixture running".to_owned()),
                    updated_at: timestamp("2026-09-05T00:00:02Z"),
                },
            )
            .expect("progress projection");
        assert_eq!(attempt.last_progress_sequence, 2);
        assert_eq!(progress.progress.expect("progress").sequence, 2);

        let attempt_evidence = ArtifactId::from_bytes([13; 16]);
        attempt.status = ToolExecutionAttemptStatusV1::Succeeded;
        attempt.side_effect_state = SideEffectStateV1::NoEffect;
        attempt.evidence_ref = Some(attempt_evidence);
        attempt.completed_at = Some(timestamp("2026-09-05T00:00:03Z"));
        let result = ToolResultV1 {
            tool_call_id: ready_id,
            outcome: ToolResultOutcomeV1::Succeeded,
            side_effect_state: SideEffectStateV1::NoEffect,
            attempts: vec![ToolExecutionAttemptOutcomeRefV1 {
                tool_execution_attempt_id: attempt_id,
                attempt_number: 1,
                side_effect_state: SideEffectStateV1::NoEffect,
                side_effect_evidence_ref: Some(attempt_evidence),
                dispatched: true,
            }],
            model_visible_result_ref: None,
            user_audit_result_ref: None,
            internal_diagnostic_ref: None,
            aggregate_side_effect_evidence_ref: Some(ArtifactId::from_bytes([14; 16])),
            started_at: Some(attempt.started_at),
            completed_at: attempt.completed_at.expect("completed"),
        };
        let terminal =
            state.finalize_attempt(&attempt, result, None, None).expect("terminal projections");
        assert_eq!(terminal.batch.status, aura_tool_contract::ToolBatchStatusV1::Completed);
        assert_eq!(terminal.batch_result.expect("batch result").results.len(), 2);
        assert!(terminal.session.active_batches.is_empty());
    }

    #[test]
    fn retryable_failure_closes_only_the_attempt_and_allows_the_next_attempt() {
        let (batch, plan, rejected) = fixture();
        let ready_id = batch.calls[0].tool_call_id;
        let (mut state, _) = ToolExecutionProjectionStateV1::admitted(
            &batch,
            &plan,
            &[
                (ready_id, "Fixture".to_owned()),
                (batch.calls[1].tool_call_id, "Rejected".to_owned()),
            ],
            &[rejected],
        )
        .expect("admission projections");
        let mut first = ToolExecutionAttemptRecordV1 {
            tool_execution_attempt_id: ToolExecutionAttemptId::from_bytes([21; 16]),
            tool_call_id: ready_id,
            attempt_number: 1,
            status: ToolExecutionAttemptStatusV1::DispatchPrepared,
            side_effect_state: SideEffectStateV1::NotStarted,
            evidence_ref: None,
            started_at: timestamp("2026-09-05T00:01:00Z"),
            dispatch_prepared_at: Some(timestamp("2026-09-05T00:01:01Z")),
            completed_at: None,
            last_progress_sequence: 0,
        };
        state.dispatch_prepared(&first).expect("first dispatch");
        first.status = ToolExecutionAttemptStatusV1::Running;
        first.side_effect_state = SideEffectStateV1::Unknown;
        state.runtime_started(&first).expect("first running");
        first.status = ToolExecutionAttemptStatusV1::Failed;
        first.side_effect_state = SideEffectStateV1::NoEffect;
        first.evidence_ref = Some(ArtifactId::from_bytes([22; 16]));
        first.completed_at = Some(timestamp("2026-09-05T00:01:02Z"));
        let error = ErrorEnvelope::new(
            ErrorCode::new("tool.runtime.dispatch_failed").expect("error code"),
            ErrorCategory::Unavailable,
            "The fixture failed and can be retried.",
            true,
            CorrelationId::from_bytes([23; 16]),
            None,
        );
        let failed = state.failed_for_retry(&first, &error).expect("retryable failure");
        assert_eq!(failed.status, ToolExecutionAttemptStatusV1::Failed);
        assert!(!failed.can_cancel);

        let second = ToolExecutionAttemptRecordV1 {
            tool_execution_attempt_id: ToolExecutionAttemptId::from_bytes([24; 16]),
            tool_call_id: ready_id,
            attempt_number: 2,
            status: ToolExecutionAttemptStatusV1::DispatchPrepared,
            side_effect_state: SideEffectStateV1::NotStarted,
            evidence_ref: None,
            started_at: timestamp("2026-09-05T00:01:03Z"),
            dispatch_prepared_at: Some(timestamp("2026-09-05T00:01:04Z")),
            completed_at: None,
            last_progress_sequence: 0,
        };
        let retry = state.dispatch_prepared(&second).expect("second dispatch");
        assert_eq!(
            retry.call.attempt_ids,
            vec![first.tool_execution_attempt_id, second.tool_execution_attempt_id]
        );
    }

    #[test]
    fn projection_state_rejects_duplicate_or_incomplete_display_name_authority() {
        let (batch, plan, rejected) = fixture();
        let result = ToolExecutionProjectionStateV1::admitted(
            &batch,
            &plan,
            &[(batch.calls[0].tool_call_id, "Only one".to_owned())],
            &[rejected],
        );
        assert!(result.is_err());
    }
}
