use aura_domain_common::{ModelExecutionSnapshotId, ModelProfileId, SessionId};
use aura_model_domain::{ModelExecutionSnapshot, TransportAttemptRecord};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, event, record};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
pub mod agent_transport;
pub mod connections;
pub mod context_window;
pub mod images;
pub mod memory_maintenance;
pub(crate) mod recovery;
pub mod transient_retry;

// Declarative Provider/Endpoint/Profile remain models.json authority. Only
// session binding and immutable execution facts belong in this Store.
pub fn bind_session(
    tx: &Transaction<'_>,
    session: SessionId,
    profile: ModelProfileId,
    expected: Option<i64>,
) -> StoreResult<()> {
    match expected {
        None=>{tx.execute("INSERT INTO session_model_bindings VALUES(?1,?2,1)",params![session.to_string(),profile.to_string()])?;},
        Some(revision)=>record::changed(tx.execute("UPDATE session_model_bindings SET profile_id=?2,revision=revision+1 WHERE session_id=?1 AND revision=?3",params![session.to_string(),profile.to_string(),revision])?)?,
    }
    Ok(())
}
pub fn insert_snapshot(
    tx: &Transaction<'_>,
    session: SessionId,
    snapshot: &ModelExecutionSnapshot,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO model_execution_snapshots VALUES(?1,?2,?3,?4,?5)",
        params![
            snapshot.id.to_string(),
            session.to_string(),
            snapshot.endpoint_id.to_string(),
            snapshot.model_profile_id.to_string(),
            record::encode(snapshot)?
        ],
    )?;
    Ok(())
}
pub fn snapshot(
    connection: &Connection,
    id: ModelExecutionSnapshotId,
) -> StoreResult<Option<ModelExecutionSnapshot>> {
    let v: Option<String> = connection
        .query_row(
            "SELECT payload FROM model_execution_snapshots WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    v.map(|v| record::decode(&v)).transpose()
}
pub fn insert_transport(
    tx: &Transaction<'_>,
    snapshot: ModelExecutionSnapshotId,
    attempt: &TransportAttemptRecord,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO transport_attempts VALUES(?1,?2,?3,1,?4)",
        params![
            attempt.id.to_string(),
            snapshot.to_string(),
            record::encode(&attempt.state)?,
            record::encode(attempt)?
        ],
    )?;
    Ok(())
}

/// Exact, redacted P05 compilation evidence. Secret header values and request
/// bodies are not copied into this record. Store before dispatching HTTP.
pub fn record_request(
    tx: &Transaction<'_>,
    snapshot_id: ModelExecutionSnapshotId,
    evidence: &aura_model_domain::ProviderRequestEvidence,
    at: aura_domain_common::UtcTimestamp,
) -> StoreResult<()> {
    evidence.validate().map_err(|_| Error::Invalid("Provider request evidence"))?;
    let snapshot = snapshot(tx, snapshot_id)?.ok_or(Error::Conflict)?;
    if !matches!(&evidence.operation_context, aura_model_domain::TransportOperationContext::AgentModelAttempt { model_step_id, model_execution_snapshot_id, context_snapshot_id, prompt_bundle_snapshot_id }
        if *model_step_id == snapshot.model_step_id && *model_execution_snapshot_id == snapshot.id && *context_snapshot_id == snapshot.context_snapshot_id && *prompt_bundle_snapshot_id == snapshot.prompt_bundle_snapshot_id)
        || evidence.adapter.api_format != snapshot.api_format
    {
        return Err(Error::Conflict);
    }
    if let Some(old) = request_evidence(tx, snapshot_id)? {
        return if old == *evidence { Ok(()) } else { Err(Error::Conflict) };
    }
    event::append(
        tx,
        &event::EventDraft {
            id: aura_domain_common::EventId::new(),
            owner: &snapshot_id.to_string(),
            kind: "model.request_evidence",
            version: 1,
            payload: &serde_json::to_value(evidence)?,
            at,
        },
        "model",
    )?;
    Ok(())
}
pub fn request_evidence(
    db: &Connection,
    snapshot_id: ModelExecutionSnapshotId,
) -> StoreResult<Option<aura_model_domain::ProviderRequestEvidence>> {
    let raw: Option<String> = db.query_row("SELECT payload FROM events WHERE owner=?1 AND kind='model.request_evidence' AND version=1 ORDER BY sequence DESC LIMIT 1",
        [snapshot_id.to_string()], |r| r.get(0)).optional()?;
    raw.map(|s| record::decode(&s)).transpose()
}
pub fn step_snapshot(
    db: &Connection,
    step_id: aura_domain_common::ModelStepId,
) -> StoreResult<ModelExecutionSnapshot> {
    let id: String = db.query_row(
        "SELECT snapshot_id FROM model_steps WHERE id=?1",
        [step_id.to_string()],
        |r| r.get(0),
    )?;
    snapshot(db, id.parse().map_err(|_| Error::Conflict)?)?.ok_or(Error::Conflict)
}
