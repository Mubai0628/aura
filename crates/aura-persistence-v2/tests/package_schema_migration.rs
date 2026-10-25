//! Frozen predecessors before the package tables -> the package tables, including refusal before writing.
use aura_persistence_core::{DatabaseConnection, schema};
use aura_persistence_v2::{MODULES, V2Store};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

const BEFORE: &str = include_str!("fixtures/tool-before-l04.sql");
const B1: &str = include_str!("fixtures/tool-before-l04-b2.sql");
const TABLES: &[&str] = &[
    "tool_packages",
    "tool_package_versions",
    "tool_package_payloads",
    "tool_package_files",
    "tool_package_references",
    "tool_package_trust",
    "tool_package_deletions",
    "tool_package_configurations",
    "tool_package_credential_bindings",
];

fn predecessor() -> (tempfile::TempDir, PathBuf) {
    predecessor_with(BEFORE)
}

fn predecessor_with(tool: &'static str) -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("temporary namespace");
    let root = temp.path().join("v2");
    std::fs::create_dir(&root).expect("root");
    let mut database = DatabaseConnection::create_new(&root.join("aura-v2.db")).expect("db");
    let modules = MODULES
        .iter()
        .map(|module| schema::SchemaModule {
            owner: module.owner,
            sql: if module.owner == "tool" { tool } else { module.sql },
        })
        .collect::<Vec<_>>();
    schema::initialize(&mut database, &modules).expect("frozen schema");
    database.connection().execute(
        "INSERT INTO events(event_id,owner,kind,version,payload,created_at) VALUES('keep-l04','test','note',1,'{}','2026-10-15T00:00:00Z')", [],
    ).expect("existing fact");
    drop(database);
    std::fs::write(
        root.join("v2-development.json"),
        b"{\"schema_version\":1,\"namespace\":\"aura-v2-development\"}\n",
    )
    .expect("namespace");
    (temp, root)
}

fn reader(root: &Path) -> Connection {
    Connection::open_with_flags(root.join("aura-v2.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("read fixture")
}

fn digest(connection: &Connection) -> String {
    connection
        .query_row("SELECT digest FROM v2_schema_metadata WHERE owner='tool'", [], |row| row.get(0))
        .expect("tool digest")
}

fn new_tables(connection: &Connection) -> usize {
    TABLES
        .iter()
        .filter(|table| {
            connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [**table],
                    |row| row.get::<_, i64>(0),
                )
                .expect("catalog")
                == 1
        })
        .count()
}

#[test]
fn fresh_namespace_has_empty_package_authorities_and_reference_constraints() {
    let temp = tempfile::tempdir().expect("temp");
    let store = V2Store::create(&temp.path().join("v2")).expect("create");
    let connection = store.database.connection();
    assert_eq!(new_tables(connection), TABLES.len(), "L04 tables are owned by V2");
    for table in TABLES {
        // Table names are this test's constants, never a package-supplied value.
        let count: i64 = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
            .expect("empty table");
        assert_eq!(count, 0, "no legacy data copied into {table}");
    }
    assert!(
        connection
            .execute(
                "INSERT INTO tool_package_references VALUES('missing','version','missing')",
                []
            )
            .is_err(),
        "a reference cannot invent content"
    );
}

#[test]
fn frozen_predecessor_upgrades_once_without_rewriting_existing_facts() {
    let (_temp, root) = predecessor();
    let before = digest(&reader(&root));
    assert_eq!(before, blake3::hash(BEFORE.as_bytes()).to_hex().as_str());
    assert_eq!(new_tables(&reader(&root)), 0);
    let store = V2Store::open(&root).expect("supported upgrade");
    assert_eq!(new_tables(store.database.connection()), TABLES.len());
    assert_ne!(digest(store.database.connection()), before);
    let payload: String = store
        .database
        .connection()
        .query_row("SELECT payload FROM events WHERE event_id='keep-l04'", [], |row| row.get(0))
        .expect("existing event");
    assert_eq!(payload, "{}");
    let after = digest(store.database.connection());
    drop(store);
    assert_eq!(digest(V2Store::open(&root).expect("reopen").database.connection()), after);
}

#[test]
fn unknown_or_counterfeit_predecessor_is_refused_before_creating_package_tables() {
    for counterfeit in [false, true] {
        let (_temp, root) = predecessor();
        let writer = Connection::open(root.join("aura-v2.db")).expect("fixture writer");
        if counterfeit {
            writer
                .execute("ALTER TABLE tool_instances ADD COLUMN unreviewed TEXT", [])
                .expect("drift");
        } else {
            writer
                .execute("UPDATE v2_schema_metadata SET digest='future-l04' WHERE owner='tool'", [])
                .expect("future digest");
        }
        let before = digest(&writer);
        drop(writer);
        let bytes_before = std::fs::read(root.join("aura-v2.db")).expect("before bytes");
        assert!(V2Store::open(&root).is_err(), "unknown schema cannot partially migrate");
        assert_eq!(new_tables(&reader(&root)), 0);
        assert_eq!(digest(&reader(&root)), before);
        assert_eq!(std::fs::read(root.join("aura-v2.db")).expect("after bytes"), bytes_before);
    }
}

#[test]
fn b1_predecessor_keeps_imported_packages_and_adds_settings_and_bindings() {
    let (_temp, root) = predecessor_with(B1);
    let writer = Connection::open(root.join("aura-v2.db")).expect("fixture writer");
    writer
        .execute("INSERT INTO tool_packages(id,package_key,revision) VALUES('tpk_01ARZ3NDEKTSV4RRFFQ69G5FAV','org.example.kept',1)", [])
        .expect("B1 package row");
    drop(writer);
    assert_eq!(digest(&reader(&root)), blake3::hash(B1.as_bytes()).to_hex().as_str());
    assert_eq!(new_tables(&reader(&root)), TABLES.len() - 2, "B1 has only the content tables");
    let store = V2Store::open(&root).expect("supported B1 upgrade");
    assert_eq!(new_tables(store.database.connection()), TABLES.len());
    let key: String = store
        .database
        .connection()
        .query_row("SELECT package_key FROM tool_packages", [], |row| row.get(0))
        .expect("kept package");
    assert_eq!(key, "org.example.kept");
    let current = digest(store.database.connection());
    drop(store);
    assert_eq!(digest(V2Store::open(&root).expect("reopen").database.connection()), current);
}
