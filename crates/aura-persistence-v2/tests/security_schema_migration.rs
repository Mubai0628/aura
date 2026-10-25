//! The `security` module's own schema predecessor.
//!
//! The reusable Native session consent added one table. A database created before
//! it exists has no consent records, so the migration only adds the table: it never
//! rewrites a stored Grant, which is what keeps a historical `allow_session` row at
//! its exact-operation meaning.
//!
//! These cases build the predecessor the same way the migration does — by cutting
//! the current module SQL at the first statement the predecessor lacks — so a change
//! to the marker that would stop the migration from being recognised fails here.
#![allow(clippy::unwrap_used)]

use aura_persistence_core::{DatabaseConnection, schema};
use aura_persistence_v2::{MODULES, V2Store};
use rusqlite::Connection;

const SECURITY_CURRENT: &str = include_str!("../src/security.sql");
const SECURITY_PREDECESSOR_MARKER: &str = "\nCREATE TABLE native_session_consents";

/// The predecessor SQL the migration must recognise, cut exactly as it cuts it.
fn predecessor_sql() -> &'static str {
    let (head, _) = SECURITY_CURRENT.split_once(SECURITY_PREDECESSOR_MARKER).expect("marker");
    Box::leak(format!("{head}\n").into_boxed_str())
}

fn modules_with_security(sql: &'static str) -> Vec<schema::SchemaModule> {
    MODULES
        .iter()
        .copied()
        .map(|module| {
            if module.owner == "security" {
                schema::SchemaModule { owner: "security", sql }
            } else {
                module
            }
        })
        .collect()
}

fn open_predecessor() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().join("v2");
    std::fs::create_dir(&directory).expect("directory");
    let mut database =
        DatabaseConnection::create_new(&directory.join("aura-v2.db")).expect("database");
    schema::initialize(&mut database, &modules_with_security(predecessor_sql()))
        .expect("predecessor schema");
    database
        .connection()
        .execute(
            "INSERT INTO events(event_id, owner, kind, version, payload, created_at) \
             VALUES('evt-keep','owner','note',1,'{}','2020-01-01T00:00:00Z')",
            [],
        )
        .expect("original event");
    drop(database);
    std::fs::write(
        directory.join("v2-development.json"),
        b"{\"schema_version\":1,\"namespace\":\"aura-v2-development\"}\n",
    )
    .expect("marker");
    (root, directory)
}

fn object_count(directory: &std::path::Path, name: &str) -> i64 {
    Connection::open_with_flags(
        directory.join("aura-v2.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("read catalog")
    .query_row("SELECT count(*) FROM sqlite_master WHERE name=?1", [name], |row| row.get(0))
    .expect("count")
}

fn security_digest(directory: &std::path::Path) -> String {
    Connection::open_with_flags(
        directory.join("aura-v2.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("read digest")
    .query_row(
        "SELECT digest FROM v2_schema_metadata WHERE owner='security' AND version=1",
        [],
        |row| row.get(0),
    )
    .expect("digest")
}

#[test]
fn the_marker_cuts_exactly_before_the_new_table() {
    let predecessor = predecessor_sql();
    assert!(
        !predecessor.contains("native_session_consents"),
        "the predecessor must be the schema without the consent table"
    );
    assert!(
        predecessor.contains("CREATE TABLE authorization_grants"),
        "the predecessor still carries the authorization it always had"
    );
    let migration = &SECURITY_CURRENT[predecessor.len()..];
    assert!(migration.starts_with("CREATE TABLE native_session_consents"), "{migration}");
    assert!(migration.contains("CREATE INDEX native_session_consents_by_session"));
}

#[test]
fn a_database_without_the_consent_table_gains_it_without_rewriting_grants() {
    let (_root, directory) = open_predecessor();
    assert_eq!(object_count(&directory, "native_session_consents"), 0);
    let before = security_digest(&directory);
    let store = V2Store::open(&directory).expect("supported security predecessor opens");
    let current = blake3::hash(SECURITY_CURRENT.as_bytes()).to_hex().to_string();
    assert_ne!(before, current, "the predecessor digest is not the current one");
    drop(store);
    assert_eq!(security_digest(&directory), current);
    assert_eq!(object_count(&directory, "native_session_consents"), 1);
    assert_eq!(object_count(&directory, "native_session_consents_by_session"), 1);
    let payload: String = Connection::open(directory.join("aura-v2.db"))
        .expect("connection")
        .query_row("SELECT payload FROM events WHERE event_id='evt-keep'", [], |row| row.get(0))
        .expect("event");
    assert_eq!(payload, "{}", "the migration rewrites no existing fact");
    V2Store::open(&directory).expect("the migrated database opens again");
    assert_eq!(security_digest(&directory), current);
}

#[test]
fn a_predecessor_whose_structure_does_not_match_is_refused_before_any_write() {
    let (_root, directory) = open_predecessor();
    // Same digest, different structure: a column added by hand is not the schema
    // this migration was written against.
    Connection::open(directory.join("aura-v2.db"))
        .expect("connection")
        .execute("ALTER TABLE source_trust ADD COLUMN extra TEXT", [])
        .expect("drift");
    let before = object_count(&directory, "native_session_consents");
    assert!(V2Store::open(&directory).is_err());
    assert_eq!(object_count(&directory, "native_session_consents"), before);
    assert_ne!(
        security_digest(&directory),
        blake3::hash(SECURITY_CURRENT.as_bytes()).to_hex().to_string()
    );
}

#[test]
fn an_unknown_security_digest_is_refused() {
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().join("v2");
    V2Store::create(&directory).expect("store");
    Connection::open(directory.join("aura-v2.db"))
        .expect("connection")
        .execute(
            "UPDATE v2_schema_metadata SET digest='unrecognized-security-schema' WHERE owner='security'",
            [],
        )
        .expect("stamp");
    assert!(V2Store::open(&directory).is_err());
    assert_eq!(security_digest(&directory), "unrecognized-security-schema");
}
