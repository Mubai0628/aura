//! The streamed deltas of replies that completed. An older version kept them beside the sealed
//! text, which is what a completed reply is read from; a current one drops them with the seal
//! (`agent::stream::drop_sealed`). A reply that failed or was stopped keeps its own.
use aura_domain_common::ModelStepId;
use aura_persistence_core::{DatabaseConnection, PersistenceCoreError as Error, StoreResult};
use rusqlite::Connection;

use super::{Progress, Report};

/// Completed replies whose deltas go in one transaction.
const REPLIES: usize = 16;

/// About how many deltas completed replies still keep, from the coverage a stream keeps of
/// itself: one small row per attempt, so this reads no delta.
pub(super) fn count(db: &Connection) -> StoreResult<u64> {
    Ok(db.query_row(
        "SELECT coalesce(sum(c.index_count), 0) FROM agent_visible_coverage c \
         JOIN model_steps s ON s.id=c.owner WHERE s.state='completed'",
        [],
        |row| row.get::<_, i64>(0),
    )? as u64)
}

pub(super) fn clear(
    store: &mut DatabaseConnection,
    report: &mut Report,
    progress: &mut Progress<'_>,
) -> StoreResult<()> {
    let replies = store
        .connection()
        .prepare("SELECT id FROM model_steps WHERE state='completed' ORDER BY id")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for batch in replies.chunks(REPLIES) {
        if progress.stopped {
            break;
        }
        let (cleared, events) = store.unit_of_work(|tx| -> StoreResult<(u64, u64)> {
            let (mut cleared, mut events) = (0, 0);
            for id in batch {
                let step: ModelStepId = id.parse().map_err(|_| Error::Invalid("model step id"))?;
                if let Some(dropped @ 1..) = crate::agent::stream::drop_sealed(tx, step)? {
                    cleared += 1;
                    events += dropped;
                }
            }
            Ok((cleared, events))
        })?;
        report.streamed_replies += cleared;
        progress.streams(events);
    }
    Ok(())
}
