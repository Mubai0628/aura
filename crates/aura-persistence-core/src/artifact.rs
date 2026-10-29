use crate::{PersistenceCoreError as Error, StoreResult};
use aura_domain_common::{ArtifactId, UtcTimestamp};
use rusqlite::{Connection, Transaction, params};

/// Content-addressed SQLite objects share the same transaction as owners and
/// state. There is no filesystem object path for callers to escape or orphan.
pub fn put(
    tx: &Transaction<'_>,
    id: ArtifactId,
    owner: &str,
    media_type: &str,
    bytes: &[u8],
    at: UtcTimestamp,
) -> StoreResult<()> {
    if owner.is_empty() || media_type.is_empty() {
        return Err(Error::Invalid("artifact owner/media type"));
    }
    let hash = blake3::hash(bytes).to_hex().to_string();
    tx.execute(
        "INSERT INTO artifact_objects VALUES(?1,?2,?3) ON CONFLICT(hash) DO NOTHING",
        params![hash, bytes.len() as i64, bytes],
    )?;
    let stored: Vec<u8> =
        tx.query_row("SELECT bytes FROM artifact_objects WHERE hash=?1", [&hash], |r| r.get(0))?;
    if stored != bytes {
        return Err(Error::Invalid("artifact object digest"));
    }
    tx.execute(
        "INSERT INTO artifacts VALUES(?1,?2,?3,?4)",
        params![id.to_string(), hash, media_type, at.to_string()],
    )?;
    tx.execute("INSERT INTO artifact_owners VALUES(?1,?2)", params![id.to_string(), owner])?;
    Ok(())
}

pub fn grant_access(
    tx: &Transaction<'_>,
    id: ArtifactId,
    owner: &str,
    subject: &str,
) -> StoreResult<()> {
    let owns: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM artifact_owners WHERE artifact_id=?1 AND owner=?2)",
        params![id.to_string(), owner],
        |r| r.get(0),
    )?;
    if !owns || subject.is_empty() {
        return Err(Error::Invalid("artifact direct owner"));
    }
    tx.execute(
        "INSERT INTO artifact_access_links VALUES(?1,?2) ON CONFLICT DO NOTHING",
        params![id.to_string(), subject],
    )?;
    Ok(())
}

/// Subject comes from Core's checked domain context, never a renderer string.
pub fn read(connection: &Connection, id: ArtifactId, subject: &str) -> StoreResult<Vec<u8>> {
    permit(connection, id, subject)?;
    let (hash, bytes): (String, Vec<u8>) = connection.query_row(
        "SELECT o.hash,o.bytes FROM artifacts a JOIN artifact_objects o ON o.hash=a.object_hash WHERE a.id=?1",
        [id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if blake3::hash(&bytes).to_hex().as_str() != hash {
        return Err(Error::Invalid("artifact corruption"));
    }
    Ok(bytes)
}

/// The digest recorded for an artifact, after the same access check as [`read`].
///
/// `put` stores the digest only once the bytes match it. A page can bind to that digest
/// without loading the bytes; [`read`] still recomputes the digest when a caller needs the
/// bytes themselves.
pub fn identity(
    connection: &Connection,
    id: ArtifactId,
    subject: &str,
) -> StoreResult<aura_domain_common::ContentHash> {
    permit(connection, id, subject)?;
    let hash: String = connection.query_row(
        "SELECT o.hash FROM artifacts a JOIN artifact_objects o ON o.hash=a.object_hash WHERE a.id=?1",
        [id.to_string()],
        |row| row.get(0),
    )?;
    aura_domain_common::ContentHash::new(aura_domain_common::ContentHashAlgorithm::Blake3, hash)
        .map_err(|_| Error::Invalid("artifact object digest"))
}

fn permit(connection: &Connection, id: ArtifactId, subject: &str) -> StoreResult<()> {
    let allowed: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM artifact_owners WHERE artifact_id=?1 AND owner=?2 \
         UNION ALL SELECT 1 FROM artifact_access_links WHERE artifact_id=?1 AND subject=?2)",
        params![id.to_string(), subject],
        |row| row.get(0),
    )?;
    if !allowed {
        return Err(Error::Invalid("artifact access"));
    }
    Ok(())
}

pub fn release_owner(tx: &Transaction<'_>, id: ArtifactId, owner: &str) -> StoreResult<()> {
    tx.execute(
        "DELETE FROM artifact_owners WHERE artifact_id=?1 AND owner=?2",
        params![id.to_string(), owner],
    )?;
    tx.execute("INSERT INTO artifact_gc_queue SELECT id FROM artifacts WHERE id=?1 AND NOT EXISTS(SELECT 1 FROM artifact_owners WHERE artifact_id=?1) ON CONFLICT DO NOTHING",[id.to_string()])?;
    Ok(())
}

/// Deletes the queued artifacts nothing owns or links, and the objects only they held.
///
/// Only the objects of the artifacts deleted here are checked. Checking every object against
/// every artifact cost O(n²) in the store's artifacts, and a terminal's output is collected on
/// every save: 24 s per save beside 20 000 artifacts.
///
/// The queue is read first and each artifact found by its key: the queue holds what one change
/// gave up, and without statistics SQLite chose to walk every artifact and look each one up in
/// the queue instead.
pub fn collect(tx: &Transaction<'_>) -> StoreResult<usize> {
    let mut query = tx.prepare(
        "SELECT a.id,a.object_hash FROM artifact_gc_queue q CROSS JOIN artifacts a \
         ON a.id=q.artifact_id \
         WHERE NOT EXISTS(SELECT 1 FROM artifact_owners WHERE artifact_id=a.id) \
         AND NOT EXISTS(SELECT 1 FROM operation_artifact_links WHERE artifact_id=a.id)",
    )?;
    let unused = query
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(query);
    for (id, _) in &unused {
        tx.execute("DELETE FROM artifacts WHERE id=?1", [id])?;
    }
    let hashes: std::collections::BTreeSet<&String> = unused.iter().map(|(_, hash)| hash).collect();
    if !hashes.is_empty() {
        // One pass over the artifacts for all the objects, however many the deleted ones held.
        tx.execute(
            "DELETE FROM artifact_objects WHERE hash IN (SELECT value FROM json_each(?1)) \
             AND hash NOT IN (SELECT object_hash FROM artifacts)",
            [serde_json::to_string(&hashes)?],
        )?;
    }
    Ok(unused.len())
}
