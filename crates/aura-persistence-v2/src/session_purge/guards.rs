//! The store refuses to delete an audit event or a terminal execution result. A purge lifts those
//! two guards inside its own transaction and puts back exactly the statements it removed, so the
//! schema the store verifies on open is unchanged and every other path stays refused. If the
//! purge fails, the transaction rolls back with the guards still in place.
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Transaction;

const GUARDS: [&str; 2] = ["immutable_event_delete", "immutable_execution_result_delete"];

pub(super) struct Lifted(Vec<String>);

pub(super) fn lift(tx: &Transaction<'_>) -> StoreResult<Lifted> {
    let mut statements = Vec::with_capacity(GUARDS.len());
    for name in GUARDS {
        let statement: String = tx.query_row(
            "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?1",
            [name],
            |row| row.get(0),
        )?;
        tx.execute_batch(&format!("DROP TRIGGER {name}"))?;
        statements.push(statement);
    }
    Ok(Lifted(statements))
}

pub(super) fn restore(tx: &Transaction<'_>, lifted: Lifted) -> StoreResult<()> {
    for statement in lifted.0 {
        tx.execute_batch(&statement)?;
    }
    for name in GUARDS {
        let present: i64 = tx.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name=?1",
            [name],
            |row| row.get(0),
        )?;
        if present != 1 {
            return Err(Error::Invalid("delete guard was not restored"));
        }
    }
    Ok(())
}
