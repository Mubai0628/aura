//! Removing artifacts: their rows inside the caller's transaction, then the object files no
//! artifact uses any more, once that transaction committed. An object file another artifact still
//! uses stays.
use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{OptionalExtension, Transaction};

use crate::{PersistenceError, artifact::resolve_object_path};

/// Deletes the artifact inside `tx`; the object files nothing uses after it, to remove once `tx`
/// commits.
pub(crate) fn remove_artifact(
    tx: &Transaction<'_>,
    artifact_root: &Path,
    artifact_id: &str,
) -> Result<Vec<PathBuf>, PersistenceError> {
    let object_hashes = {
        let mut statement =
            tx.prepare("SELECT object_hash FROM artifact_chunks WHERE artifact_id=?1")?;
        statement
            .query_map([artifact_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    tx.execute("DELETE FROM artifacts WHERE artifact_id=?1", [artifact_id])?;
    let mut unused = Vec::new();
    for object_hash in object_hashes {
        tx.execute(
            "UPDATE artifact_object_refs SET ref_count=ref_count-1 WHERE object_hash=?1",
            [&object_hash],
        )?;
        let remaining = tx
            .query_row(
                "SELECT ref_count FROM artifact_object_refs WHERE object_hash=?1",
                [&object_hash],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if remaining == Some(0) {
            let relative = tx.query_row(
                "SELECT relative_path FROM artifact_object_refs WHERE object_hash=?1",
                [&object_hash],
                |row| row.get::<_, String>(0),
            )?;
            let object_path = resolve_object_path(artifact_root, &relative, &object_hash)?;
            tx.execute("DELETE FROM artifact_object_refs WHERE object_hash=?1", [&object_hash])?;
            unused.push(object_path);
        }
    }
    Ok(unused)
}

/// Removes the files `remove_artifact` named: how many went, and how many could not.
pub(crate) fn remove_files(paths: Vec<PathBuf>) -> (usize, usize) {
    let (mut removed, mut remaining) = (0, 0);
    for path in paths {
        match fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => remaining += 1,
        }
    }
    (removed, remaining)
}

/// Deletes those of `artifact_ids` that no catalog version, conversation or open quarantine record
/// references. One that another record still points at stays: its delete fails that record's
/// foreign key and is undone alone. The object files to remove once `tx` commits.
pub(crate) fn remove_unreferenced(
    tx: &Transaction<'_>,
    artifact_root: &Path,
    artifact_ids: &[String],
) -> Result<Vec<PathBuf>, PersistenceError> {
    let mut files = Vec::new();
    for artifact_id in artifact_ids {
        let referenced: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_catalog_snapshots WHERE raw_artifact_id=?1)
                 OR EXISTS(SELECT 1 FROM artifact_access_links WHERE artifact_id=?1)
                 OR EXISTS(SELECT 1 FROM quarantine_records
                           WHERE artifact_id=?1 AND state!='discarded')",
            [artifact_id],
            |row| row.get(0),
        )?;
        if referenced {
            continue;
        }
        tx.execute_batch("SAVEPOINT remove_artifact")?;
        match remove_artifact(tx, artifact_root, artifact_id) {
            Ok(unused) => {
                tx.execute_batch("RELEASE remove_artifact")?;
                files.extend(unused);
            }
            Err(PersistenceError::Sqlite(rusqlite::Error::SqliteFailure(error, _)))
                if error.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                tx.execute_batch("ROLLBACK TO remove_artifact; RELEASE remove_artifact")?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(files)
}
