//! Only the current model catalog version is kept, with its raw response. Storing a version drops
//! the one it replaces in the same transaction; the run before a start drops what older versions
//! kept. Nothing reads a version but the current one.
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::PersistenceError;

fn current(connection: &Connection) -> Result<Option<String>, PersistenceError> {
    Ok(connection
        .query_row(
            "SELECT current_revision FROM provider_catalog_runtime_state WHERE catalog_key='models.dev'",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

/// The versions besides the current one; none while there is no current one.
pub(crate) fn replaced_versions(connection: &Connection) -> Result<u64, PersistenceError> {
    let Some(current) = current(connection)? else {
        return Ok(0);
    };
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM provider_catalog_snapshots WHERE catalog_revision != ?1",
        [current],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as u64)
}

/// Drops every version but the current one, then the raw responses no version references any
/// more: theirs and `also`, the one a version stored again replaced. The versions dropped, and the
/// object files to remove once `tx` commits.
pub(crate) fn drop_replaced(
    tx: &Transaction<'_>,
    artifact_root: &Path,
    also: Option<String>,
) -> Result<(u64, Vec<PathBuf>), PersistenceError> {
    let Some(current) = current(tx)? else {
        return Ok((0, Vec::new()));
    };
    let mut raw = {
        let mut statement = tx.prepare(
            "SELECT raw_artifact_id FROM provider_catalog_snapshots WHERE catalog_revision != ?1",
        )?;
        statement
            .query_map([&current], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    raw.extend(also);
    // The entries go with their version (`ON DELETE CASCADE`).
    let dropped = tx.execute(
        "DELETE FROM provider_catalog_snapshots WHERE catalog_revision != ?1",
        [&current],
    )?;
    let files = crate::artifact_gc::remove_unreferenced(tx, artifact_root, &raw)?;
    Ok((dropped as u64, files))
}
