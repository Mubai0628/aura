//! Strong references a running Agent call holds on the package content it runs.
//! Collection never removes referenced content; a restarted Core releases what its
//! predecessor's calls held, because their Jobs ended with it.
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Transaction, params};

fn owner(operation: &str) -> String {
    format!("agent-call:{operation}")
}

pub fn retain_for_call(
    tx: &Transaction<'_>,
    fingerprint: &str,
    operation: &str,
) -> StoreResult<()> {
    let present: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM tool_package_payloads WHERE fingerprint=?1)",
        [fingerprint],
        |row| row.get(0),
    )?;
    if !present {
        return Err(Error::Invalid("package.content_unavailable"));
    }
    tx.execute(
        "INSERT INTO tool_package_references VALUES(?1,'execution',?2) ON CONFLICT DO NOTHING",
        params![fingerprint, owner(operation)],
    )?;
    Ok(())
}

pub fn release_for_call(
    tx: &Transaction<'_>,
    fingerprint: &str,
    operation: &str,
) -> StoreResult<()> {
    tx.execute(
        "DELETE FROM tool_package_references WHERE fingerprint=?1 AND owner_kind='execution' AND owner_id=?2",
        params![fingerprint, owner(operation)],
    )?;
    Ok(())
}

/// Startup only: every Agent call reference belonged to a process that has ended.
pub fn release_all_calls(tx: &Transaction<'_>) -> StoreResult<usize> {
    Ok(tx.execute(
        "DELETE FROM tool_package_references WHERE owner_kind='execution' AND owner_id LIKE 'agent-call:%'",
        [],
    )?)
}
