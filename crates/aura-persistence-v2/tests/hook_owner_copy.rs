//! Explicit opt-in probe on an already-authorized TEMP copy, never the owner database.
//!
//! Adds `hook_invocations` and `hook_decisions` to `tool.sql`. Opening the copy must add
//! exactly the Hook tables it lacks, keep every event and stored tool row, stay intact, and
//! start normally again afterwards.
use aura_persistence_v2::V2Store;
use rusqlite::{Connection, OpenFlags};

const HOOK_TABLES: [&str; 2] = ["hook_decisions", "hook_invocations"];

fn tables(connection: &Connection) -> Vec<String> {
    connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .expect("tables")
        .query_map([], |row| row.get(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("names")
}

fn count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
        .expect("row count only")
}

#[test]
#[ignore = "requires the authorized owner database COPY in AURA_L07_OWNER_COPY"]
fn owner_copy_upgrade_preserves_events_and_adds_only_hook_tables() {
    let root = std::path::PathBuf::from(
        std::env::var_os("AURA_L07_OWNER_COPY").expect("explicit copy path"),
    );
    let root = std::fs::canonicalize(root).expect("existing copy");
    let temporary = std::fs::canonicalize(std::env::temp_dir()).expect("temporary root");
    assert!(
        root.starts_with(&temporary)
            && root
                .file_name()
                .expect("copy name")
                .to_string_lossy()
                .starts_with("aura-l07-owner-copy-"),
        "probe only accepts this run's named TEMP copy"
    );
    let before =
        Connection::open_with_flags(root.join("aura-v2.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("copy reader");
    let tables_before = tables(&before);
    let kept = ["events", "tool_batches", "tool_calls", "tool_results", "operations"]
        .into_iter()
        .filter(|table| tables_before.iter().any(|name| name == table))
        .map(|table| (table, count(&before, table)))
        .collect::<Vec<_>>();
    drop(before);
    let missing: Vec<&str> = HOOK_TABLES
        .into_iter()
        .filter(|name| !tables_before.iter().any(|table| table == name))
        .collect();

    let store = V2Store::open(&root).expect("upgrade COPY only");
    let after = store.database.connection();
    let tables_after = tables(after);
    let added: Vec<&str> = tables_after
        .iter()
        .map(String::as_str)
        .filter(|name| !tables_before.iter().any(|table| table == name))
        .collect();
    assert_eq!(added, missing, "only the missing Hook tables are added");
    for (table, rows) in &kept {
        assert_eq!(count(after, table), *rows, "{table} rows are kept");
    }
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
    let hook_rows: i64 = HOOK_TABLES.iter().map(|table| count(after, table)).sum();
    if !missing.is_empty() {
        assert_eq!(hook_rows, 0, "nothing is invented for an older store");
    }
    println!(
        "L07 OWNER COPY tables {}->{}; added {added:?}; kept {kept:?}; hook rows {hook_rows}",
        tables_before.len(),
        tables_after.len(),
    );
    drop(store);
    V2Store::open(&root).expect("startup after completed upgrade");
}
