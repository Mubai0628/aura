//! The context snapshots an older version kept for every step: each conversation keeps the two a
//! current version keeps (`context::keep_two`), and the others go with the section texts only
//! their steps held.
use aura_domain_common::SessionId;
use aura_persistence_core::{
    DatabaseConnection, PersistenceCoreError as Error, StoreResult, artifact,
};
use rusqlite::Connection;

use super::{Progress, Report};

/// Snapshots cleared in one transaction, about 2 MB each with their section texts.
const SNAPSHOTS: usize = 8;

/// About how many snapshots go: all but each conversation's newest.
pub(super) fn count(db: &Connection) -> StoreResult<u64> {
    Ok(db.query_row(
        "SELECT count(*) - count(DISTINCT session_id) FROM context_snapshots",
        [],
        |row| row.get::<_, i64>(0),
    )? as u64)
}

pub(super) fn clear(
    store: &mut DatabaseConnection,
    report: &mut Report,
    progress: &mut Progress<'_>,
) -> StoreResult<()> {
    let sessions = {
        let mut query =
            store.connection().prepare("SELECT DISTINCT session_id FROM context_snapshots")?;
        query.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?
    };
    for session in sessions {
        let older = older(store.connection(), &session)?;
        for batch in older.chunks(SNAPSHOTS) {
            if progress.stopped {
                return Ok(());
            }
            store.unit_of_work(|tx| -> StoreResult<()> {
                for id in batch {
                    let (step, sections): (Option<String>, Option<String>) = tx.query_row(
                        "SELECT json_extract(payload,'$.model_step_id'), \
                         json_extract(payload,'$.sections') FROM context_snapshots WHERE id=?1",
                        [id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )?;
                    crate::context::clear_snapshot(
                        tx,
                        id,
                        step.as_deref().unwrap_or_default(),
                        sections.as_deref().unwrap_or("[]"),
                    )?;
                }
                artifact::collect(tx)?;
                Ok(())
            })?;
            report.snapshots += batch.len() as u64;
            progress.snapshots(batch.len() as u64);
        }
    }
    Ok(())
}

/// The conversation's snapshots but its newest and the one behind its newest counted request,
/// oldest first.
fn older(db: &Connection, session: &str) -> StoreResult<Vec<String>> {
    let id: SessionId = session.parse().map_err(|_| Error::Invalid("context snapshot session"))?;
    let counted = crate::agent::counted_request::latest_counted_snapshot(db, id)?;
    let mut query = db.prepare(
        "SELECT id FROM context_snapshots WHERE session_id=?1 AND (?2 IS NULL OR id<>?2) \
         AND rowid < (SELECT max(rowid) FROM context_snapshots WHERE session_id=?1) \
         ORDER BY rowid",
    )?;
    let older =
        query.query_map(rusqlite::params![session, counted], |row| row.get::<_, String>(0))?;
    Ok(older.collect::<Result<Vec<_>, _>>()?)
}
