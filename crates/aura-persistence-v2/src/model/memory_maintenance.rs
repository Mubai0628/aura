//! Auxiliary Model ownership in the existing Model event journal. No Memory
//! content index is placed here, and these leases never reopen an Agent turn.
use super::*;
use aura_agent_domain::TurnTaskState;
use aura_domain_common::*;
use aura_model_domain::NormalizedModelOutput;
use serde::{Deserialize, Serialize};

const OPEN: &str = "model.memory_maintenance.opened";
const CLOSE: &str = "model.memory_maintenance.closed";
const USAGE: &str = "model.memory_maintenance.usage";
pub fn usage(
    db: &Connection,
    step: ModelStepId,
) -> StoreResult<Option<aura_model_domain::PartialModelUsage>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind=?2 ORDER BY sequence DESC LIMIT 1",
            [step.to_string(), USAGE.into()],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|value| record::decode(&value)).transpose()
}
pub fn record_usage(
    tx: &Transaction<'_>,
    frame: &aura_model_domain::ModelStreamEventEnvelope,
) -> StoreResult<()> {
    let aura_model_domain::ModelStreamEvent::UsageUpdated { usage } = &frame.event else {
        return Ok(());
    };
    if !authorize_model(tx, frame.model_step_id)? {
        return Err(Error::Conflict);
    }
    let bound = binding(tx, frame.model_step_id)?.ok_or(Error::Conflict)?;
    let attempt = super::agent_transport::attempt(tx, frame.transport_attempt_id)?;
    if !matches!(attempt.request_evidence.operation_context,aura_model_domain::TransportOperationContext::AgentModelAttempt {model_step_id,model_execution_snapshot_id,..} if model_step_id==frame.model_step_id && model_execution_snapshot_id==bound.snapshot_id)
    {
        return Err(Error::Conflict);
    }
    let previous = self::usage(tx, frame.model_step_id)?;
    let mut merged = previous.unwrap_or_else(|| usage.clone());
    if let Some(tokens) = usage.input_tokens {
        merged.input_tokens = Some(tokens);
    }
    if let Some(tokens) = usage.output_tokens {
        merged.output_tokens = Some(tokens);
    }
    if let Some(tokens) = usage.total_tokens {
        merged.total_tokens = Some(tokens);
    }
    if let Some(tokens) = usage.reasoning_tokens {
        merged.reasoning_tokens = Some(tokens);
    }
    if let Some(tokens) = usage.cached_input_tokens {
        merged.cached_input_tokens = Some(tokens);
    }
    if let Some(tokens) = usage.cache_write_tokens {
        merged.cache_write_tokens = Some(tokens);
    }
    append(tx, frame.model_step_id, USAGE, &serde_json::to_value(merged)?, frame.received_at)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryModelBinding {
    pub session_id: SessionId,
    pub source_run_id: AgentRunId,
    pub runtime_instance_id: RuntimeInstanceId,
    pub snapshot_id: ModelExecutionSnapshotId,
    pub snapshot_hash: ContentHash,
}
/// Every visible-stream append asks this first, so it must not grow with the
/// stream. `open` writes a lease's binding as the very first event of its fresh
/// step, so the step's first event (one row of the owner index) answers it: an
/// Agent step with thousands of deltas is never scanned here.
pub fn binding(db: &Connection, step: ModelStepId) -> StoreResult<Option<MemoryModelBinding>> {
    let first: Option<(String, String)> = db
        .query_row(
            "SELECT kind,payload FROM events WHERE owner=?1 ORDER BY sequence LIMIT 1",
            [step.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match first {
        Some((kind, raw)) if kind == OPEN => Ok(Some(record::decode(&raw)?)),
        _ => Ok(None),
    }
}
pub fn authorize_model(db: &Connection, step: ModelStepId) -> StoreResult<bool> {
    let Some(binding) = binding(db, step)? else { return Ok(false) };
    let closed: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM events WHERE owner=?1 AND kind=?2)",
        [step.to_string(), CLOSE.into()],
        |r| r.get(0),
    )?;
    let Some(snapshot) = snapshot(db, binding.snapshot_id)? else { return Err(Error::Conflict) };
    let session: String = db.query_row(
        "SELECT session_id FROM model_execution_snapshots WHERE id=?1",
        [binding.snapshot_id.to_string()],
        |r| r.get(0),
    )?;
    let source = crate::agent::turns::load_run(db, binding.source_run_id)?;
    let turn = crate::agent::turns::load(db, source.turn_task_id)?;
    if session != binding.session_id.to_string()
        || turn.session_id != binding.session_id
        || !matches!(turn.state, TurnTaskState::Completed { .. })
    {
        return Err(Error::Conflict);
    }
    if snapshot.model_step_id != step
        || ContentHash::blake3(&serde_json::to_vec(&snapshot)?) != binding.snapshot_hash
    {
        return Err(Error::Conflict);
    }
    Ok(!closed)
}
pub fn open(
    tx: &Transaction<'_>,
    run: AgentRunId,
    runtime: RuntimeInstanceId,
    frozen: &ModelExecutionSnapshot,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let source = crate::agent::turns::load_run(tx, run)?;
    let turn = crate::agent::turns::load(tx, source.turn_task_id)?;
    // The binding must be the step's first event (see `binding`): a step that
    // already has any event is not a fresh auxiliary lease.
    let used: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM events WHERE owner=?1)",
        [frozen.model_step_id.to_string()],
        |r| r.get(0),
    )?;
    if !matches!(turn.state, TurnTaskState::Completed { .. })
        || used
        || snapshot(tx, frozen.id)?.is_some()
    {
        return Err(Error::Conflict);
    }
    insert_snapshot(tx, turn.session_id, frozen)?;
    let value = MemoryModelBinding {
        session_id: turn.session_id,
        source_run_id: run,
        runtime_instance_id: runtime,
        snapshot_id: frozen.id,
        snapshot_hash: ContentHash::blake3(&serde_json::to_vec(frozen)?),
    };
    append(tx, frozen.model_step_id, OPEN, &serde_json::to_value(value)?, at)
}
pub fn close(
    tx: &Transaction<'_>,
    step: ModelStepId,
    runtime: RuntimeInstanceId,
    reason: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let value = binding(tx, step)?.ok_or(Error::Conflict)?;
    if value.runtime_instance_id != runtime {
        return Err(Error::Conflict);
    }
    if !authorize_model(tx, step)? {
        return Ok(());
    }
    append(tx, step, CLOSE, &serde_json::json!({"reason":reason,"at":at}), at)
}
/// `open` writes a lease with its snapshot, as its step's first event, in one transaction.
/// So the steps that may hold one are the snapshots' steps, and each is answered by its first
/// event, one row of the owner index (`binding`). Read by kind, every start read the whole journal;
/// read by owner, every event of every Agent step. A lease whose snapshot was purged with its
/// session is left as it is: without the snapshot `authorize_model` already refuses it.
pub fn interrupt_previous_process(
    tx: &Transaction<'_>,
    runtime: RuntimeInstanceId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let mut statement = tx.prepare(
        "SELECT DISTINCT json_extract(payload,'$.model_step_id') FROM model_execution_snapshots \
         WHERE json_extract(payload,'$.model_step_id') IS NOT NULL",
    )?;
    let steps =
        statement.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for step in steps {
        let step: ModelStepId = step.parse().map_err(|_| Error::Conflict)?;
        let Some(previous) = binding(tx, step)? else { continue };
        let closed: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE owner=?1 AND kind=?2)",
            [step.to_string(), CLOSE.into()],
            |r| r.get(0),
        )?;
        if !closed && previous.runtime_instance_id != runtime {
            append(
                tx,
                step,
                CLOSE,
                &serde_json::json!({"reason":"interrupted; never replay","at":at}),
                at,
            )?;
        }
    }
    Ok(())
}
pub fn record_output(
    tx: &Transaction<'_>,
    frozen: &ModelExecutionSnapshot,
    output: &NormalizedModelOutput,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if !authorize_model(tx, frozen.model_step_id)? || !output.tool_calls.is_empty() {
        return Err(Error::Conflict);
    }
    // Private Model evidence, not conversation history, Memory modification
    // history, a new task completion, or a Goal budget debit.
    let bytes = serde_json::to_vec(output)?;
    let id = ArtifactId::new();
    aura_persistence_core::artifact::put(
        tx,
        id,
        &frozen.model_step_id.to_string(),
        "application/json",
        &bytes,
        at,
    )?;
    append(
        tx,
        frozen.model_step_id,
        "model.memory_maintenance.output",
        &serde_json::json!({"artifact_id":id,"hash":ContentHash::blake3(&bytes)}),
        at,
    )
}
fn append(
    tx: &Transaction<'_>,
    step: ModelStepId,
    kind: &str,
    payload: &serde_json::Value,
    at: UtcTimestamp,
) -> StoreResult<()> {
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &step.to_string(),
            kind,
            version: 1,
            payload,
            at,
        },
        "model",
    )?;
    Ok(())
}
