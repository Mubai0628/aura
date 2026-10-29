//! The outbox copies, the whole records older versions put into `model.transport` events, and the
//! raw streams of completed calls. Each batch is one transaction.
use aura_domain_common::ArtifactId;
use aura_model_domain::{TransportAttemptRecord, TransportAttemptState, TransportOperationContext};
use aura_persistence_core::{DatabaseConnection, StoreResult, artifact, record};
use rusqlite::{Connection, OptionalExtension, params};

use super::{Progress, Report};
use crate::session_purge::guards;

/// Sequences of a table read or deleted in one transaction.
const RANGE: i64 = 20_000;
/// Calls whose raw streams go in one transaction.
const CALLS: usize = 32;

fn bounds(db: &Connection, table: &str) -> StoreResult<Option<(i64, i64)>> {
    let bounds: (Option<i64>, Option<i64>) =
        db.query_row(&format!("SELECT min(sequence), max(sequence) FROM {table}"), [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
    Ok(match bounds {
        (Some(low), Some(high)) => Some((low, high)),
        _ => None,
    })
}

/// About how many outbox rows there are; the sequence is the row id, so this reads two pages.
pub(super) fn outbox_rows(db: &Connection) -> StoreResult<u64> {
    Ok(bounds(db, "projection_outbox")?.map_or(0, |(low, high)| (high - low + 1) as u64))
}

pub(super) fn calls(db: &Connection) -> StoreResult<u64> {
    Ok(db.query_row("SELECT count(*) FROM transport_attempts", [], |row| row.get::<_, i64>(0))?
        as u64)
}

pub(super) fn streams(db: &Connection) -> StoreResult<u64> {
    Ok(db.query_row(
        "SELECT count(*) FROM artifacts WHERE media_type='text/event-stream'",
        [],
        |row| row.get::<_, i64>(0),
    )? as u64)
}

/// Every outbox row: nothing reads them, so they are deleted range by range of their sequence.
pub(super) fn outbox(
    store: &mut DatabaseConnection,
    report: &mut Report,
    progress: &mut Progress<'_>,
) -> StoreResult<()> {
    let Some((low, high)) = bounds(store.connection(), "projection_outbox")? else {
        return Ok(());
    };
    let mut from = low;
    while from <= high && !progress.stopped {
        let to = from.saturating_add(RANGE - 1).min(high);
        let deleted = store.unit_of_work(|tx| {
            tx.execute("DELETE FROM projection_outbox WHERE sequence BETWEEN ?1 AND ?2", [from, to])
        })?;
        report.outbox_rows += deleted as u64;
        progress.outbox((to - from + 1) as u64);
        from = to + 1;
    }
    Ok(())
}

/// An older version's `model.transport` event carries the call's whole record, which is also the
/// call's own row. It is rewritten as the event a current version writes: which call, which state,
/// and when (`model::agent_transport::record_attempt`).
pub(super) fn transport_events(
    store: &mut DatabaseConnection,
    report: &mut Report,
    progress: &mut Progress<'_>,
) -> StoreResult<()> {
    let Some((low, high)) = bounds(store.connection(), "events")? else {
        return Ok(());
    };
    let whole = (high - low + 1) as u64;
    let mut from = low;
    while from <= high && !progress.stopped {
        let to = from.saturating_add(RANGE - 1).min(high);
        let rewritten = store.unit_of_work(|tx| -> StoreResult<u64> {
            let mut query = tx.prepare(
                "SELECT sequence, json_extract(payload,'$.id'), json_extract(payload,'$.state'), \
                 created_at FROM events \
                 WHERE sequence BETWEEN ?1 AND ?2 AND kind='model.transport' AND version=1",
            )?;
            let found = query
                .query_map([from, to], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            drop(query);
            if found.is_empty() {
                return Ok(0);
            }
            let lifted = guards::lift(tx, guards::EVENT_UPDATE)?;
            for (sequence, attempt, state, at) in &found {
                let payload = record::encode(
                    &serde_json::json!({ "attempt_id": attempt, "state": state, "at": at }),
                )?;
                tx.execute(
                    "UPDATE events SET version=2, payload=?2 WHERE sequence=?1",
                    params![sequence, payload],
                )?;
            }
            guards::restore(tx, lifted)?;
            Ok(found.len() as u64)
        })?;
        report.transport_events += rewritten;
        progress.transport_events((to - from + 1) as u64, whole);
        from = to + 1;
    }
    Ok(())
}

/// A completed call's answer is kept parsed; its raw stream is never read again. The call's row
/// stops naming it and the step gives it up. A failed call keeps its stream for diagnosis. The
/// answer records an older version sealed keep naming the stream: they are not rewritten, and
/// nothing resolves that name.
pub(super) fn raw_streams(
    store: &mut DatabaseConnection,
    report: &mut Report,
    progress: &mut Progress<'_>,
) -> StoreResult<()> {
    let completed = record::encode(&TransportAttemptState::Completed)?;
    let mut query = store.connection().prepare(
        "SELECT id FROM transport_attempts \
         WHERE state=?1 AND json_extract(payload,'$.raw_stream_artifact_id') IS NOT NULL",
    )?;
    let calls = query
        .query_map([completed], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(query);
    for batch in calls.chunks(CALLS) {
        if progress.stopped {
            break;
        }
        let dropped = store.unit_of_work(|tx| -> StoreResult<u64> {
            let mut dropped = 0;
            for id in batch {
                let Some(raw) = tx
                    .query_row("SELECT payload FROM transport_attempts WHERE id=?1", [id], |row| {
                        row.get::<_, String>(0)
                    })
                    .optional()?
                else {
                    continue;
                };
                let mut attempt: TransportAttemptRecord = record::decode(&raw)?;
                let TransportOperationContext::AgentModelAttempt { model_step_id, .. } =
                    &attempt.request_evidence.operation_context
                else {
                    continue;
                };
                let owner = model_step_id.to_string();
                let Some(stream): Option<ArtifactId> = attempt.raw_stream_artifact_id.take() else {
                    continue;
                };
                record::changed(tx.execute(
                    "UPDATE transport_attempts SET revision=revision+1, payload=?2 \
                     WHERE id=?1 AND payload=?3",
                    params![id, record::encode(&attempt)?, raw],
                )?)?;
                artifact::release_owner(tx, stream, &owner)?;
                dropped += 1;
            }
            artifact::collect(tx)?;
            Ok(dropped)
        })?;
        report.raw_streams += dropped;
        progress.raw_streams(batch.len() as u64);
    }
    Ok(())
}
