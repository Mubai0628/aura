//! The launcher's two questions before the desktop starts, answered without Core's runtime: for
//! `--store-slim` a line per step and one report of both stores, for `--store-migration-check` one
//! answer for both stores.
#![allow(clippy::unwrap_used)]

use aura_domain_common::RuntimeInstanceId;
use aura_persistence::{PersistenceCoordinator, PersistenceOptions};
use serde_json::{Value, json};

fn run(arguments: &[&str]) -> (Option<u8>, Vec<Value>) {
    let arguments: Vec<String> =
        std::iter::once("aura-core").chain(arguments.iter().copied()).map(String::from).collect();
    let mut out = Vec::new();
    let code = aura_store_maintenance::command(&arguments, &mut out);
    let lines = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (code, lines)
}

/// A data folder holding the `aura.db` this build makes.
fn data_folder_with_aura_db() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let options = PersistenceOptions::new(
        root.path().join("data/aura.db"),
        root.path().join("data/artifact-objects"),
        RuntimeInstanceId::new(),
    );
    drop(PersistenceCoordinator::start(options).unwrap());
    root
}

#[test]
fn other_arguments_are_left_to_core() {
    assert_eq!(run(&["--pipe-name=x", "--bootstrap-stdin"]).0, None);
}

#[test]
fn a_command_without_its_data_folder_is_an_argument_error() {
    assert_eq!(run(&["--store-slim"]).0, Some(2));
    assert_eq!(run(&["--store-migration-check"]).0, Some(2));
}

#[test]
fn a_data_folder_without_stores_reports_neither() {
    let root = tempfile::tempdir().unwrap();
    let (code, lines) = run(&["--store-slim", root.path().to_str().unwrap()]);
    assert_eq!(code, Some(0));
    assert_eq!(lines, [json!({ "report": { "execution": null, "legacy": null } })]);
}

#[test]
fn a_slim_run_over_aura_db_reports_it_in_the_last_line_and_notes_what_it_kept() {
    let root = data_folder_with_aura_db();
    let (code, lines) = run(&["--store-slim", root.path().to_str().unwrap()]);
    assert_eq!(code, Some(0), "{lines:?}");
    let report = &lines.last().unwrap()["report"];
    assert_eq!(report["execution"], Value::Null);
    assert_eq!(report["legacy"]["rebuild"]["state"], "not_needed", "{report}");
    let marker = std::fs::read(root.path().join("data/aura-db-slimming.json")).unwrap();
    let marker: Value = serde_json::from_slice(&marker).unwrap();
    assert!(marker["kept_bytes"].as_u64().unwrap() > 0, "{marker}");
}

#[test]
fn a_store_whose_run_fails_reports_its_error_and_the_exit_code_says_so() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("data")).unwrap();
    std::fs::write(root.path().join("data/aura.db"), b"not a database at all").unwrap();
    let (code, lines) = run(&["--store-slim", root.path().to_str().unwrap()]);
    assert_eq!(code, Some(1));
    let report = &lines.last().unwrap()["report"];
    assert!(report["legacy"]["error"].is_string(), "{report}");
    assert_eq!(report["execution"], Value::Null);
}

#[test]
fn the_migration_check_answers_for_both_stores() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_str().unwrap();
    let (code, lines) = run(&["--store-migration-check", path, "--replacing", "0.2.0"]);
    assert_eq!(code, Some(0));
    assert_eq!(lines, [json!({ "data/aura.db": false, "data/v2-execution/aura-v2.db": false })]);
}
