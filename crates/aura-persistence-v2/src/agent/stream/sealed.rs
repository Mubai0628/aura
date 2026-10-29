//! A sealed reply's streamed deltas. Its window then reads the sealed text, and nothing reads the
//! deltas again, so they go in the transaction that seals it. A reply that failed or was stopped
//! keeps them: they are the only copy of what it showed.
use super::*;
use crate::session_purge::guards;

/// The events a stream writes for each delta it stores.
const KINDS: &str =
    "('agent.visible_delta','agent.visible_delta_index','agent.visible_delta_epoch')";

/// Drops the stream events of `step`, whose response is sealed, and the rows derived from them;
/// the number of events dropped. `None` while an older version's outbox copy is left: a copy
/// points at its event with no index on that column, so each event deleted would read the whole
/// outbox. The slimming that clears the outbox drops them after it.
pub fn drop_sealed(tx: &Transaction<'_>, step: ModelStepId) -> StoreResult<Option<u64>> {
    let outbox: bool =
        tx.query_row("SELECT EXISTS(SELECT 1 FROM projection_outbox)", [], |row| row.get(0))?;
    if outbox {
        return Ok(None);
    }
    let owner = step.to_string();
    let streamed: bool = tx.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM events WHERE owner=?1 AND kind IN {KINDS})"),
        [&owner],
        |row| row.get(0),
    )?;
    let mut dropped = 0;
    if streamed {
        // Lifted for these deletes only, and put back before the transaction ends.
        let lifted = guards::lift(tx, guards::EVENT_DELETE)?;
        dropped = tx
            .execute(&format!("DELETE FROM events WHERE owner=?1 AND kind IN {KINDS}"), [&owner])?;
        guards::restore(tx, lifted)?;
    }
    for table in [
        "agent_visible_delta",
        "agent_visible_index",
        "agent_visible_coverage",
        "agent_visible_repair",
    ] {
        tx.execute(&format!("DELETE FROM {table} WHERE owner=?1"), [&owner])?;
    }
    Ok(Some(dropped as u64))
}
