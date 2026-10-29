//! The store refuses to change an audit event or delete a terminal execution result. A purge, the
//! slimming of what older versions wrote (`crate::slimming`), or a seal dropping the reply's
//! streamed deltas (`crate::agent::stream::drop_sealed`) lifts the guards it needs inside its own
//! transaction and puts back exactly the statements it removed, so the schema the store verifies
//! on open is unchanged and every other path stays refused. If the work fails, the transaction
//! rolls back with the guards still in place.
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Transaction;

/// What a purge deletes past.
pub(crate) const PURGE: &[&str] = &["immutable_event_delete", "immutable_execution_result_delete"];
/// What a slimming rewrites past: an older version's `model.transport` events.
pub(crate) const EVENT_UPDATE: &[&str] = &["immutable_event_update"];
/// What dropping a sealed reply's streamed deltas deletes past.
pub(crate) const EVENT_DELETE: &[&str] = &["immutable_event_delete"];

pub(crate) struct Lifted {
    names: &'static [&'static str],
    statements: Vec<String>,
}

pub(crate) fn lift(tx: &Transaction<'_>, names: &'static [&'static str]) -> StoreResult<Lifted> {
    let mut statements = Vec::with_capacity(names.len());
    for name in names {
        let statement: String = tx.query_row(
            "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?1",
            [name],
            |row| row.get(0),
        )?;
        tx.execute_batch(&format!("DROP TRIGGER {name}"))?;
        statements.push(statement);
    }
    Ok(Lifted { names, statements })
}

pub(crate) fn restore(tx: &Transaction<'_>, lifted: Lifted) -> StoreResult<()> {
    for statement in lifted.statements {
        tx.execute_batch(&statement)?;
    }
    for name in lifted.names {
        let present: i64 = tx.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name=?1",
            [name],
            |row| row.get(0),
        )?;
        if present != 1 {
            return Err(Error::Invalid("store guard was not restored"));
        }
    }
    Ok(())
}
