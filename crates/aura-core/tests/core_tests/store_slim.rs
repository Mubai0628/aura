//! What Core does for the launcher before the desktop starts: `aura-core --store-slim` slims the
//! execution store and `aura.db`, a line per step and then one line reporting both.
#![allow(clippy::unwrap_used)]
use std::process::Command;

use aura_domain_common::RuntimeInstanceId;
use aura_persistence::{PersistenceCoordinator, PersistenceOptions};
use serde_json::Value;

const MIB: u64 = 1024 * 1024;

#[test]
fn the_slim_command_slims_both_stores_and_reports_them_in_its_last_line() {
    let data_root = tempfile::tempdir().unwrap();
    let data = data_root.path().join("data");
    std::fs::create_dir(&data).unwrap();
    drop(aura_persistence_v2::V2Store::create(&data.join("v2-execution")).unwrap());
    let options = PersistenceOptions::new(
        data.join("aura.db"),
        data.join("artifact-objects"),
        RuntimeInstanceId::new(),
    );
    drop(PersistenceCoordinator::start(options).unwrap());
    // About 70 MiB of aura.db left free, as deleted records leave it.
    let db = rusqlite::Connection::open(data.join("aura.db")).unwrap();
    db.execute_batch("CREATE TABLE ballast(bytes BLOB NOT NULL)").unwrap();
    for _ in 0..70 {
        db.execute("INSERT INTO ballast VALUES (zeroblob(1048576))", []).unwrap();
    }
    db.execute_batch("DROP TABLE ballast; PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
    drop(db);
    let before = std::fs::metadata(data.join("aura.db")).unwrap().len();

    let output = Command::new(env!("CARGO_BIN_EXE_aura-core"))
        .arg("--store-slim")
        .arg(data_root.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    let lines: Vec<Value> =
        stdout.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert!(lines.iter().any(|line| line["step"] == "rebuilding"), "{stdout}");
    let report = &lines.last().unwrap()["report"];
    assert_eq!(report["execution"]["rebuild"]["state"], "not_needed", "{report}");
    assert_eq!(report["legacy"]["rebuild"]["state"], "done", "{report}");
    let after = std::fs::metadata(data.join("aura.db")).unwrap().len();
    assert!(after + 60 * MIB < before, "aura.db {before} -> {after} bytes");
    let marker = std::fs::read(data.join("aura-db-slimming.json")).unwrap();
    let marker: Value = serde_json::from_slice(&marker).unwrap();
    assert_eq!(marker["owed_runs"], Value::Null, "{marker}");
}
