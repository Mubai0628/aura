//! Execution facts in the existing V2 Tool-owned tables. No second current
//! Registry/mode authority and no legacy execution/result dual write.
use aura_domain_common::{
    SecurityOperationId, ToolBatchId, ToolCallId, ToolExecutionAttemptId, ToolId, TurnTaskId,
    UtcTimestamp,
};
use aura_execution_contract::{AttemptState, AuthorizedDispatch, ExecutionOutcome};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, event, record};
use aura_tool_contract::*;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchExecution {
    pub batch: ToolBatchRequestV1,
    pub plan: ToolExecutionPlanV1,
    pub admitted_at: UtcTimestamp,
    pub result: Option<ToolBatchResultV1>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallExecution {
    pub call: NormalizedToolCallV1,
    pub attempt_id: Option<ToolExecutionAttemptId>,
    pub result: Option<ToolResultV1>,
}
pub fn batch(connection: &Connection, id: ToolBatchId) -> StoreResult<BatchExecution> {
    let text: String = connection.query_row(
        "SELECT payload FROM tool_batches WHERE id=?1",
        [id.to_string()],
        |r| r.get(0),
    )?;
    let value: BatchExecution = record::decode(&text)?;
    value.batch.validate_hash().map_err(|_| Error::Invalid("Tool batch hash"))?;
    value.plan.validate().map_err(|_| Error::Invalid("Tool plan hash"))?;
    if let Some(result) = &value.result {
        result.validate(&value.batch).map_err(|_| Error::Invalid("Tool batch result"))?;
    }
    Ok(value)
}
pub fn call(connection: &Connection, id: ToolCallId) -> StoreResult<CallExecution> {
    let text: String = connection.query_row(
        "SELECT payload FROM tool_calls WHERE id=?1",
        [id.to_string()],
        |r| r.get(0),
    )?;
    let value: CallExecution = record::decode(&text)?;
    if value.call.tool_call_id != id {
        return Err(Error::Conflict);
    }
    if let Some(result) = &value.result {
        result.validate().map_err(|_| Error::Invalid("Tool result"))?;
    }
    Ok(value)
}
/// Calls of one tool in a turn that ended without running: refused while planning or
/// failed. A call still in flight has no result yet and is not counted.
pub fn unsuccessful_in_turn(
    connection: &Connection,
    turn: TurnTaskId,
    tool: ToolId,
) -> StoreResult<usize> {
    let mut query = connection.prepare(concat!(
        "SELECT c.payload FROM tool_calls c JOIN tool_batches b ON b.id=c.batch_id ",
        "WHERE json_extract(b.payload,'$.batch.invocation_identity.turn_task_id')=?1 ",
        "AND json_extract(c.payload,'$.call.definition_ref.tool_id')=?2"
    ))?;
    let rows = query.query_map([turn.to_string(), tool.to_string()], |r| r.get::<_, String>(0))?;
    let mut count = 0;
    for row in rows {
        let value: CallExecution = record::decode(&row?)?;
        if value.result.is_some_and(|result| {
            matches!(
                result.outcome,
                ToolResultOutcomeV1::Failed { .. }
                    | ToolResultOutcomeV1::NotExecuted {
                        reason: ToolNotExecutedReasonV1::ResourceClaimResolutionFailed,
                        ..
                    }
            )
        }) {
            count += 1;
        }
    }
    Ok(count)
}
/// Identity admission and resource planning finish before this transaction.
/// Repeated identical admission returns false; it never dispatches again.
pub fn admit(
    tx: &Transaction<'_>,
    value: &BatchExecution,
    registry: &ToolRegistrySnapshotV1,
    preflight: &[ToolResultV1],
) -> StoreResult<bool> {
    value.batch.validate_hash().map_err(|_| Error::Invalid("Tool batch hash"))?;
    value.plan.validate().map_err(|_| Error::Invalid("Tool plan hash"))?;
    if value.result.is_some()
        || value.batch.registry_snapshot_ref != registry.snapshot_ref
        || value.plan.registry_snapshot_ref != registry.snapshot_ref
        || value.plan.tool_batch_id != value.batch.tool_batch_id
    {
        return Err(Error::Conflict);
    }
    let previous: Option<String> = tx
        .query_row(
            "SELECT id FROM tool_batches WHERE id=?1",
            [value.batch.tool_batch_id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if previous.is_some() {
        let previous = batch(tx, value.batch.tool_batch_id)?;
        return if previous.batch == value.batch && previous.plan == value.plan {
            Ok(false)
        } else {
            Err(Error::Conflict)
        };
    }
    let stored = super::registry(tx, registry.snapshot_ref.tool_registry_snapshot_id)?;
    if let Some(stored) = stored {
        if *stored != *registry {
            return Err(Error::Conflict);
        }
    } else {
        super::insert_registry(tx, registry)?;
    }
    let planned =
        value.plan.calls.iter().map(|c| c.tool_call_id).collect::<std::collections::BTreeSet<_>>();
    let mut rejected = std::collections::BTreeMap::new();
    for result in preflight {
        result.validate().map_err(|_| Error::Invalid("Tool preflight result"))?;
        if !matches!(result.outcome, ToolResultOutcomeV1::NotExecuted { .. })
            || planned.contains(&result.tool_call_id)
            || rejected.insert(result.tool_call_id, result).is_some()
        {
            return Err(Error::Conflict);
        }
    }
    if planned.len() + rejected.len() != value.batch.calls.len() {
        return Err(Error::Conflict);
    }
    super::insert_batch(
        tx,
        value.batch.tool_batch_id,
        value.batch.invocation_identity.session_id,
        registry.snapshot_ref.tool_registry_snapshot_id,
        &serde_json::to_value(value)?,
    )?;
    for (position, call) in value.batch.calls.iter().enumerate() {
        if !planned.contains(&call.tool_call_id) && !rejected.contains_key(&call.tool_call_id) {
            return Err(Error::Conflict);
        }
        let record = CallExecution { call: call.clone(), attempt_id: None, result: None };
        tx.execute(
            "INSERT INTO tool_calls VALUES(?1,?2,?3,NULL,?4)",
            params![
                call.tool_call_id.to_string(),
                value.batch.tool_batch_id.to_string(),
                // SQL row order is one-based. The Provider's frozen ordinal
                // stays unchanged in the normalized call and history payload.
                i64::try_from(position + 1).map_err(|_| Error::Conflict)?,
                record::encode(&record)?
            ],
        )?;
        if let Some(result) = rejected.get(&call.tool_call_id) {
            let result = super::result::refusal_artifact(tx, &value.batch, call, result)?;
            complete_call(tx, value.batch.tool_batch_id, &result)?;
        }
    }
    publish(
        tx,
        value.batch.tool_batch_id,
        "tool.batch_admitted",
        &serde_json::to_value(value)?,
        value.admitted_at,
    )?;
    Ok(true)
}
pub struct CallReservation {
    pub batch: ToolBatchRequestV1,
    pub call_id: ToolCallId,
    pub attempt_id: ToolExecutionAttemptId,
}
/// Called inside the same transaction as Operation/authority/Attempt creation.
pub fn bind_attempt(
    tx: &Transaction<'_>,
    binding: &CallReservation,
    dispatch: &AuthorizedDispatch,
) -> StoreResult<()> {
    let stored = batch(tx, binding.batch.tool_batch_id)?;
    let mut call = call(tx, binding.call_id)?;
    let identity = &dispatch.request().operation.content.identity;
    if stored.batch != binding.batch
        || stored.result.is_some()
        || call.result.is_some()
        || call.attempt_id.is_some()
        || !stored.plan.calls.iter().any(|v| v.tool_call_id == binding.call_id)
        || !stored.batch.calls.contains(&call.call)
        || !identity.requires_model_purpose()
        || identity.session_id != Some(binding.batch.invocation_identity.session_id)
        || identity.turn_task_id != Some(binding.batch.invocation_identity.turn_task_id)
        || identity.agent_run_id != Some(binding.batch.invocation_identity.agent_run_id)
    {
        return Err(Error::Conflict);
    }
    let attempt = &dispatch.request().binding.attempt;
    let current = crate::execution::load(tx, attempt.operation_id, attempt.ordinal.get())?;
    if current.state != AttemptState::Prepared || current.binding != dispatch.request().binding {
        return Err(Error::Conflict);
    }
    call.attempt_id = Some(binding.attempt_id);
    record::changed(tx.execute("UPDATE tool_calls SET operation_id=?2,payload=?3 WHERE id=?1 AND operation_id IS NULL AND json_extract(payload,'$.result') IS NULL",
        params![binding.call_id.to_string(), attempt.operation_id.to_string(), record::encode(&call)?])?)
}
/// Returns only an existing admitted binding. Standalone trusted Core requests
/// have no Tool binding and must not acquire a fabricated Agent identity.
pub fn operation_call(
    db: &Connection,
    operation: SecurityOperationId,
) -> StoreResult<Option<(ToolBatchRequestV1, ToolCallId)>> {
    let binding: Option<(String, String)> = db
        .query_row(
            "SELECT batch_id,id FROM tool_calls WHERE operation_id=?1",
            [operation.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    binding
        .map(|(batch_id, call_id)| {
            Ok((
                batch(db, batch_id.parse().map_err(|_| Error::Conflict)?)?.batch,
                call_id.parse().map_err(|_| Error::Conflict)?,
            ))
        })
        .transpose()
}
/// Internal diagnostic only. Model history uses the separately validated Tool
/// result, never this error text or a full process environment/command dump.
pub fn record_dispatch_error(
    tx: &Transaction<'_>,
    id: ToolBatchId,
    calls: &[ToolCallId],
    error: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let stored = batch(tx, id)?;
    if calls.iter().any(|id| !stored.batch.calls.iter().any(|c| c.tool_call_id == *id)) {
        return Err(Error::Conflict);
    }
    publish(
        tx,
        id,
        "tool.dispatch_adapter_error",
        &serde_json::json!({"calls":calls,"message":error.chars().take(2048).collect::<String>()}),
        at,
    )
}
pub fn complete_call(
    tx: &Transaction<'_>,
    batch_id: ToolBatchId,
    result: &ToolResultV1,
) -> StoreResult<()> {
    result.validate().map_err(|_| Error::Invalid("Tool result"))?;
    let stored = batch(tx, batch_id)?;
    let mut call = call(tx, result.tool_call_id)?;
    if !stored.batch.calls.contains(&call.call) {
        return Err(Error::Conflict);
    }
    if let Some(previous) = call.result {
        return if previous == *result { Ok(()) } else { Err(Error::Conflict) };
    }
    if stored.result.is_some() {
        return Err(Error::Conflict);
    }
    if call.attempt_id.map_or(!result.attempts.is_empty(), |id| {
        result.attempts.len() != 1 || result.attempts[0].tool_execution_attempt_id != id
    }) {
        return Err(Error::Conflict);
    }
    if call.attempt_id.is_some() {
        let operation: String = tx.query_row(
            "SELECT operation_id FROM tool_calls WHERE id=?1",
            [result.tool_call_id.to_string()],
            |r| r.get(0),
        )?;
        let operation = operation.parse().map_err(|_| Error::Invalid("Tool operation ID"))?;
        let execution = crate::execution::load(tx, operation, 1)?;
        let AttemptState::Terminal { result: evidence } = &execution.state else {
            return Err(Error::Conflict);
        };
        let reference = &result.attempts[0];
        let mut outcome_matches = matches!(
            (&result.outcome, evidence.outcome),
            (ToolResultOutcomeV1::OperationRejected { .. }, ExecutionOutcome::Failed)
                | (ToolResultOutcomeV1::Succeeded, ExecutionOutcome::Succeeded)
                | (ToolResultOutcomeV1::Failed { .. }, ExecutionOutcome::Failed)
                | (ToolResultOutcomeV1::Cancelled { .. }, ExecutionOutcome::Cancelled)
                | (
                    ToolResultOutcomeV1::Cancelled { reason: ToolCancellationReasonV1::Timeout },
                    ExecutionOutcome::TimedOut
                )
                | (ToolResultOutcomeV1::Indeterminate { .. }, ExecutionOutcome::Unknown)
        );
        if evidence.outcome == ExecutionOutcome::Succeeded
            && matches!(&result.outcome, ToolResultOutcomeV1::Failed { error } if error.code == ToolResultErrorCode::SchemaValidationFailed.error_code())
        {
            let (_, output) = super::result::execution_output(tx, result.tool_call_id)?;
            outcome_matches = !super::result::output_valid(tx, result.tool_call_id, &output)?;
        }
        if !outcome_matches
            || reference.dispatched != execution.dispatched_at.is_some()
            || reference.side_effect_state != evidence.side_effect
            || result.side_effect_state != evidence.side_effect
            || reference.side_effect_evidence_ref != Some(evidence.receipt.evidence_ref)
            || result.aggregate_side_effect_evidence_ref != Some(evidence.receipt.evidence_ref)
            || result.completed_at < evidence.completed_at
        {
            return Err(Error::Conflict);
        }
    }
    // A Tool result may only expose artifacts deliberately attached to this
    // call. An ID from another operation/session cannot become model content.
    for artifact in [
        result.model_visible_result_ref,
        result.user_audit_result_ref,
        result.internal_diagnostic_ref,
    ]
    .into_iter()
    .flatten()
    {
        aura_persistence_core::artifact::read(tx, artifact, &result.tool_call_id.to_string())?;
    }
    call.result = Some(result.clone());
    record::changed(tx.execute(
        "UPDATE tool_calls SET payload=?2 WHERE id=?1 AND json_extract(payload,'$.result') IS NULL",
        params![result.tool_call_id.to_string(), record::encode(&call)?],
    )?)?;
    publish(
        tx,
        batch_id,
        "tool.call_completed",
        &serde_json::to_value(result)?,
        result.completed_at,
    )
}
/// Only consumes this committed whole group; partial result order is never
/// projected as a synthetic assistant/user conversation.
pub fn finish(
    tx: &Transaction<'_>,
    id: ToolBatchId,
    at: UtcTimestamp,
) -> StoreResult<ToolBatchResultV1> {
    let mut stored = batch(tx, id)?;
    if let Some(result) = stored.result {
        return Ok(result);
    }
    let results = stored
        .batch
        .calls
        .iter()
        .map(|c| call(tx, c.tool_call_id)?.result.ok_or(Error::Conflict))
        .collect::<StoreResult<Vec<_>>>()?;
    let result = ToolBatchResultV1 {
        tool_batch_id: id,
        status: ToolBatchStatusV1::Completed,
        results,
        completed_at: at,
    };
    result.validate(&stored.batch).map_err(|_| Error::Conflict)?;
    stored.result = Some(result.clone());
    record::changed(tx.execute("UPDATE tool_batches SET state='completed',revision=revision+1,payload=?2 WHERE id=?1 AND state<>'completed' AND json_extract(payload,'$.result') IS NULL",params![id.to_string(), record::encode(&stored)?])?)?;
    publish(tx, id, "tool.batch_completed", &serde_json::to_value(&result)?, at)?;
    Ok(result)
}
fn publish(
    tx: &Transaction<'_>,
    id: ToolBatchId,
    kind: &str,
    payload: &serde_json::Value,
    at: UtcTimestamp,
) -> StoreResult<()> {
    event::append(
        tx,
        &event::EventDraft {
            id: aura_domain_common::EventId::new(),
            owner: &id.to_string(),
            kind,
            version: 1,
            payload,
            at,
        },
        "tool",
    )?;
    Ok(())
}
