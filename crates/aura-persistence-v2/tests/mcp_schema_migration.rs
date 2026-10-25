//! The frozen `tool.sql` before MCP -> with MCP: only the two MCP tables are added, existing facts stay.
use aura_persistence_core::{DatabaseConnection, schema};
use aura_persistence_v2::{MODULES, V2Store};
use rusqlite::{Connection, OpenFlags};

const BEFORE: &str = include_str!("fixtures/tool-before-l08.sql");
const TABLES: &[&str] = &["mcp_catalogs", "mcp_tool_revisions"];

fn mcp_tables(connection: &Connection) -> usize {
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

fn digest(connection: &Connection) -> String {
    connection
        .query_row("SELECT digest FROM v2_schema_metadata WHERE owner='tool'", [], |row| row.get(0))
        .expect("tool digest")
}

#[test]
fn the_l07_schema_upgrades_once_adding_only_the_mcp_tables() {
    let temp = tempfile::tempdir().expect("temporary namespace");
    let root = temp.path().join("v2");
    std::fs::create_dir(&root).expect("root");
    let mut database = DatabaseConnection::create_new(&root.join("aura-v2.db")).expect("db");
    let modules = MODULES
        .iter()
        .map(|module| schema::SchemaModule {
            owner: module.owner,
            sql: if module.owner == "tool" { BEFORE } else { module.sql },
        })
        .collect::<Vec<_>>();
    schema::initialize(&mut database, &modules).expect("frozen L07 schema");
    database
        .connection()
        .execute(
            "INSERT INTO hook_decisions(subject,event,decision,decided_at) VALUES('call','tool.before_call','{\"kind\":\"continue\"}','2026-10-17T00:00:00Z')",
            [],
        )
        .expect("an L07 decision row");
    drop(database);
    std::fs::write(
        root.join("v2-development.json"),
        b"{\"schema_version\":1,\"namespace\":\"aura-v2-development\"}\n",
    )
    .expect("namespace");
    let reader =
        Connection::open_with_flags(root.join("aura-v2.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("read fixture");
    assert_eq!(digest(&reader), blake3::hash(BEFORE.as_bytes()).to_hex().as_str());
    assert_eq!(mcp_tables(&reader), 0);
    drop(reader);

    let store = V2Store::open(&root).expect("supported L07 upgrade");
    assert_eq!(mcp_tables(store.database.connection()), TABLES.len());
    let kept: String = store
        .database
        .connection()
        .query_row("SELECT subject FROM hook_decisions", [], |row| row.get(0))
        .expect("kept decision");
    assert_eq!(kept, "call");
    let after = digest(store.database.connection());
    drop(store);
    assert_eq!(digest(V2Store::open(&root).expect("reopen").database.connection()), after);
}

#[test]
fn tool_revisions_keep_their_inputs_and_first_time_and_advance_on_change() {
    use aura_persistence_v2::mcp::{revision_of, tool_revision};
    let temp = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&temp.path().join("v2")).expect("create");
    let first = aura_domain_common::UtcTimestamp::now();
    let later =
        aura_domain_common::UtcTimestamp::from_value(first.value() + time::Duration::hours(1));
    let revisions = store
        .database
        .unit_of_work(|tx| -> aura_persistence_core::StoreResult<_> {
            Ok([
                tool_revision(tx, "tool", "a", first)?,
                tool_revision(tx, "tool", "a", later)?,
                tool_revision(tx, "tool", "b", later)?,
                tool_revision(tx, "tool", "a", later)?,
                tool_revision(tx, "other", "b", later)?,
            ])
        })
        .expect("revisions");
    assert_eq!(revisions.map(|r| r.revision), [1, 1, 2, 1, 1]);
    assert_eq!(revisions[1].created_at, first, "a known key keeps its first time");
    assert_eq!(revisions[2].created_at, later);
    let db = store.database.connection();
    assert_eq!(revision_of(db, "tool", "b").expect("read"), Some(revisions[2]));
    assert_eq!(revision_of(db, "tool", "c").expect("read"), None, "reading assigns nothing");
}
