//! Explicit opt-in probe on an already-authorized TEMP copy, never the owner database.
//!
//! Adds the package tables to `tool.sql` and new variants to stored enums (the package
//! consent caller and destination, the ToolPackage resource). Opening the copy must add exactly
//! the package tables it lacks, keep every event, stay intact, and every stored grant, dispatch
//! authority, secured operation, registry snapshot, tool definition and tool instance must still
//! decode.
use aura_execution_contract::SecuredOperation;
use aura_persistence_v2::V2Store;
use aura_security_contract::{AuthorizationDecision, AuthorizationGrant};
use aura_tool_contract::ToolRegistrySnapshotV1;
use aura_tool_domain::{ToolDefinitionRecordV1, ToolInstanceRecordV1};
use rusqlite::{Connection, OpenFlags};
use serde::de::DeserializeOwned;

const PACKAGE_TABLES: [&str; 9] = [
    "tool_package_configurations",
    "tool_package_credential_bindings",
    "tool_package_deletions",
    "tool_package_files",
    "tool_package_payloads",
    "tool_package_references",
    "tool_package_trust",
    "tool_package_versions",
    "tool_packages",
];

fn tables(connection: &Connection) -> Vec<String> {
    connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .expect("tables")
        .query_map([], |row| row.get(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("names")
}

fn decode_all<T: DeserializeOwned>(connection: &Connection, sql: &str, what: &str) -> usize {
    let payloads: Vec<String> = connection
        .prepare(sql)
        .expect(what)
        .query_map([], |row| row.get(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("payloads");
    for payload in &payloads {
        if let Err(error) = serde_json::from_str::<T>(payload) {
            panic!("a stored {what} does not decode with the L04 contracts: {error}");
        }
    }
    payloads.len()
}

#[test]
#[ignore = "requires the authorized owner database COPY in AURA_L04_OWNER_COPY"]
fn owner_copy_upgrade_preserves_events_and_adds_only_package_tables() {
    let root = std::path::PathBuf::from(
        std::env::var_os("AURA_L04_OWNER_COPY").expect("explicit copy path"),
    );
    let root = std::fs::canonicalize(root).expect("existing copy");
    let temporary = std::fs::canonicalize(std::env::temp_dir()).expect("temporary root");
    assert!(
        root.starts_with(&temporary)
            && root
                .file_name()
                .expect("copy name")
                .to_string_lossy()
                .starts_with("aura-l04-owner-copy-"),
        "probe only accepts this run's named TEMP copy"
    );
    let before =
        Connection::open_with_flags(root.join("aura-v2.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("copy reader");
    let events: i64 = before
        .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
        .expect("event count only");
    let tables_before = tables(&before);
    drop(before);
    let missing: Vec<&str> = PACKAGE_TABLES
        .into_iter()
        .filter(|name| !tables_before.iter().any(|table| table == name))
        .collect();

    let store = V2Store::open(&root).expect("upgrade COPY only");
    let after = store.database.connection();
    let events_after: i64 = after
        .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
        .expect("event count only");
    let tables_after = tables(after);
    let added: Vec<&str> = tables_after
        .iter()
        .map(String::as_str)
        .filter(|name| !tables_before.iter().any(|table| table == name))
        .collect();
    assert_eq!(events_after, events);
    assert_eq!(added, missing, "only the missing package tables are added");
    let integrity: String =
        after.query_row("PRAGMA integrity_check", [], |row| row.get(0)).expect("integrity");
    assert_eq!(integrity, "ok");
    let foreign_key_problems = after
        .prepare("PRAGMA foreign_key_check")
        .expect("foreign keys")
        .query_map([], |_| Ok(()))
        .expect("rows")
        .count();
    assert_eq!(foreign_key_problems, 0);
    let package_rows: i64 = PACKAGE_TABLES
        .iter()
        .map(|name| {
            after
                .query_row(&format!("SELECT count(*) FROM {name}"), [], |row| row.get::<_, i64>(0))
                .expect("package rows")
        })
        .sum();
    if !missing.is_empty() {
        assert_eq!(package_rows, 0, "no legacy package/trust migration");
    }

    let grants = decode_all::<AuthorizationGrant>(
        after,
        "SELECT payload FROM authorization_grants ORDER BY id",
        "authorization grant",
    );
    let authorities = decode_all::<AuthorizationDecision>(
        after,
        "SELECT payload FROM operation_journal_entries WHERE kind='dispatch_authority' ORDER BY operation_id",
        "dispatch authority",
    );
    let operations = decode_all::<SecuredOperation>(
        after,
        "SELECT payload FROM operations WHERE domain IN ('core_broker','execution') ORDER BY id",
        "secured operation",
    );
    let snapshots = decode_all::<ToolRegistrySnapshotV1>(
        after,
        "SELECT payload FROM tool_registry_snapshots ORDER BY id",
        "registry snapshot",
    );
    let definitions = decode_all::<ToolDefinitionRecordV1>(
        after,
        "SELECT payload FROM tool_definitions ORDER BY tool_id, version",
        "tool definition",
    );
    let instances = decode_all::<ToolInstanceRecordV1>(
        after,
        "SELECT payload FROM tool_instances ORDER BY id",
        "tool instance",
    );
    println!(
        "L04 OWNER COPY events {events}->{events_after}; tables {}->{}; added {added:?}; package rows {package_rows}; grants {grants}; authorities {authorities}; operations {operations}; snapshots {snapshots}; definitions {definitions}; instances {instances}",
        tables_before.len(),
        tables_after.len(),
    );
    drop(store);
    V2Store::open(&root).expect("startup after completed upgrade");
}
