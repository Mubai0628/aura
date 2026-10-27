//! Opening a database identifies it before any write.
//!
//! An unknown agent digest, or a file that is not this application, is refused
//! with the digest and tables left as they were. A supported predecessor is
//! migrated in one transaction. A write that cannot finish leaves that predecessor.
#![allow(clippy::unwrap_used)]

use aura_persistence_core::{DatabaseConnection, schema};
use aura_persistence_v2::{MODULES, V2Store};
use rusqlite::Connection;

fn predecessor(marker: &str) -> String {
    let sql = include_str!("../src/agent.sql");
    let (head, _) = sql.split_once(marker).expect("predecessor marker");
    format!("{head}\n")
}

fn static_sql(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn write_marker(directory: &std::path::Path) {
    std::fs::write(
        directory.join("v2-development.json"),
        b"{\"schema_version\":1,\"namespace\":\"aura-v2-development\"}\n",
    )
    .expect("marker");
}

fn predecessor_store(marker: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().join("v2");
    std::fs::create_dir(&directory).expect("directory");
    let mut database =
        DatabaseConnection::create_new(&directory.join("aura-v2.db")).expect("database");
    let agent_sql = static_sql(predecessor(marker));
    let modules = MODULES
        .iter()
        .copied()
        .map(|module| {
            if module.owner == "agent" {
                schema::SchemaModule { owner: "agent", sql: agent_sql }
            } else {
                module
            }
        })
        .collect::<Vec<_>>();
    schema::initialize(&mut database, &modules).expect("predecessor schema");
    write_marker(&directory);
    database
        .connection()
        .execute(
            "INSERT INTO events(event_id, owner, kind, version, payload, created_at) \
             VALUES('evt-keep','owner','note',1,'{}','2020-01-01T00:00:00Z')",
            [],
        )
        .expect("original event");
    drop(database);
    (root, directory)
}

fn agent_digest(directory: &std::path::Path) -> String {
    let connection = Connection::open_with_flags(
        directory.join("aura-v2.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("read digest");
    connection
        .query_row(
            "SELECT digest FROM v2_schema_metadata WHERE owner='agent' AND version=1",
            [],
            |row| row.get(0),
        )
        .expect("digest")
}

fn object_count(directory: &std::path::Path, name: &str) -> i64 {
    let connection = Connection::open_with_flags(
        directory.join("aura-v2.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("read catalog");
    connection
        .query_row("SELECT count(*) FROM sqlite_master WHERE name=?1", [name], |row| row.get(0))
        .expect("count")
}

#[test]
fn a_supported_predecessor_migrates_once_and_keeps_existing_events() {
    let (_root, directory) = predecessor_store("\nCREATE TABLE agent_visible_index");
    let before = agent_digest(&directory);
    let store = V2Store::open(&directory).expect("supported predecessor opens");
    let payload: String = store
        .database
        .connection()
        .query_row("SELECT payload FROM events WHERE event_id='evt-keep'", [], |row| row.get(0))
        .expect("event");
    assert_eq!(payload, "{}");
    drop(store);
    let current = blake3::hash(include_str!("../src/agent.sql").as_bytes()).to_hex().to_string();
    assert_ne!(before, current);
    assert_eq!(agent_digest(&directory), current);
    assert_eq!(object_count(&directory, "agent_visible_delta"), 1);
    assert_eq!(object_count(&directory, "turn_poll"), 1);
    V2Store::open(&directory).expect("the migrated database opens again");
    assert_eq!(agent_digest(&directory), current);
}

#[test]
fn an_indexed_predecessor_gains_the_locatable_rows_without_rewriting_events() {
    let (_root, directory) = predecessor_store("\nCREATE TABLE agent_visible_delta");
    assert_eq!(object_count(&directory, "agent_visible_index"), 1);
    assert_eq!(object_count(&directory, "agent_visible_delta"), 0);
    V2Store::open(&directory).expect("indexed predecessor opens");
    assert_eq!(object_count(&directory, "agent_visible_delta"), 1);
    assert_eq!(object_count(&directory, "turn_poll"), 1);
    let payload: String = Connection::open(directory.join("aura-v2.db"))
        .expect("connection")
        .query_row("SELECT payload FROM events WHERE event_id='evt-keep'", [], |row| row.get(0))
        .expect("event");
    assert_eq!(payload, "{}");
}

#[test]
fn a_store_from_before_the_session_indexes_gains_them_and_keeps_its_events() {
    let (_root, directory) = predecessor_store("\nCREATE INDEX turn_tasks_session");
    assert_eq!(object_count(&directory, "turn_tasks_session"), 0);
    assert_eq!(object_count(&directory, "agent_runs_turn"), 0);
    V2Store::open(&directory).expect("the predecessor opens");
    assert_eq!(object_count(&directory, "turn_tasks_session"), 1);
    assert_eq!(object_count(&directory, "agent_runs_turn"), 1);
    let current = blake3::hash(include_str!("../src/agent.sql").as_bytes()).to_hex().to_string();
    assert_eq!(agent_digest(&directory), current);
    let payload: String = Connection::open(directory.join("aura-v2.db"))
        .expect("connection")
        .query_row("SELECT payload FROM events WHERE event_id='evt-keep'", [], |row| row.get(0))
        .expect("event");
    assert_eq!(payload, "{}");
}

#[test]
fn an_unknown_agent_digest_is_rejected_without_rewrite() {
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().join("v2");
    let store = V2Store::create(&directory).expect("store");
    drop(store);
    let connection = Connection::open(directory.join("aura-v2.db")).expect("connection");
    connection
        .execute(
            "UPDATE v2_schema_metadata SET digest='unrecognized-agent-schema' WHERE owner='agent'",
            [],
        )
        .expect("stamp");
    let before = connection
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get::<_, i64>(0))
        .expect("catalog");
    drop(connection);
    assert!(V2Store::open(&directory).is_err());
    assert_eq!(agent_digest(&directory), "unrecognized-agent-schema");
    let after = Connection::open(directory.join("aura-v2.db"))
        .expect("connection")
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get::<_, i64>(0))
        .expect("catalog");
    assert_eq!(before, after);
}

#[test]
fn a_foreign_application_is_rejected_before_any_table_is_created() {
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().join("v2");
    let store = V2Store::create(&directory).expect("store");
    drop(store);
    let connection = Connection::open(directory.join("aura-v2.db")).expect("connection");
    connection.execute("DROP TABLE agent_visible_index", []).expect("drop index");
    connection.execute("DROP TABLE agent_visible_body", []).expect("drop body");
    connection
        .execute(
            "UPDATE v2_schema_metadata SET digest='unrecognized-agent-schema' WHERE owner='agent'",
            [],
        )
        .expect("stamp");
    connection.pragma_update(None, "application_id", 17).expect("application id");
    drop(connection);
    assert!(V2Store::open(&directory).is_err());
    assert_eq!(object_count(&directory, "agent_visible_index"), 0);
    assert_eq!(object_count(&directory, "agent_visible_body"), 0);
    assert_eq!(agent_digest(&directory), "unrecognized-agent-schema");
    let application_id: i32 = Connection::open(directory.join("aura-v2.db"))
        .expect("connection")
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .expect("application id");
    assert_eq!(application_id, 17);
}

#[test]
fn a_migration_that_cannot_write_leaves_the_predecessor() {
    let (_root, directory) = predecessor_store("\nCREATE TABLE agent_visible_index");
    let before = agent_digest(&directory);
    let database = directory.join("aura-v2.db");
    let mut permissions = std::fs::metadata(&database).expect("metadata").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&database, permissions).expect("readonly");
    let opened = V2Store::open(&directory);
    assert!(opened.is_err());
    assert_eq!(agent_digest(&directory), before);
    assert_eq!(object_count(&directory, "agent_visible_delta"), 0);
    assert_eq!(object_count(&directory, "turn_poll"), 0);
}

#[test]
fn a_known_digest_with_a_different_column_is_rejected_before_any_write() {
    let (_root, directory) = predecessor_store("\nCREATE TABLE agent_visible_index");
    let before = agent_digest(&directory);
    Connection::open(directory.join("aura-v2.db"))
        .expect("alter")
        .execute_batch("ALTER TABLE turn_tasks ADD COLUMN unexpected TEXT")
        .expect("extra column");
    let opened = V2Store::open(&directory);
    assert!(opened.is_err(), "a predecessor digest with a different table is not migrated");
    assert_eq!(agent_digest(&directory), before);
    for name in [
        "agent_visible_index",
        "agent_visible_body",
        "agent_visible_delta",
        "agent_visible_coverage",
        "turn_poll",
        "agent_visible_repair",
        "turn_poll_capture",
    ] {
        assert_eq!(object_count(&directory, name), 0, "{name} was created before rejection");
    }
}
