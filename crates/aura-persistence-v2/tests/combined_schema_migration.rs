//! Migrating a database whose modules are out of date together.
//!
//! A database created before both the agent visible index and the reusable Native
//! session consent table existed needs both moves, and it is a database this build
//! has a reviewed path from. Treating "two modules are behind" as an unsupported
//! schema is what made that database unopenable.
//!
//! What must not change is the standard the single-module path already met: only the
//! exact frozen digests are accepted, the whole structure is compared before a writer
//! opens, and every module moves in one transaction or none does.
#![allow(clippy::unwrap_used)]

use aura_persistence_core::{DatabaseConnection, schema};
use aura_persistence_v2::{MODULES, V2Store};
use rusqlite::Connection;

const AGENT_CURRENT: &str = include_str!("../src/agent.sql");
const SECURITY_CURRENT: &str = include_str!("../src/security.sql");

fn module_predecessor(owner: &str, marker: &str) -> &'static str {
    let sql = if owner == "agent" { AGENT_CURRENT } else { SECURITY_CURRENT };
    let (head, _) = sql.split_once(marker).unwrap_or_else(|| panic!("{owner} marker {marker}"));
    Box::leak(format!("{head}\n").into_boxed_str())
}

fn agent_predecessor(marker: &str) -> &'static str {
    module_predecessor("agent", marker)
}

fn security_predecessor() -> &'static str {
    module_predecessor("security", "\nCREATE TABLE native_session_consents")
}

fn modules(agent: &'static str, security: &'static str) -> Vec<schema::SchemaModule> {
    MODULES
        .iter()
        .copied()
        .map(|module| match module.owner {
            "agent" => schema::SchemaModule { owner: "agent", sql: agent },
            "security" => schema::SchemaModule { owner: "security", sql: security },
            _ => module,
        })
        .collect()
}

/// A database created from the named predecessors of both modules.
fn predecessor_store(
    agent: &'static str,
    security: &'static str,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().join("v2");
    std::fs::create_dir(&directory).expect("directory");
    let mut database =
        DatabaseConnection::create_new(&directory.join("aura-v2.db")).expect("database");
    schema::initialize(&mut database, &modules(agent, security)).expect("predecessor schema");
    database
        .connection()
        .execute(
            "INSERT INTO events(event_id, owner, kind, version, payload, created_at) \
             VALUES('evt-keep','owner','note',1,'{}','2020-01-01T00:00:00Z')",
            [],
        )
        .expect("original event");
    database
        .connection()
        .execute("INSERT INTO authorization_grants VALUES('pgr_exact',1,'{}',0,0,'{}')", [])
        .expect("historical exact-operation grant");
    drop(database);
    std::fs::write(
        directory.join("v2-development.json"),
        b"{\"schema_version\":1,\"namespace\":\"aura-v2-development\"}\n",
    )
    .expect("marker");
    (root, directory)
}

fn read_only(directory: &std::path::Path) -> Connection {
    Connection::open_with_flags(
        directory.join("aura-v2.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("read")
}

fn object_count(directory: &std::path::Path, name: &str) -> i64 {
    read_only(directory)
        .query_row("SELECT count(*) FROM sqlite_master WHERE name=?1", [name], |row| row.get(0))
        .expect("count")
}

fn digest(directory: &std::path::Path, owner: &str) -> String {
    read_only(directory)
        .query_row(
            "SELECT digest FROM v2_schema_metadata WHERE owner=?1 AND version=1",
            [owner],
            |row| row.get(0),
        )
        .expect("digest")
}

fn historical_grant_payload(directory: &std::path::Path) -> String {
    read_only(directory)
        .query_row("SELECT payload FROM authorization_grants WHERE id='pgr_exact'", [], |row| {
            row.get(0)
        })
        .expect("grant")
}

fn current_digest(owner: &str) -> String {
    let sql = if owner == "agent" { AGENT_CURRENT } else { SECURITY_CURRENT };
    blake3::hash(sql.as_bytes()).to_hex().to_string()
}

#[test]
fn a_database_behind_in_both_modules_migrates_in_one_transaction() {
    let (_root, directory) = predecessor_store(
        agent_predecessor("\nCREATE TABLE agent_visible_index"),
        security_predecessor(),
    );
    assert_eq!(object_count(&directory, "agent_visible_index"), 0);
    assert_eq!(object_count(&directory, "native_session_consents"), 0);
    let store = V2Store::open(&directory).expect("a doubly outdated database opens");
    drop(store);
    assert_eq!(digest(&directory, "agent"), current_digest("agent"));
    assert_eq!(digest(&directory, "security"), current_digest("security"));
    assert_eq!(object_count(&directory, "agent_visible_index"), 1);
    assert_eq!(object_count(&directory, "native_session_consents"), 1);
    let payload: String = read_only(&directory)
        .query_row("SELECT payload FROM events WHERE event_id='evt-keep'", [], |row| row.get(0))
        .expect("event");
    assert_eq!(payload, "{}", "the migration rewrites no existing fact");
    assert_eq!(
        historical_grant_payload(&directory),
        "{}",
        "a historical exact-operation Grant keeps its stored meaning"
    );
    // Opening again is a no-op: the digests are current, so nothing is applied twice.
    V2Store::open(&directory).expect("the migrated database opens again");
    assert_eq!(digest(&directory, "agent"), current_digest("agent"));
    assert_eq!(digest(&directory, "security"), current_digest("security"));
    assert_eq!(object_count(&directory, "native_session_consents"), 1);
}

#[test]
fn an_older_security_predecessor_combines_with_each_supported_agent_predecessor() {
    for marker in [
        "\nCREATE TABLE agent_visible_index",
        "\nCREATE TABLE agent_visible_delta",
        "\nCREATE TABLE agent_visible_repair",
    ] {
        let (_root, directory) =
            predecessor_store(agent_predecessor(marker), security_predecessor());
        V2Store::open(&directory)
            .unwrap_or_else(|error| panic!("agent predecessor {marker} must combine: {error}"));
        assert_eq!(digest(&directory, "agent"), current_digest("agent"), "{marker}");
        assert_eq!(digest(&directory, "security"), current_digest("security"), "{marker}");
        assert_eq!(object_count(&directory, "native_session_consents"), 1, "{marker}");
    }
}

#[test]
fn a_single_outdated_module_still_migrates() {
    let (_root, directory) = predecessor_store(AGENT_CURRENT, security_predecessor());
    V2Store::open(&directory).expect("only security is behind");
    assert_eq!(digest(&directory, "agent"), current_digest("agent"));
    assert_eq!(digest(&directory, "security"), current_digest("security"));

    let (_root, directory) = predecessor_store(
        agent_predecessor("\nCREATE TABLE agent_visible_index"),
        SECURITY_CURRENT,
    );
    V2Store::open(&directory).expect("only agent is behind");
    assert_eq!(digest(&directory, "agent"), current_digest("agent"));
    assert_eq!(digest(&directory, "security"), current_digest("security"));
}

#[test]
fn a_current_database_is_left_alone() {
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().join("v2");
    V2Store::create(&directory).expect("store");
    let before = object_count(&directory, "native_session_consents");
    V2Store::open(&directory).expect("a current database opens");
    assert_eq!(digest(&directory, "agent"), current_digest("agent"));
    assert_eq!(digest(&directory, "security"), current_digest("security"));
    assert_eq!(object_count(&directory, "native_session_consents"), before);
}

#[test]
fn one_unknown_module_refuses_the_whole_set_without_rewriting_anything() {
    let (_root, directory) = predecessor_store(
        agent_predecessor("\nCREATE TABLE agent_visible_index"),
        security_predecessor(),
    );
    read_only(&directory).execute("PRAGMA journal_mode=WAL", []).ok();
    Connection::open(directory.join("aura-v2.db"))
        .expect("connection")
        .execute(
            "UPDATE v2_schema_metadata SET digest='unrecognized-security-schema' WHERE owner='security'",
            [],
        )
        .expect("stamp");
    let agent_before = digest(&directory, "agent");
    let tables_before = read_only(&directory)
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get::<_, i64>(0))
        .expect("catalog");
    assert!(V2Store::open(&directory).is_err());
    assert_eq!(
        digest(&directory, "agent"),
        agent_before,
        "the supported module must not move when another module is unknown"
    );
    assert_eq!(digest(&directory, "security"), "unrecognized-security-schema");
    let tables_after = read_only(&directory)
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get::<_, i64>(0))
        .expect("catalog");
    assert_eq!(tables_before, tables_after, "no partial migration is left behind");
}

#[test]
fn a_structure_drift_in_one_module_refuses_the_whole_set() {
    let (_root, directory) = predecessor_store(
        agent_predecessor("\nCREATE TABLE agent_visible_index"),
        security_predecessor(),
    );
    // Same digests, different structure: a column added by hand is not either
    // predecessor this build was written against.
    Connection::open(directory.join("aura-v2.db"))
        .expect("connection")
        .execute("ALTER TABLE source_trust ADD COLUMN extra TEXT", [])
        .expect("drift");
    assert!(V2Store::open(&directory).is_err());
    assert_eq!(object_count(&directory, "agent_visible_index"), 0);
    assert_eq!(object_count(&directory, "native_session_consents"), 0);
    assert_ne!(digest(&directory, "security"), current_digest("security"));
}

#[test]
fn a_foreign_application_is_refused_before_any_write() {
    let (_root, directory) = predecessor_store(
        agent_predecessor("\nCREATE TABLE agent_visible_index"),
        security_predecessor(),
    );
    Connection::open(directory.join("aura-v2.db"))
        .expect("connection")
        .execute("PRAGMA application_id = 17", [])
        .expect("stamp");
    assert!(V2Store::open(&directory).is_err());
    assert_eq!(object_count(&directory, "agent_visible_index"), 0);
    assert_eq!(object_count(&directory, "native_session_consents"), 0);
}
