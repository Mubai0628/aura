//! Compaction owns auxiliary ModelSteps. They never create User/Assistant
//! conversation events or increment the Agent's normal response ordinal.
use aura_agent_domain::{AgentRunState, ModelStepState};
use aura_context_domain::*;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, event, record};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Compaction {
    pub id: ContextProjectionSnapshotId,
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub source_step_id: ModelStepId,
    pub sequence: u32,
    pub revision: Revision,
    pub state: String,
    pub source_span: CompactionSourceSpan,
    pub anchors: CompactionAnchorManifest,
    pub source_artifact_id: ArtifactId,
    pub source_hash: ContentHash,
    pub excluded_event_ids: Vec<EventId>,
    pub retained_recent_tokens: u64,
    pub retention_exceptions: Vec<ContextRetentionException>,
    pub attempts: Vec<CompactionModelAttemptRecord>,
    pub result: Option<ContextReadCompactionResult>,
    pub result_artifact_id: Option<ArtifactId>,
    pub created_at: UtcTimestamp,
}
pub fn by_result_artifact(
    db: &Connection,
    artifact_id: ArtifactId,
) -> StoreResult<Option<Compaction>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM context_compactions WHERE json_extract(payload,'$.result_artifact_id')=?1",
            [artifact_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    raw.map(|payload| {
        let value: Compaction = record::decode(&payload)?;
        if value.result_artifact_id != Some(artifact_id) {
            return Err(Error::Conflict);
        }
        Ok(value)
    })
    .transpose()
}

pub fn load(db: &Connection, id: ContextProjectionSnapshotId) -> StoreResult<Compaction> {
    let raw: String = db.query_row(
        "SELECT payload FROM context_compactions WHERE id=?1",
        [id.to_string()],
        |r| r.get(0),
    )?;
    let value: Compaction = record::decode(&raw)?;
    if value.id != id {
        return Err(Error::Conflict);
    }
    Ok(value)
}
pub fn latest(db: &Connection, session: SessionId) -> StoreResult<Option<Compaction>> {
    let raw: Option<String> = db.query_row("SELECT payload FROM context_compactions WHERE session_id=?1 AND state='completed' ORDER BY json_extract(payload,'$.sequence') DESC LIMIT 1", [session.to_string()], |r| r.get(0)).optional()?;
    raw.map(|s| record::decode(&s)).transpose()
}
pub fn next_sequence(db: &Connection, session: SessionId) -> StoreResult<u32> {
    let value: i64 = db.query_row("SELECT COALESCE(MAX(json_extract(payload,'$.sequence')),0)+1 FROM context_compactions WHERE session_id=?1", [session.to_string()], |r| r.get(0))?;
    value.try_into().map_err(|_| Error::Conflict)
}
pub fn begin(tx: &Transaction<'_>, value: &Compaction, sources: &[u8]) -> StoreResult<()> {
    validate_owner(tx, value)?;
    if value.sequence != next_sequence(tx, value.session_id)?
        || value.revision != Revision::INITIAL
        || value.state != "running"
        || !value.attempts.is_empty()
        || value.result.is_some()
        || value.result_artifact_id.is_some()
        || ContentHash::sha256(sources) != value.source_hash
        || canonical_json_v1_sha256(&value.anchors.anchors).map_err(|_| Error::Conflict)?
            != value.anchors.manifest_hash
        || value.source_span.source_refs
            != value.anchors.anchors.iter().map(|a| a.source_ref.clone()).collect::<Vec<_>>()
    {
        return Err(Error::Conflict);
    }
    artifact::put(
        tx,
        value.source_artifact_id,
        &value.id.to_string(),
        "application/vnd.aura.compaction-sources+json",
        sources,
        value.created_at,
    )?;
    tx.execute(
        "INSERT INTO context_compactions VALUES(?1,?2,'running',1,?3)",
        params![value.id.to_string(), value.session_id.to_string(), record::encode(value)?],
    )?;
    Ok(())
}
fn validate_owner(db: &Connection, value: &Compaction) -> StoreResult<()> {
    let run = crate::agent::turns::load_run(db, value.agent_run_id)?;
    let turn = crate::agent::turns::load(db, run.turn_task_id)?;
    let step = crate::agent::turns::step(db, value.source_step_id)?;
    if run.state != AgentRunState::PreparingContext
        || turn.state.terminal()
        || turn.session_id != value.session_id
        || step.agent_run_id != run.agent_run_id
        || step.state != ModelStepState::Preparing
        || crate::agent::application::cancellation_requested(db, run.agent_run_id)?
    {
        return Err(Error::Conflict);
    }
    Ok(())
}
fn save(tx: &Transaction<'_>, old: &Compaction, next: &mut Compaction) -> StoreResult<()> {
    next.revision = Revision::new(old.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    let revision = i64::try_from(next.revision.value()).map_err(|_| Error::Conflict)?;
    record::changed(tx.execute(
        "UPDATE context_compactions SET state=?2,revision=?3,payload=?4 WHERE id=?1 AND payload=?5",
        params![
            old.id.to_string(),
            next.state,
            revision,
            record::encode(next)?,
            record::encode(old)?
        ],
    )?)
}
pub fn model_owner(db: &Connection, step: ModelStepId) -> StoreResult<Option<Compaction>> {
    let id: Option<String> = db.query_row("SELECT c.id FROM context_compactions c,json_each(c.payload,'$.attempts') a WHERE json_extract(a.value,'$.model_step_id')=?1", [step.to_string()], |r| r.get(0)).optional()?;
    id.map(|s| load(db, s.parse().map_err(|_| Error::Conflict)?)).transpose()
}
pub fn authorize_model(db: &Connection, step: ModelStepId) -> StoreResult<bool> {
    let Some(value) = model_owner(db, step)? else {
        return Ok(false);
    };
    validate_owner(db, &value)?;
    if value.state != "running"
        || !value.attempts.last().is_some_and(|a| {
            a.model_step_id == step && a.state == CompactionModelAttemptState::Running
        })
    {
        return Err(Error::Conflict);
    }
    Ok(true)
}
pub fn start_attempt(
    tx: &Transaction<'_>,
    id: ContextProjectionSnapshotId,
    snapshot: &ModelExecutionSnapshot,
    attempt: CompactionModelAttemptRecord,
) -> StoreResult<()> {
    let old = load(tx, id)?;
    validate_owner(tx, &old)?;
    if old.state != "running"
        || attempt.attempt_ordinal as usize != old.attempts.len() + 1
        || attempt.attempt_ordinal > 2
        || old.attempts.last().is_some_and(|a| a.state != CompactionModelAttemptState::Failed)
        || attempt.state != CompactionModelAttemptState::Running
        || attempt.model_step_id != snapshot.model_step_id
        || attempt.model_execution_snapshot_id != snapshot.id
        || attempt.attempt_context_snapshot_id != snapshot.context_snapshot_id
        || attempt.source_context_snapshot_id != id
        || attempt.compaction_sequence != old.sequence
    {
        return Err(Error::Conflict);
    }
    crate::model::insert_snapshot(tx, old.session_id, snapshot)?;
    let mut next = old.clone();
    next.attempts.push(attempt);
    save(tx, &old, &mut next)
}
pub fn record_output(
    tx: &Transaction<'_>,
    snapshot: &ModelExecutionSnapshot,
    transport: TransportAttemptId,
    output: &NormalizedModelOutput,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if !authorize_model(tx, snapshot.model_step_id)? {
        return Err(Error::Conflict);
    }
    let body = record::encode(output)?;
    let old: Option<String> = tx
        .query_row(
            "SELECT payload FROM model_outputs WHERE snapshot_id=?1",
            [snapshot.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old) = old {
        return if old == body { Ok(()) } else { Err(Error::Conflict) };
    }
    let id = ArtifactId::new();
    artifact::put(
        tx,
        id,
        &snapshot.model_step_id.to_string(),
        "application/vnd.aura.model-output+json",
        body.as_bytes(),
        at,
    )?;
    tx.execute(
        "INSERT INTO model_outputs VALUES(?1,?2,'completed',?3)",
        params![snapshot.id.to_string(), id.to_string(), body],
    )?;
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &snapshot.model_step_id.to_string(),
            kind: "context.compaction_model_output",
            version: 1,
            payload: &serde_json::json!({"transport_attempt_id":transport,"usage":output.usage}),
            at,
        },
        "context",
    )?;
    Ok(())
}
pub fn output(
    db: &Connection,
    snapshot: ModelExecutionSnapshotId,
) -> StoreResult<NormalizedModelOutput> {
    let raw: String = db.query_row(
        "SELECT payload FROM model_outputs WHERE snapshot_id=?1",
        [snapshot.to_string()],
        |r| r.get(0),
    )?;
    record::decode(&raw)
}
pub fn usage(db: &Connection, run: AgentRunId) -> StoreResult<(u64, u64)> {
    let mut q=db.prepare("SELECT m.payload FROM context_compactions c,json_each(c.payload,'$.attempts') a JOIN model_outputs m ON m.snapshot_id=json_extract(a.value,'$.model_execution_snapshot_id') WHERE json_extract(c.payload,'$.agent_run_id')=?1")?;
    let mut tokens = 0u64;
    let mut turns = 0u64;
    for row in q.query_map([run.to_string()], |r| r.get::<_, String>(0))? {
        let output: NormalizedModelOutput = record::decode(&row?)?;
        tokens = tokens.checked_add(output.usage.total_tokens).ok_or(Error::Conflict)?;
        turns = turns.checked_add(1).ok_or(Error::Conflict)?;
    }
    Ok((tokens, turns))
}
#[expect(
    clippy::too_many_arguments,
    reason = "Explicit terminal evidence and retry disposition update one exact compaction attempt atomically"
)]
pub fn finish_attempt(
    tx: &Transaction<'_>,
    id: ContextProjectionSnapshotId,
    ordinal: u8,
    state: CompactionModelAttemptState,
    transport: Option<TransportAttemptId>,
    error: Option<String>,
    retry: bool,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let old = load(tx, id)?;
    let mut next = old.clone();
    let a = next.attempts.last_mut().ok_or(Error::Conflict)?;
    if a.attempt_ordinal != ordinal
        || a.state != CompactionModelAttemptState::Running
        || old.state != "running"
        || state == CompactionModelAttemptState::Running
    {
        return Err(Error::Conflict);
    }
    a.state = state;
    a.transport_attempt_id = transport;
    a.error_code = error;
    a.terminal_at = Some(at);
    if !retry {
        next.state =
            if state == CompactionModelAttemptState::Cancelled { "cancelled" } else { "failed" }
                .into();
        super::manual_compaction::finish_linked(
            tx,
            &old,
            if state == CompactionModelAttemptState::Cancelled {
                ManualCompactionRequestState::Cancelled
            } else {
                ManualCompactionRequestState::Failed
            },
            a.error_code.clone(),
            at,
        )?;
    }
    save(tx, &old, &mut next)
}
pub fn complete(
    tx: &Transaction<'_>,
    id: ContextProjectionSnapshotId,
    ordinal: u8,
    transport: TransportAttemptId,
    result: ContextReadCompactionResult,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let old = load(tx, id)?;
    validate_owner(tx, &old)?;
    let mut next = old.clone();
    let a = next.attempts.last_mut().ok_or(Error::Conflict)?;
    if old.state != "running"
        || a.attempt_ordinal != ordinal
        || a.state != CompactionModelAttemptState::Running
        || result.source_context_snapshot_id != id
        || result.sequence != old.sequence
        || result.source_span != old.source_span
        || result.anchor_manifest != old.anchors
    {
        return Err(Error::Conflict);
    }
    let attempt = crate::model::agent_transport::attempt(tx, transport)?;
    if attempt.state != TransportAttemptState::Completed
        || !matches!(attempt.request_evidence.operation_context,TransportOperationContext::AgentModelAttempt{model_step_id,..} if model_step_id==a.model_step_id)
    {
        return Err(Error::Conflict);
    }
    a.state = CompactionModelAttemptState::Completed;
    a.transport_attempt_id = Some(transport);
    a.terminal_at = Some(at);
    let artifact_id = ArtifactId::new();
    artifact::put(
        tx,
        artifact_id,
        &id.to_string(),
        "application/vnd.aura.compaction+json",
        &serde_json::to_vec(&result)?,
        at,
    )?;
    next.result = Some(result);
    next.result_artifact_id = Some(artifact_id);
    next.state = "completed".into();
    save(tx, &old, &mut next)?;
    super::manual_compaction::finish_linked(
        tx,
        &old,
        ManualCompactionRequestState::Completed,
        None,
        at,
    )?;
    compacted(tx, &old, at)
}

/// Core's own summary for a compaction whose model attempts all failed: the history still leaves
/// the request, and the failed attempts stay recorded. Only Core's own compaction falls back; one
/// the user asked for reports its failure instead.
pub fn complete_by_core(
    tx: &Transaction<'_>,
    id: ContextProjectionSnapshotId,
    result: ContextReadCompactionResult,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let old = load(tx, id)?;
    validate_owner(tx, &old)?;
    if old.state != "failed"
        || old.attempts.is_empty()
        || old.attempts.iter().any(|a| a.state == CompactionModelAttemptState::Running)
        || result.source_context_snapshot_id != id
        || result.sequence != old.sequence
        || result.source_span != old.source_span
        || result.anchor_manifest != old.anchors
        || super::manual_compaction::linked(tx, &old)?
    {
        return Err(Error::Conflict);
    }
    let artifact_id = ArtifactId::new();
    artifact::put(
        tx,
        artifact_id,
        &id.to_string(),
        "application/vnd.aura.compaction+json",
        &serde_json::to_vec(&result)?,
        at,
    )?;
    let mut next = old.clone();
    next.result = Some(result);
    next.result_artifact_id = Some(artifact_id);
    next.state = "completed".into();
    save(tx, &old, &mut next)?;
    compacted(tx, &old, at)
}

/// The turn's lineage learns its history was compacted.
fn compacted(tx: &Transaction<'_>, old: &Compaction, at: UtcTimestamp) -> StoreResult<()> {
    let run = crate::agent::turns::load_run(tx, old.agent_run_id)?;
    let turn = crate::agent::turns::load(tx, run.turn_task_id)?;
    if turn.lineage_resolution_required {
        return Ok(());
    }
    let previous = crate::agent::lineage::load(tx, turn.lineage_id)?;
    if !previous.source_recheck.compacted {
        let mut next = previous.clone();
        next.source_recheck.compacted = true;
        next.revision =
            Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
        next.updated_at = at;
        next.completion_gate = aura_agent_domain::CompletionGate::Invalidated {
            reason: aura_agent_domain::CompletionInvalidation::ContextCompacted,
        };
        crate::agent::lineage::save(tx, &previous, &next)?;
    }
    Ok(())
}
pub fn interrupt_run(tx: &Transaction<'_>, run: AgentRunId, at: UtcTimestamp) -> StoreResult<()> {
    super::manual_compaction::cancel_run(tx, run, "context.compaction_interrupted", at)?;
    let mut q=tx.prepare("SELECT id FROM context_compactions WHERE state='running' AND json_extract(payload,'$.agent_run_id')=?1")?;
    let ids = q
        .query_map([run.to_string()], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        let old = load(tx, id.parse().map_err(|_| Error::Conflict)?)?;
        let mut next = old.clone();
        next.state = "interrupted".into();
        if let Some(a) =
            next.attempts.last_mut().filter(|a| a.state == CompactionModelAttemptState::Running)
        {
            a.state = CompactionModelAttemptState::UnknownSideEffect;
            a.error_code = Some("context.compaction_interrupted".into());
            a.terminal_at = Some(at);
        }
        save(tx, &old, &mut next)?;
    }
    Ok(())
}
