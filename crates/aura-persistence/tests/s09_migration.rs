use aura_domain_common::{CorrelationId, RuntimeInstanceId, UtcTimestamp};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use rusqlite::Connection;

fn open(path: &std::path::Path) -> PersistenceCoordinator {
    let options = PersistenceOptions::new(
        path.to_path_buf(),
        path.parent().expect("isolated parent").join("artifacts"),
        RuntimeInstanceId::new(),
    );
    let PersistenceStartup::Ready(store) = PersistenceCoordinator::start(options).expect("migrate")
    else {
        panic!("historical schema must upgrade without recovery");
    };
    store
}

#[test]
fn s09_version_50_creates_rebinding_storage_before_any_builtin_is_materialized() {
    let root = tempfile::tempdir().expect("isolated schema");
    let path = root.path().join("state.db");
    drop(open(&path));
    let db = Connection::open(&path).expect("inspect fresh schema");
    let version: i64 = db
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0))
        .expect("official migration version");
    assert_eq!(version, 50, "rebinding storage is a versioned migration");
    db.prepare("SELECT tool_instance_id,reason FROM builtin_credential_rebind_required")
        .expect("the table exists before business operations");
}

#[test]
fn s09_version_49_upgrades_and_converts_the_unversioned_reason_to_a_code() {
    let root = tempfile::tempdir().expect("isolated historical schema");
    let path = root.path().join("state.db");
    let store = open(&path);
    store
        .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
        .expect("a genuine historical instance owner");
    let id = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP entry")
        .instance
        .tool_instance_id
        .to_string();
    drop(store);
    let db = Connection::open(&path).expect("seed pre-migration state");
    db.execute_batch("DELETE FROM schema_migrations WHERE version >= 50;
        DROP TABLE IF EXISTS builtin_credential_rebind_required;
        CREATE TABLE builtin_credential_rebind_required(tool_instance_id TEXT PRIMARY KEY,reason TEXT NOT NULL);
        ")
        .expect("schema 49 with the first batch's unversioned row");
    db.execute(
        "INSERT INTO builtin_credential_rebind_required VALUES(?1,?2)",
        rusqlite::params![
            id,
            "凭据槽或放置方式已变化。请在工具设置中重新绑定凭据，并核对允许网站。"
        ],
    )
    .expect("unversioned legacy reason for an existing instance");
    drop(db);
    drop(open(&path));
    let db = Connection::open(path).expect("inspect upgraded storage");
    let reason: String = db
        .query_row("SELECT reason FROM builtin_credential_rebind_required", [], |r| r.get(0))
        .expect("historical reason is retained");
    assert_eq!(reason, "credential_slot_changed", "UI prose is not persisted");
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM schema_migrations WHERE version=50", [], |r| r.get(0))
        .expect("version 50 is journaled");
    assert_eq!(count, 1);
}
