//! The frozen `tool.sql` before Hooks -> with Hooks: only the two Hook tables are added, existing facts stay.
use aura_persistence_core::{DatabaseConnection, schema};
use aura_persistence_v2::{MODULES, V2Store};
use rusqlite::{Connection, OpenFlags};

const BEFORE: &str = include_str!("fixtures/tool-before-l07.sql");
const TABLES: &[&str] = &["hook_invocations", "hook_decisions"];

fn hook_tables(connection: &Connection) -> usize {
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
fn the_l06_schema_upgrades_once_adding_only_the_hook_tables() {
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
    schema::initialize(&mut database, &modules).expect("frozen L06 schema");
    database
        .connection()
        .execute(
            "INSERT INTO tool_packages(id,package_key,revision) VALUES('tpk_01ARZ3NDEKTSV4RRFFQ69G5FAV','org.example.kept',1)",
            [],
        )
        .expect("an L06 package row");
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
    assert_eq!(hook_tables(&reader), 0);
    drop(reader);

    let store = V2Store::open(&root).expect("supported L06 upgrade");
    assert_eq!(hook_tables(store.database.connection()), TABLES.len());
    let key: String = store
        .database
        .connection()
        .query_row("SELECT package_key FROM tool_packages", [], |row| row.get(0))
        .expect("kept package");
    assert_eq!(key, "org.example.kept");
    let after = digest(store.database.connection());
    drop(store);
    assert_eq!(digest(V2Store::open(&root).expect("reopen").database.connection()), after);
}

#[test]
fn a_fresh_namespace_has_empty_hook_tables() {
    let temp = tempfile::tempdir().expect("temp");
    let store = V2Store::create(&temp.path().join("v2")).expect("create");
    assert_eq!(hook_tables(store.database.connection()), TABLES.len());
}
