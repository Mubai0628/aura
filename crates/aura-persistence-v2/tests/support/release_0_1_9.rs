//! A store as 0.1.9 left it: the same tables, without the indexes a later version adds on what
//! points at an artifact, under the digests of 0.1.9's own schema texts.
#![allow(dead_code)]
use std::path::Path;

use aura_persistence_core::schema::SchemaModule;

/// 0.1.9's texts of the two modules that changed since.
const EXECUTION: &str = include_str!("../fixtures/execution-0.1.9.sql");
const TOOL: &str = include_str!("../fixtures/tool-0.1.9.sql");

/// 0.1.9's modules, which its structure check compares a store against.
pub fn modules() -> Vec<SchemaModule> {
    aura_persistence_v2::MODULES
        .iter()
        .map(|module| match module.owner {
            "execution" => SchemaModule { owner: "execution", sql: EXECUTION },
            "tool" => SchemaModule { owner: "tool", sql: TOOL },
            _ => *module,
        })
        .collect()
}

/// What a later version adds to a 0.1.9 store.
pub const INDEXES: [&str; 4] = [
    "artifacts_by_object",
    "execution_results_by_receipt",
    "operation_artifact_links_by_artifact",
    "terminal_output_segments_by_artifact",
];

/// Turns the current store at `database` into the store 0.1.9 would have written.
pub fn as_0_1_9(database: &Path) {
    let db = rusqlite::Connection::open(database).unwrap();
    for index in INDEXES {
        db.execute_batch(&format!("DROP INDEX IF EXISTS {index}")).unwrap();
    }
    for (owner, sql) in [("execution", EXECUTION), ("tool", TOOL)] {
        db.execute(
            "UPDATE v2_schema_metadata SET digest=?2 WHERE owner=?1 AND version=1",
            rusqlite::params![owner, blake3::hash(sql.as_bytes()).to_hex().as_str()],
        )
        .unwrap();
    }
}

/// Which of [`INDEXES`] the store has.
pub fn indexes(db: &rusqlite::Connection) -> Vec<String> {
    let mut query = db
        .prepare(
            "SELECT name FROM sqlite_master WHERE type='index' AND name IN (SELECT value FROM \
             json_each(?1)) ORDER BY name",
        )
        .unwrap();
    query
        .query_map([serde_json::to_string(&INDEXES).unwrap()], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
