//! Agent model transport facts; configuration remains models.json authority.
//! Each write verifies the real Step/Snapshot before changing a transport fact.
use super::*;
use crate::agent::{history, turns};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_persistence_core::artifact;

pub fn owning_step(db: &Connection, id: ModelStepId) -> StoreResult<ModelStepId> {
    if super::memory_maintenance::authorize_model(db, id)? {
        return Ok(id);
    }
    if crate::context::compaction::authorize_model(db, id)? {
        return Ok(id);
    }
    let step = turns::step(db, id)?;
    let run = turns::load_run(db, step.agent_run_id)?;
    if run.state != (AgentRunState::ModelRunning { model_step_id: id })
        || !matches!(step.state, ModelStepState::Running { .. })
        || turns::load(db, run.turn_task_id)?.state.terminal()
    {
        return Err(Error::Conflict);
    }
    Ok(id)
}
pub fn put_artifact(
    tx: &Transaction<'_>,
    step: ModelStepId,
    media: &str,
    bytes: &[u8],
    at: UtcTimestamp,
) -> StoreResult<ArtifactId> {
    owning_step(tx, step)?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(Error::Invalid("model artifact too large"));
    }
    let id = ArtifactId::new();
    artifact::put(tx, id, &step.to_string(), media, bytes, at)?;
    Ok(id)
}
pub fn attempt(db: &Connection, id: TransportAttemptId) -> StoreResult<TransportAttemptRecord> {
    let raw: String = db.query_row(
        "SELECT payload FROM transport_attempts WHERE id=?1",
        [id.to_string()],
        |r| r.get(0),
    )?;
    let value: TransportAttemptRecord = record::decode(&raw)?;
    if value.id != id {
        return Err(Error::Conflict);
    }
    Ok(value)
}
pub fn record_attempt(
    tx: &Transaction<'_>,
    frozen: &ModelExecutionSnapshot,
    value: &TransportAttemptRecord,
) -> StoreResult<()> {
    value.request_evidence.validate().map_err(|_| Error::Conflict)?;
    let step = owning_step(tx, frozen.model_step_id)?;
    let expected = TransportOperationContext::AgentModelAttempt {
        model_step_id: step,
        model_execution_snapshot_id: frozen.id,
        context_snapshot_id: frozen.context_snapshot_id,
        prompt_bundle_snapshot_id: frozen.prompt_bundle_snapshot_id,
    };
    if snapshot(tx, frozen.id)?.as_ref() != Some(frozen)
        || value.request_evidence.operation_context != expected
        || value.endpoint_id != frozen.endpoint_id
        || value.endpoint_fingerprint != frozen.endpoint_fingerprint
        || value.attempt_number == 0
        || terminal(value.state) != value.finished_at.is_some()
        || (value.state == TransportAttemptState::Failed) != value.failure_id.is_some()
        || value.finished_at.is_some_and(|v| v < value.started_at)
    {
        return Err(Error::Conflict);
    }
    for id in [value.raw_stream_artifact_id, value.request_evidence.canonical_body_artifact_id]
        .into_iter()
        .flatten()
    {
        artifact::read(tx, id, &step.to_string())?;
    }
    let previous: Option<String> = tx
        .query_row(
            "SELECT payload FROM transport_attempts WHERE id=?1",
            [value.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(raw) = previous {
        let previous: TransportAttemptRecord = record::decode(&raw)?;
        if previous == *value {
            return Ok(());
        }
        let mut immutable = value.clone();
        immutable.state = previous.state;
        immutable.provider_request_id = previous.provider_request_id.clone();
        immutable.first_byte_at = previous.first_byte_at;
        immutable.first_model_event_at = previous.first_model_event_at;
        immutable.finished_at = previous.finished_at;
        immutable.raw_stream_artifact_id = previous.raw_stream_artifact_id;
        immutable.failure_id = previous.failure_id;
        if immutable != previous
            || terminal(previous.state)
            || !transition(previous.state, value.state)
            || !monotonic(&previous.provider_request_id, &value.provider_request_id)
            || !monotonic(&previous.first_byte_at, &value.first_byte_at)
            || !monotonic(&previous.first_model_event_at, &value.first_model_event_at)
            || !monotonic(&previous.raw_stream_artifact_id, &value.raw_stream_artifact_id)
        {
            return Err(Error::Conflict);
        }
        record::changed(tx.execute("UPDATE transport_attempts SET state=?2,revision=revision+1,payload=?3 WHERE id=?1 AND payload=?4",params![value.id.to_string(),record::encode(&value.state)?,record::encode(value)?,raw])?)?;
    } else {
        if value.state != TransportAttemptState::QueuedForEndpoint
            || value.first_byte_at.is_some()
            || value.first_model_event_at.is_some()
            || value.raw_stream_artifact_id.is_some()
        {
            return Err(Error::Conflict);
        }
        insert_transport(tx, frozen.id, value)?;
    }
    // The whole record is the one row above, updated in place; the event says only how it moved.
    let at = UtcTimestamp::now();
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &step.to_string(),
            kind: "model.transport",
            version: 2,
            payload: &serde_json::json!({ "attempt_id": value.id, "state": value.state, "at": at }),
            at,
        },
        "model",
    )?;
    Ok(())
}
fn monotonic<T: PartialEq>(old: &Option<T>, new: &Option<T>) -> bool {
    old.as_ref().is_none_or(|v| new.as_ref() == Some(v))
}
fn terminal(state: TransportAttemptState) -> bool {
    matches!(
        state,
        TransportAttemptState::Completed
            | TransportAttemptState::Failed
            | TransportAttemptState::Cancelled
    )
}
fn transition(old: TransportAttemptState, next: TransportAttemptState) -> bool {
    use TransportAttemptState::*;
    old == next
        || matches!(
            (old, next),
            (QueuedForEndpoint, AcquiringPermit)
                | (AcquiringPermit, PreparingRequest | Cancelled | Failed)
                | (PreparingRequest, AuthorizingNetwork | Cancelled | Failed)
                | (AuthorizingNetwork, Authorized | Connecting | Cancelled | Failed)
                | (Authorized, DispatchPrepared | Cancelled | Failed)
                | (DispatchPrepared, Connecting | Cancelled | Failed)
                | (Connecting, ResponseStarted | AwaitingFirstEvent | Cancelled | Failed)
                | (ResponseStarted, ResponseCompleted | Cancelled | Failed)
                | (ResponseCompleted, Completed | Cancelled | Failed)
                | (AwaitingFirstEvent, Streaming | Cancelled | Failed)
                | (Streaming, Completed | Cancelled | Failed)
        )
}
pub fn record_failure(
    tx: &Transaction<'_>,
    step: ModelStepId,
    value: &PersistedModelFailure,
) -> StoreResult<()> {
    owning_step(tx, step)?;
    if let Some(id) = value.failure.transport_attempt_id
        && !matches!(attempt(tx,id)?.request_evidence.operation_context,TransportOperationContext::AgentModelAttempt{model_step_id,..} if model_step_id==step)
    {
        return Err(Error::Conflict);
    }
    let old:Option<String>=tx.query_row("SELECT payload FROM events WHERE owner=?1 AND kind='model.failure' AND json_extract(payload,'$.id')=?2",[step.to_string(),value.id.to_string()],|r|r.get(0)).optional()?;
    if let Some(old) = old {
        return if record::decode::<PersistedModelFailure>(&old)? == *value {
            Ok(())
        } else {
            Err(Error::Conflict)
        };
    }
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &step.to_string(),
            kind: "model.failure",
            version: 1,
            payload: &serde_json::to_value(value)?,
            at: value.created_at,
        },
        "model",
    )?;
    Ok(())
}
pub fn record_output(
    tx: &Transaction<'_>,
    frozen: &ModelExecutionSnapshot,
    attempt_id: TransportAttemptId,
    output: &NormalizedModelOutput,
    hold_completion: bool,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let attempt = attempt(tx, attempt_id)?;
    if snapshot(tx, frozen.id)?.as_ref() != Some(frozen)
        || attempt.state != TransportAttemptState::Completed
        || output.model_step_id != frozen.model_step_id
        || output.raw_response_ref != attempt.raw_stream_artifact_id
        || !matches!(attempt.request_evidence.operation_context,TransportOperationContext::AgentModelAttempt{model_execution_snapshot_id,..} if model_execution_snapshot_id==frozen.id)
    {
        return Err(Error::Conflict);
    }
    if crate::context::compaction::model_owner(tx, frozen.model_step_id)?.is_some() {
        record_request(tx, frozen.id, &attempt.request_evidence, at)?;
        return crate::context::compaction::record_output(tx, frozen, attempt_id, output, at);
    }
    if super::memory_maintenance::binding(tx, frozen.model_step_id)?.is_some() {
        record_request(tx, frozen.id, &attempt.request_evidence, at)?;
        return super::memory_maintenance::record_output(tx, frozen, output, at);
    }
    let current = turns::step(tx, frozen.model_step_id)?;
    if matches!(current.state, ModelStepState::Completed { .. }) {
        return if history::output(tx, current.model_step_id)? == *output {
            Ok(())
        } else {
            Err(Error::Conflict)
        };
    }
    owning_step(tx, frozen.model_step_id)?;
    let step = current;
    record_request(tx, frozen.id, &attempt.request_evidence, at)?;
    let batch = (!output.tool_calls.is_empty()).then(ToolBatchId::new);
    history::record_output(tx, output, batch, at)?;
    let old = turns::load_run(tx, step.agent_run_id)?;
    let mut next = old.clone();
    next.revision = Revision::new(old.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    next.next_model_ordinal = old.next_model_ordinal.checked_add(1).ok_or(Error::Conflict)?;
    next.updated_at = at;
    next.state = batch.map_or(AgentRunState::PreparingContext, |tool_batch_id| {
        AgentRunState::ToolsRunning { tool_batch_id }
    });
    turns::save_run(tx, &old, &next)?;
    // A trustworthy Normal response ends the turn here, in this very transaction,
    // so the recorded answer and the terminal facts commit or roll back together.
    // When the response is not eligible this returns without side effects and the
    // protocol reminder/stop protection below still governs the turn.
    if batch.is_none() {
        // With a `turn.before_complete` Hook, an answer that would end the turn is kept
        // and the turn waits for the Hook, which runs outside this transaction.
        if hold_completion && crate::agent::completion::natural_eligible(tx, output, at)? {
            crate::hooks::hold_completion(tx, output.model_step_id, at)?;
        } else {
            crate::agent::completion::natural_from_response(tx, output, at)?;
        }
    }
    crate::agent::guard::note_end_turn(
        tx,
        old.agent_run_id,
        output.stop_reason,
        batch.is_some(),
        at,
    )?;
    Ok(())
}
