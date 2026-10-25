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
    let digest = blake3::hash(include_bytes!("../src/memory.sql")).to_hex().to_string();
    assert_eq!(
        (CACHE_SCHEMA_VERSION, digest.as_str()),
        (1, "30a41e73618afe3f58bd5de33652cd85876fac0054ea0e58c02068448d35635d"),
        "bump CACHE_SCHEMA_VERSION together with memory.sql"
    );
}

#[test]
fn a_cache_of_another_version_or_partial_creation_is_recreated_not_migrated() {
    let temp = tempfile::tempdir().expect("TEMP");
    let scope = VaultScope::Project { workspace_id: WorkspaceId::new() };
    for (name, setup) in [
        // A cache created before the version stamp existed (user_version 0).
        ("unversioned.db", include_str!("../src/memory.sql").to_owned()),
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
