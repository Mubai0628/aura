//! Memory indexes are per-Vault caches. The shared V2 database never holds
//! them, so opening an older database needs no migration at all.
use aura_domain_common::WorkspaceId;
use aura_memory_domain::VaultScope;
use aura_persistence_v2::{
    V2Store,
    memory::{CACHE_SCHEMA_VERSION, MemoryIndex},
};
use rusqlite::Connection;

fn inventory(c: &Connection) -> Vec<(String, String)> {
    c.prepare("SELECT type,name FROM sqlite_master ORDER BY type,name")
        .expect("schema")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("names")
}
fn owners(c: &Connection) -> Vec<(String, String)> {
    c.prepare("SELECT owner,digest FROM v2_schema_metadata ORDER BY owner")
        .expect("metadata")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("owners")
}

#[test]
fn mixed_length_queries_find_old_short_terms_and_synonyms_in_the_body()
-> Result<(), Box<dyn std::error::Error>> {
    use aura_memory_domain::{MemoryDocument, MemoryMetadata, MemoryScope};
    use aura_persistence_v2::memory::{IndexedFile, MemorySearch};
    let temp = tempfile::tempdir()?;
    let scope = VaultScope::Project { workspace_id: WorkspaceId::new() };
    let mut index = MemoryIndex::open(&temp.path().join("vault.db"), "test-vault", scope)?;
    let note = |path: String, body: &str, old: bool| {
        let mut metadata = MemoryMetadata::new(MemoryScope::Project, "notes".into());
        if old {
            metadata.updated_at = "2020-01-01T00:00:00Z".parse().expect("fixture timestamp");
        }
        IndexedFile {
            path,
            hash: "sample".into(),
            note: Some(MemoryDocument { metadata, body: body.into() }),
            invalid_reason: None,
        }
    };
    let mut files = vec![note("first.md".into(), "# 资料\n手册里有封面和题名。", true)];
    files.extend((0..140).map(|i| note(format!("new-{i}.md"), "# 其他\n无关", false)));
    index.replace_all(&files)?;
    let request = MemorySearch { query: "unmatched 封面 题名".into(), ..Default::default() };
    let hits = index.search_related(&request)?.hits;
    assert_eq!(hits.first().map(|hit| hit.path.as_str()), Some("first.md"));
    let synonym = MemorySearch { query: "指南".into(), ..Default::default() };
    assert_eq!(index.search_related(&synonym)?.hits[0].path, "first.md");
    index.update_files(&[note("first.md".into(), "# 资料\n更新后的无关文字。", false)], &[])?;
    assert!(index.search_related(&request)?.hits.is_empty());
    assert!(index.search_related(&synonym)?.hits.is_empty());
    Ok(())
}

#[test]
fn the_shared_database_holds_no_memory_tables_or_owner() {
    let temp = tempfile::tempdir().expect("TEMP");
    let store = V2Store::create(&temp.path().join("v2")).expect("new store");
    let c = store.database.connection();
    assert!(inventory(c).iter().all(|(_, name)| !name.starts_with("memory_")));
    assert!(owners(c).iter().all(|(owner, _)| owner != "memory"));
}

#[test]
fn cache_schema_version_tracks_memory_sql() {
    // Changing memory.sql without bumping CACHE_SCHEMA_VERSION would keep old
    // caches whose structure no longer matches the queries.
    let digest = blake3::Hasher::new()
        .update(include_bytes!("../../src/memory.sql"))
        .update(include_bytes!("../../src/memory/search_index.sql"))
        .finalize()
        .to_hex()
        .to_string();
    assert_eq!(
        (CACHE_SCHEMA_VERSION, digest.as_str()),
        (2, "4ae52805c4ad015ab0dce02d51d95a228d9c3fd89de5d7fe0652307ccd62f52b"),
        "bump CACHE_SCHEMA_VERSION together with memory.sql"
    );
}

#[test]
fn a_v1_vault_upgrades_its_index_only_after_checking_ownership()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("old.db");
    let workspace_id = WorkspaceId::new();
    let scope = VaultScope::Project { workspace_id };
    let db = Connection::open(&path)?;
    db.execute_batch(include_str!("../../src/memory.sql"))?;
    db.execute_batch("CREATE VIRTUAL TABLE memory_fts USING fts5(relative_path UNINDEXED,title,body,tokenize='trigram');
                      PRAGMA user_version=1; INSERT INTO memory_fts VALUES('old.md','old','早期的题名');")?;
    db.execute(
        "INSERT INTO memory_vaults VALUES('right',1,'project',?1,NULL,'ready')",
        [workspace_id.to_string()],
    )?;
    db.execute("INSERT INTO memory_history(record_hash,relative_path,action,actor,title,at) VALUES('record','old.md','edit','user','old','2026-01-01')", [])?;
    drop(db);
    assert!(MemoryIndex::open(&path, "wrong", scope).is_err());
    let db = Connection::open(&path)?;
    assert_eq!(db.pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))?, 1);
    drop(db);
    drop(MemoryIndex::open(&path, "right", scope)?);
    let db = Connection::open(&path)?;
    let file: String = db.query_row(
        "SELECT relative_path FROM memory_fts WHERE memory_fts MATCH 'short_terms:\"_题名\"'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(file, "old.md");
    assert_eq!(
        db.query_row("SELECT record_hash FROM memory_history", [], |r| r.get::<_, String>(0))?,
        "record"
    );
    Ok(())
}

#[test]
fn a_cache_of_another_version_or_partial_creation_is_recreated_not_migrated() {
    let temp = tempfile::tempdir().expect("TEMP");
    let scope = VaultScope::Project { workspace_id: WorkspaceId::new() };
    for (name, setup) in [
        // A cache created before the version stamp existed (user_version 0).
        ("unversioned.db", include_str!("../../src/memory.sql").to_owned()),
        // A different structure carrying the current number cannot exist, but an
        // interrupted creation can: some tables and no stamp.
        ("partial.db", "CREATE TABLE memory_entries(relative_path TEXT);".to_owned()),
        ("future.db", "CREATE TABLE later(value TEXT); PRAGMA user_version=99;".to_owned()),
    ] {
        let path = temp.path().join(name);
        let c = Connection::open(&path).expect("fixture");
        c.execute_batch(&setup).expect("fixture schema");
        c.execute_batch("CREATE TABLE stray(value TEXT); INSERT INTO stray VALUES('x');")
            .expect("stray");
        drop(c);
        drop(MemoryIndex::open(&path, "vault-identity", scope).expect("recreated cache"));
        let c = Connection::open(&path).expect("reopen");
        let version: i32 =
            c.pragma_query_value(None, "user_version", |r| r.get(0)).expect("version");
        assert_eq!(version, CACHE_SCHEMA_VERSION, "{name}");
        assert!(inventory(&c).iter().all(|(_, n)| n != "stray" && n != "later"), "{name}");
        let vaults: i64 =
            c.query_row("SELECT count(*) FROM memory_vaults", [], |r| r.get(0)).expect("vault");
        assert_eq!(vaults, 1, "{name}");
        drop(c);
        // A current cache is reused as it is, and still belongs to one Vault only.
        drop(MemoryIndex::open(&path, "vault-identity", scope).expect("current cache"));
        assert!(MemoryIndex::open(&path, "another-vault", scope).is_err(), "{name}");
    }
}

#[test]
#[ignore = "requires the authorized TEMP COPY in AURA_L05_OWNER_COPY"]
fn owner_copy_opens_without_any_schema_change() {
    let root =
        std::fs::canonicalize(std::env::var_os("AURA_L05_OWNER_COPY").expect("explicit COPY path"))
            .expect("copy");
    assert!(
        root.starts_with(std::fs::canonicalize(std::env::temp_dir()).expect("TEMP"))
            && root
                .file_name()
                .expect("leaf")
                .to_string_lossy()
                .starts_with("aura-l05-owner-copy-")
    );
    let reader = aura_persistence_core::open_reader(&root.join("aura-v2.db")).expect("copy reader");
    let (schema, metadata) = (inventory(&reader), owners(&reader));
    let events: i64 =
        reader.query_row("SELECT count(*) FROM events", [], |r| r.get(0)).expect("events count");
    drop(reader);
    let store = V2Store::open(&root).expect("open copy");
    let c = store.database.connection();
    assert_eq!(inventory(c), schema, "no table, index or trigger added or removed");
    assert_eq!(owners(c), metadata, "no module digest changed");
    let after: i64 = c.query_row("SELECT count(*) FROM events", [], |r| r.get(0)).expect("events");
    assert_eq!(after, events);
    println!(
        "L05 owner COPY: {} schema objects and {} module digests unchanged; events {events}->{after}",
        schema.len(),
        metadata.len()
    );
    drop(store);
    V2Store::open(&root).expect("normal restart");
}
