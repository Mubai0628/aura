//! Releasing the artifacts a purge's rows held, and collecting the queued artifacts nothing uses
//! with the objects only they held.
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Transaction, params};

use super::edges::quoted;
use super::plan::PurgePlan;
use super::scope::json;
use super::steps::CHUNK;

/// Rows that go with a collected artifact; every other key that points at one keeps it, its
/// owners included.
const GOING: [&str; 2] = ["artifact_access_links", "artifact_gc_queue"];

/// Queues the held artifacts nothing owns any more.
pub(super) fn release(tx: &Transaction<'_>, held: &[String]) -> StoreResult<()> {
    for chunk in held.chunks(CHUNK) {
        tx.execute(
            "INSERT INTO artifact_gc_queue SELECT id FROM artifacts
             WHERE id IN (SELECT value FROM json_each(?1))
               AND NOT EXISTS(SELECT 1 FROM artifact_owners o WHERE o.artifact_id=artifacts.id)
             ON CONFLICT DO NOTHING",
            [json(chunk)],
        )?;
    }
    Ok(())
}

/// Collects the next page of queued artifacts that nothing uses; at the end of the queue the plan
/// is collected. An artifact still in use stays queued, as before.
pub(super) fn next(tx: &Transaction<'_>, plan: &mut PurgePlan) -> StoreResult<()> {
    let after = plan.collected_to.clone().unwrap_or_default();
    let queued: Vec<String> = {
        let mut statement = tx.prepare(
            "SELECT artifact_id FROM artifact_gc_queue WHERE artifact_id>?1
             ORDER BY artifact_id LIMIT ?2",
        )?;
        let rows = statement.query_map(params![after, CHUNK as i64], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let Some(last) = queued.last().cloned() else {
        plan.collected = true;
        return Ok(());
    };
    plan.collected_to = Some(last);
    let gone: Vec<(String, String)> = {
        let mut statement = tx.prepare(&format!(
            "DELETE FROM artifacts WHERE id IN (SELECT value FROM json_each(?1)){}
             RETURNING id, object_hash",
            plan.edges.guards("artifacts", &GOING, &[]),
        ))?;
        let rows = statement.query_map([json(&queued)], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    if gone.is_empty() {
        return Ok(());
    }
    let ids = json(&gone.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>());
    for child in GOING {
        let edge = plan
            .edges
            .edge("artifacts", child)
            .filter(|edge| edge.cascade && edge.columns.len() == 1)
            .ok_or(Error::Invalid("artifact child key"))?;
        tx.execute(
            &format!(
                "DELETE FROM {} WHERE {} IN (SELECT value FROM json_each(?1))",
                quoted(child),
                quoted(&edge.columns[0])
            ),
            [&ids],
        )?;
    }
    let mut hashes: Vec<String> = gone.iter().map(|(_, hash)| hash.clone()).collect();
    hashes.sort();
    hashes.dedup();
    tx.execute(
        &format!(
            "DELETE FROM artifact_objects WHERE hash IN (SELECT value FROM json_each(?1)){}",
            plan.edges.guards("artifact_objects", &[], &[]),
        ),
        [json(&hashes)],
    )?;
    plan.report.artifacts += gone.len();
    Ok(())
}
