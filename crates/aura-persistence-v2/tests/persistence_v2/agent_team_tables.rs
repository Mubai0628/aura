//! The sub-agent tables: a store written before them gains them in one migration and keeps its
//! events, and deleting a conversation deletes its sub-agent rows, both those naming it as the
//! sub-agent and those naming it as the conversation that started one.
#![allow(clippy::unwrap_used)]

use aura_domain_common::SessionId;
use aura_persistence_core::{DatabaseConnection, schema};
use aura_persistence_v2::{MODULES, V2Store, session_purge};
use rusqlite::Connection;

const MARKER: &str = "\nCREATE TABLE agent_team_members";

fn predecessor_store() -> (tempfile::TempDir, std::path::PathBuf) {
    let sql = include_str!("../../src/agent.sql");
    let (head, _) = sql.split_once(MARKER).expect("the sub-agent tables follow the predecessor");
    let agent_sql: &'static str = Box::leak(format!("{head}\n").into_boxed_str());
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().join("v2");
    std::fs::create_dir(&directory).expect("directory");
    let mut database =
        DatabaseConnection::create_new(&directory.join("aura-v2.db")).expect("database");
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
    std::fs::write(
        directory.join("v2-development.json"),
        b"{\"schema_version\":1,\"namespace\":\"aura-v2-development\"}\n",
    )
    .expect("marker");
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

fn tables(directory: &std::path::Path) -> Vec<String> {
    let connection = Connection::open(directory.join("aura-v2.db")).expect("connection");
    let mut statement = connection
        .prepare("SELECT name FROM sqlite_master WHERE name LIKE 'agent_team%' ORDER BY name")
        .expect("catalog");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("names")
}

#[test]
fn a_store_from_before_the_sub_agent_tables_gains_them_and_keeps_its_events() {
    let (_root, directory) = predecessor_store();
    assert!(tables(&directory).is_empty());
    V2Store::open(&directory).expect("the predecessor opens");
    assert_eq!(
        tables(&directory),
        [
            "agent_team_members",
            "agent_team_members_parent",
            "agent_team_reports",
            "agent_team_settings",
        ]
    );
    let payload: String = Connection::open(directory.join("aura-v2.db"))
        .expect("connection")
        .query_row("SELECT payload FROM events WHERE event_id='evt-keep'", [], |row| row.get(0))
        .expect("event");
    assert_eq!(payload, "{}");
    V2Store::open(&directory).expect("the migrated store opens again");
}

fn member(db: &Connection, child: SessionId, parent: SessionId, handle: &str) {
    db.execute(
        "INSERT INTO agent_team_members(child_session_id, parent_session_id, handle, spawn_call, \
         state, revision, payload, created_at, updated_at) \
         VALUES(?1, ?2, ?3, ?4, 'running', 1, '{}', '2026-11-06T00:00:00Z', \
         '2026-11-06T00:00:00Z')",
        rusqlite::params![child.to_string(), parent.to_string(), handle, format!("call-{child}")],
    )
    .expect("member");
    db.execute(
        "INSERT INTO agent_team_reports(child_session_id, sequence, source, turn_task_id, \
         status, payload, created_at) \
         VALUES(?1, 1, ?2, 'turn', 'ready', '{}', '2026-11-06T00:00:00Z')",
        rusqlite::params![child.to_string(), format!("report-{child}")],
    )
    .expect("report");
}

fn count(db: &Connection, table: &str, column: &str, session: SessionId) -> i64 {
    db.query_row(
        &format!("SELECT count(*) FROM {table} WHERE {column}=?1"),
        [session.to_string()],
        |row| row.get(0),
    )
    .expect("count")
}

#[test]
fn deleting_a_conversation_deletes_its_sub_agent_rows_on_either_side() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let (parent, child, other_child) = (SessionId::new(), SessionId::new(), SessionId::new());
    let unrelated = SessionId::new();
    {
        let db = store.database.connection();
        member(db, child, parent, "A1");
        member(db, other_child, parent, "A2");
        member(db, SessionId::new(), unrelated, "A1");
    }
    session_purge::purge_now(&mut store.database, |db| session_purge::plan_session(db, child))
        .expect("the sub-agent's conversation purged");
    let db = store.database.connection();
    assert_eq!(count(db, "agent_team_members", "child_session_id", child), 0);
    assert_eq!(count(db, "agent_team_reports", "child_session_id", child), 0);
    assert_eq!(count(db, "agent_team_members", "child_session_id", other_child), 1);
    session_purge::purge_now(&mut store.database, |db| session_purge::plan_session(db, parent))
        .expect("the starting conversation purged");
    let db = store.database.connection();
    assert_eq!(count(db, "agent_team_members", "parent_session_id", parent), 0);
    assert_eq!(count(db, "agent_team_members", "parent_session_id", unrelated), 1);
}
