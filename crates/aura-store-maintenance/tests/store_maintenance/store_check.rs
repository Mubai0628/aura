//! `--store-check`: the quick check a move runs on its copy of the stores before it trusts them.
#![allow(clippy::unwrap_used)]
use std::io::{Seek, SeekFrom, Write};

use aura_persistence_v2::V2Store;
use serde_json::{Value, json};

fn run(root: &std::path::Path) -> (Option<u8>, Vec<Value>) {
    let arguments: Vec<String> =
        ["aura-core", "--store-check", root.to_str().unwrap()].map(String::from).to_vec();
    let mut out = Vec::new();
    let code = aura_store_maintenance::command(&arguments, &mut out);
    let lines = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (code, lines)
}

#[test]
fn a_folder_without_stores_passes_and_says_neither_is_there() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        run(root.path()),
        (Some(0), vec![json!({ "data/aura.db": null, "data/v2-execution/aura-v2.db": null })])
    );
    assert_eq!(aura_store_maintenance::check_stores(root.path()), Ok(()));
    let arguments = ["aura-core", "--store-check"].map(String::from);
    assert_eq!(aura_store_maintenance::command(&arguments, &mut Vec::new()), Some(2));
}

#[test]
fn a_sound_store_passes_and_a_damaged_page_fails_by_name() {
    let root = tempfile::tempdir().unwrap();
    let execution = root.path().join("data/v2-execution");
    std::fs::create_dir_all(root.path().join("data")).unwrap();
    let store = V2Store::create(&execution).unwrap();
    store.mark_clean_exit().unwrap();
    drop(store);
    assert_eq!(
        run(root.path()),
        (Some(0), vec![json!({ "data/aura.db": null, "data/v2-execution/aura-v2.db": true })])
    );

    let mut file =
        std::fs::OpenOptions::new().write(true).open(execution.join("aura-v2.db")).unwrap();
    file.seek(SeekFrom::Start(4096)).unwrap();
    file.write_all(&[0xFF; 64]).unwrap();
    drop(file);
    let (code, lines) = run(root.path());
    assert_eq!(code, Some(1));
    assert!(lines[0]["data/v2-execution/aura-v2.db"]["error"].is_string(), "{lines:?}");
    let failed = aura_store_maintenance::check_stores(root.path()).unwrap_err();
    assert!(failed.starts_with("data/v2-execution/aura-v2.db: "), "{failed}");
}
