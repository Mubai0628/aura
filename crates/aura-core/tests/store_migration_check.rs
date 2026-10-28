//! What Core answers the launcher before this build first starts in place of an older version:
//! the stores to copy, so that going back can put them back. A store needs a copy when this build
//! migrates it, and also when the version it replaces would refuse records this build writes.
#![allow(clippy::unwrap_used)]
use std::{path::Path, process::Command};

const EXECUTION: &str = "data/v2-execution/aura-v2.db";
const LEGACY: &str = "data/aura.db";

fn answer(data_root: &Path, replacing: Option<&str>) -> serde_json::Value {
    let mut core = Command::new(env!("CARGO_BIN_EXE_aura-core"));
    core.arg("--store-migration-check").arg(data_root);
    if let Some(version) = replacing {
        core.arg("--replacing").arg(version);
    }
    let output = core.output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

/// 0.1.7 refuses a context snapshot whose estimates are `approximate`, and reads the newest ones
/// of several conversations before every request: a store this build wrote would leave it unable
/// to send, so the store is copied although its schema stays the same.
#[test]
fn a_current_execution_store_is_copied_before_replacing_a_version_that_refuses_its_records() {
    let data_root = tempfile::tempdir().unwrap();
    std::fs::create_dir(data_root.path().join("data")).unwrap();
    drop(
        aura_persistence_v2::V2Store::create(&data_root.path().join("data").join("v2-execution"))
            .unwrap(),
    );

    assert_eq!(answer(data_root.path(), Some("0.1.7"))[EXECUTION], true);
    assert_eq!(answer(data_root.path(), Some("0.1.6"))[EXECUTION], true);
    assert_eq!(
        answer(data_root.path(), None)[EXECUTION],
        true,
        "a launcher that does not say what it replaces may be replacing any older version"
    );
    let readable = aura_persistence_v2::RECORDS_READABLE_SINCE;
    assert_eq!(answer(data_root.path(), Some(readable))[EXECUTION], false);
    assert_eq!(answer(data_root.path(), Some("0.1.7"))[LEGACY], false, "there is no legacy store");
}

#[test]
fn a_missing_execution_store_is_never_copied() {
    let data_root = tempfile::tempdir().unwrap();
    assert_eq!(answer(data_root.path(), Some("0.1.7"))[EXECUTION], false);
    assert_eq!(answer(data_root.path(), None)[EXECUTION], false);
}
