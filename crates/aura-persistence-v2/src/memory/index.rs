use aura_domain_common::UtcTimestamp;
use aura_memory_domain::{MemoryDocument, VaultScope};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

/// Bump with every change to `memory.sql`. A cache is derived from the Vault's
/// files and the metadata records beside it, so a cache of another version is
/// discarded and rebuilt instead of migrated.
pub const CACHE_SCHEMA_VERSION: i32 = 1;

pub struct MemoryIndex {
    pub(super) connection: Connection,
}

#[derive(Clone, Debug)]
pub struct IndexedFile {
    pub path: String,
    pub hash: String,
    pub note: Option<MemoryDocument>,
    pub invalid_reason: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct VaultIndexStatus {
    pub valid: u64,
    pub invalid: u64,
    pub rebuilding: bool,
    pub watcher_error: Option<String>,
}

impl MemoryIndex {
    pub fn open_reader(cache: &Path) -> StoreResult<Self> {
        Ok(Self { connection: aura_persistence_core::open_reader(cache)? })
    }
    /// The cache contains exactly one Vault identity. An existing cache for a
    /// different Vault is refused instead of becoming a cross-project index.
    pub fn open(cache: &Path, vault_id: &str, scope: VaultScope) -> StoreResult<Self> {
        let mut connection = current_cache(cache)?;
        let tx = connection.transaction()?;
        let current: Option<String> =
            tx.query_row("SELECT vault_id FROM memory_vaults", [], |r| r.get(0)).optional()?;
        if current.as_deref().is_some_and(|id| id != vault_id) {
            return Err(Error::Invalid("independent Vault cache identity"));
        }
        let (kind, workspace) = match scope {
            VaultScope::Global => ("global", None),
            VaultScope::Project { workspace_id } => ("project", Some(workspace_id.to_string())),
        };
        tx.execute(
            "INSERT OR IGNORE INTO memory_vaults VALUES(?1,1,?2,?3,NULL,'ready')",
            params![vault_id, kind, workspace],
        )?;
        let live: (String, Option<String>) =
            tx.query_row("SELECT scope,workspace_id FROM memory_vaults", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        if live != (kind.to_owned(), workspace) {
            return Err(Error::Invalid("Vault scope"));
        }
        tx.commit()?;
        Ok(Self { connection })
    }

    pub fn set_state(&self, state: &str, identity: Option<&str>) -> StoreResult<()> {
        self.connection.execute(
            "UPDATE memory_vaults SET state=?1, root_identity_json=COALESCE(?2,root_identity_json)",
            params![state, identity],
        )?;
        Ok(())
    }

    pub fn replace_all(&mut self, files: &[IndexedFile]) -> StoreResult<()> {
        let tx = self.connection.transaction()?;
        tx.execute("DELETE FROM memory_links", [])?;
        tx.execute("DELETE FROM memory_entries", [])?;
        tx.execute("DELETE FROM memory_fts", [])?;
        for file in files {
            put(&tx, file)?;
        }
        mark_duplicates(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Change only the watcher delta; duplicate IDs are re-evaluated as a group.
    pub fn update_files(&mut self, files: &[IndexedFile], removed: &[String]) -> StoreResult<()> {
        let tx = self.connection.transaction()?;
        for path in removed.iter().map(String::as_str).chain(files.iter().map(|f| f.path.as_str()))
        {
            tx.execute("DELETE FROM memory_entries WHERE relative_path=?1", [path])?;
            tx.execute("DELETE FROM memory_fts WHERE relative_path=?1", [path])?;
        }
        for file in files {
            put(&tx, file)?;
        }
        mark_duplicates(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn files(&self) -> StoreResult<Vec<(String, String)>> {
        let mut s = self.connection.prepare(
            "SELECT relative_path,content_hash FROM memory_entries ORDER BY relative_path",
        )?;
        Ok(s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?)
    }
    pub fn path_for_id(&self, id: &str) -> StoreResult<Option<String>> {
        let mut s = self.connection.prepare(
            "SELECT relative_path FROM memory_entries WHERE memory_id=?1 AND valid=1 LIMIT 2",
        )?;
        let paths: Vec<String> = s.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(if paths.len() == 1 { paths.into_iter().next() } else { None })
    }
    pub fn contains_id(&self, id: &str) -> StoreResult<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_entries WHERE memory_id=?1)",
            [id],
            |r| r.get(0),
        )?)
    }
    pub fn id_count(&self, id: &str) -> StoreResult<i64> {
        Ok(self.connection.query_row(
            "SELECT count(*) FROM memory_entries WHERE memory_id=?1",
            [id],
            |r| r.get(0),
        )?)
    }
    pub fn status(&self) -> StoreResult<VaultIndexStatus> {
        let valid:i64 = self.connection.query_row("SELECT count(*) FROM memory_entries WHERE valid=1 AND status NOT IN ('forgotten','archived','superseded')", [], |r| r.get(0))?;
        let invalid: i64 = self.connection.query_row(
            "SELECT count(*) FROM memory_entries WHERE valid=0",
            [],
            |r| r.get(0),
        )?;
        let state: String =
            self.connection.query_row("SELECT state FROM memory_vaults", [], |r| r.get(0))?;
        Ok(VaultIndexStatus {
            valid: valid as u64,
            invalid: invalid as u64,
            rebuilding: state == "rebuilding",
            watcher_error: state.strip_prefix("watcher_error:").map(str::to_owned),
        })
    }
    pub fn valid_file_count(&self) -> StoreResult<u64> {
        let count: i64 = self.connection.query_row(
            "SELECT count(*) FROM memory_entries WHERE valid=1",
            [],
            |r| r.get(0),
        )?;
        Ok(count as u64)
    }
    pub fn invalid_files(&self) -> StoreResult<Vec<(String, String)>> {
        let mut s = self.connection.prepare("SELECT relative_path,invalid_reason FROM memory_entries WHERE valid=0 ORDER BY relative_path")?;
        Ok(s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?)
    }
}

fn connect(cache: &Path) -> StoreResult<Connection> {
    let connection = Connection::open(cache)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    Ok(connection)
}

/// Opens a cache of the current schema. Any other cache (an earlier schema, a
/// partial creation or an unrelated file) is removed and created again; the next
/// rebuild derives the same rows from files, so nothing authoritative is lost.
fn current_cache(cache: &Path) -> StoreResult<Connection> {
    let connection = connect(cache)?;
    let version: i32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let objects: i64 =
        connection.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
    if objects > 0 && version == CACHE_SCHEMA_VERSION {
        return Ok(connection);
    }
    let connection = if objects > 0 {
        drop(connection);
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut path = cache.as_os_str().to_owned();
            path.push(suffix);
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        connect(cache)?
    } else {
        connection
    };
    connection.execute_batch(&format!(
        "BEGIN;\n{}\nPRAGMA user_version={CACHE_SCHEMA_VERSION};\nCOMMIT;",
        include_str!("../memory.sql")
    ))?;
    Ok(connection)
}

fn put(tx: &Connection, file: &IndexedFile) -> StoreResult<()> {
    let at = UtcTimestamp::now().to_string();
    let Some(note) = &file.note else {
        tx.execute("INSERT INTO memory_entries(relative_path,title,content_hash,valid,invalid_reason,indexed_at) VALUES(?1,'',?2,0,?3,?4)", params![file.path,file.hash,file.invalid_reason.as_deref().unwrap_or("invalid note"),at])?;
        return Ok(());
    };
    let m = &note.metadata;
    let status =
        serde_json::to_value(m.status)?.as_str().ok_or(Error::Invalid("memory status"))?.to_owned();
    tx.execute(
        "INSERT INTO memory_entries VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,1,NULL,?11,?12)",
        params![
            file.path,
            m.id.to_string(),
            note.title(),
            m.category,
            status,
            m.importance,
            m.confidence,
            m.recall_weight,
            m.updated_at.to_string(),
            file.hash,
            serde_json::to_string(m)?,
            at
        ],
    )?;
    tx.execute(
        "INSERT INTO memory_fts(relative_path,title,body) VALUES(?1,?2,?3)",
        params![file.path, note.title(), note.body],
    )?;
    for (kind, ids) in [
        ("related", &m.related_ids),
        ("supersedes", &m.supersedes_ids),
        ("conflicts", &m.conflicts_with_ids),
    ] {
        for id in ids {
            tx.execute(
                "INSERT OR IGNORE INTO memory_links VALUES(?1,?2,?3)",
                params![file.path, id.to_string(), kind],
            )?;
        }
    }
    // Wiki-links are local names, never a request to open another Vault.
    for link in note.body.split("[[").skip(1).filter_map(|p| p.split_once("]]").map(|(l, _)| l)) {
        tx.execute(
            "INSERT OR IGNORE INTO memory_links VALUES(?1,?2,'wikilink')",
            params![file.path, link],
        )?;
    }
    Ok(())
}

fn mark_duplicates(tx: &Connection) -> StoreResult<()> {
    tx.execute("UPDATE memory_entries SET valid=1,invalid_reason=NULL WHERE invalid_reason='missing or ambiguous local relation'",[])?;
    tx.execute("UPDATE memory_entries SET valid=1,invalid_reason=NULL WHERE invalid_reason='duplicate memory id'", [])?;
    tx.execute("UPDATE memory_entries SET valid=0,invalid_reason='duplicate memory id' WHERE memory_id IN (SELECT memory_id FROM memory_entries WHERE memory_id IS NOT NULL GROUP BY memory_id HAVING count(*)>1)", [])?;
    tx.execute("UPDATE memory_entries SET valid=0,invalid_reason='missing or ambiguous local relation' WHERE valid=1 AND relative_path IN (SELECT source_path FROM memory_links l WHERE relation IN ('related','supersedes','conflicts') AND (SELECT count(*) FROM memory_entries e WHERE e.memory_id=l.target_id)<>1)",[])?;
    Ok(())
}
