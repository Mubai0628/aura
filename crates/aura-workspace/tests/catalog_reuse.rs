//! What a second and third open of the same catalog reuse, and what they refuse to.
//!
//! The defect these cases come from is a startup cost, not a wrong answer: every
//! open re-read, decoded and hashed the text of every file in the workspace, so a
//! real development workspace spent most of its connect budget re-deriving facts
//! the previous open already had. The fix may only reuse a fact while a proof still
//! covers it — object identity, size, and the volume's change generation read
//! through a handle that excluded writers — and these cases pin both halves: what is
//! reused, and everything that must *not* be.
#![allow(clippy::unwrap_used)]
use std::sync::Arc;

use aura_domain_common::{CorrelationId, RuntimeInstanceId};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::WorkspaceCreateCommand;

fn persistence(data: &std::path::Path) -> Arc<PersistenceCoordinator> {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        data.join("aura.sqlite3"),
        data.join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("persistence startup")
    {
        PersistenceStartup::Ready(value) => Arc::new(value),
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    }
}

/// An open whose first walks have finished: they check the saved index after the open returns.
fn opened(data: &std::path::Path) -> Arc<WorkspaceRuntime> {
    let runtime = WorkspaceRuntime::open(persistence(data), WorkspaceRuntimeOptions::default())
        .expect("runtime open");
    runtime.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walks");
    runtime
}

fn create_workspace(
    runtime: &WorkspaceRuntime,
    root: &std::path::Path,
) -> aura_domain_common::WorkspaceId {
    let workspace_id = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: root.display().to_string(),
                display_name: "Reuse".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create workspace")
        .workspace
        .workspace_id;
    runtime.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
    workspace_id
}

fn text_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("src/main.rs", "fn main() { println!(\"aura\"); }\n"),
        ("src/lib.rs", "pub fn aura() -> &'static str { \"aura\" }\n"),
        ("docs/readme.md", "# Aura\n\nA note long enough to be indexed.\n"),
        ("asset.bin", "\u{0}\u{1}\u{2}not text\u{0}"),
    ]
}

#[test]
fn a_second_open_reuses_the_text_its_proof_still_covers() {
    let data = tempfile::tempdir().expect("data root");
    let workspace_root = tempfile::tempdir().expect("workspace root");
    for (relative, content) in text_files() {
        let path = workspace_root.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
    }

    let runtime = opened(data.path());
    let workspace_id = create_workspace(&runtime, workspace_root.path());
    drop(runtime);

    // Creating the workspace already reconciled the tree, which is where every
    // file's proof was taken, so the next open reads no text at all.
    let first = opened(data.path());
    let reused = first.open_report().expect("report");
    assert!(
        reused.catalog_reused >= 3,
        "files with a live proof are reused instead of read: {reused:?}"
    );
    assert_eq!(reused.catalog_rehydrated, 0, "no text was read again: {reused:?}");
    assert_eq!(reused.catalog_removed, 0);
    let catalog = first.catalog(workspace_id).expect("catalog");
    let entry = catalog
        .entry(&aura_domain_common::WorkspacePath::parse("src/main.rs").expect("path"))
        .expect("entry")
        .expect("present");
    assert!(entry.content_hash.is_some(), "the reused entry still carries its digest");
    assert!(entry.change_usn.is_some(), "the reused entry still carries its proof");
    drop(first);

    // And reading nothing is not a one-off: the proofs survive the open that used
    // them, because the pass re-verifies them rather than consuming them.
    let second = opened(data.path());
    let again = second.open_report().expect("report");
    assert!(again.catalog_reused >= 3, "{again:?}");
    assert_eq!(again.catalog_rehydrated, 0, "{again:?}");
    assert_eq!(again.catalog_entries, reused.catalog_entries);
}

#[test]
fn a_same_length_rewrite_whose_listed_facts_are_put_back_is_read_again() {
    let data = tempfile::tempdir().expect("data root");
    let workspace_root = tempfile::tempdir().expect("workspace root");
    let target = workspace_root.path().join("src/main.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "fn main() { println!(\"first\"); }\n").unwrap();
    std::fs::write(workspace_root.path().join("other.txt"), "unchanged\n").unwrap();

    let runtime = opened(data.path());
    create_workspace(&runtime, workspace_root.path());
    drop(runtime);
    let runtime = opened(data.path());
    drop(runtime);
    let runtime = opened(data.path());
    assert!(runtime.open_report().expect("report").catalog_reused >= 2, "warm before the rewrite");
    drop(runtime);

    // Same length, and the modification time the index recorded is put back the way
    // an mtime-preserving rewrite presents it: the size and the time the scan
    // compares are both unchanged, so only the volume's own change generation can
    // see that the bytes are not the bytes that were read.
    let before = std::fs::read(&target).unwrap();
    let mut rewritten = before.clone();
    rewritten[15] = b'A';
    assert_eq!(rewritten.len(), before.len());
    std::fs::write(&target, &rewritten).unwrap();
    let listed = std::fs::metadata(&target)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .to_string();
    let connection = rusqlite::Connection::open(data.path().join("aura.sqlite3")).unwrap();
    connection
        .execute(
            "UPDATE workspace_catalog_entries SET modified_at=?1 WHERE relative_path='src/main.rs'",
            [&listed],
        )
        .unwrap();
    drop(connection);

    let runtime = opened(data.path());
    let report = runtime.open_report().expect("report");
    assert!(report.catalog_rehydrated >= 1, "the rewritten file must be read again: {report:?}");
    assert!(report.catalog_reused >= 1, "the untouched file is still reused: {report:?}");
}

#[test]
fn a_metadata_only_change_is_written_even_though_no_text_is_read() {
    let data = tempfile::tempdir().expect("data root");
    let workspace_root = tempfile::tempdir().expect("workspace root");
    // Not a text candidate, so no content is indexed for it at all: the only fact
    // that can move is the metadata the listing collects.
    std::fs::write(workspace_root.path().join("asset.bin"), [0_u8, 1, 2, 3]).unwrap();
    let runtime = opened(data.path());
    create_workspace(&runtime, workspace_root.path());
    drop(runtime);
    let runtime = opened(data.path());
    assert_eq!(runtime.open_report().expect("report").catalog_rehydrated, 0);
    drop(runtime);

    std::fs::write(workspace_root.path().join("asset.bin"), [0_u8, 1, 2, 3, 4, 5]).unwrap();

    let runtime = opened(data.path());
    let report = runtime.open_report().expect("report");
    assert_eq!(report.catalog_rehydrated, 0, "the file is still not text: {report:?}");
    assert!(report.catalog_metadata_changed >= 1, "the moved row is written: {report:?}");
    drop(runtime);

    let connection = rusqlite::Connection::open(data.path().join("aura.sqlite3")).unwrap();
    let stored: i64 = connection
        .query_row(
            "SELECT byte_length FROM workspace_catalog_entries WHERE relative_path='asset.bin'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, 6, "the persisted row describes the file as it is now");
}

#[test]
fn rows_without_a_proof_are_read_again_rather_than_trusted() {
    let data = tempfile::tempdir().expect("data root");
    let workspace_root = tempfile::tempdir().expect("workspace root");
    std::fs::write(workspace_root.path().join("note.txt"), "content\n").unwrap();
    let runtime = opened(data.path());
    create_workspace(&runtime, workspace_root.path());
    drop(runtime);

    // Strip the proof the way a database written by an older build would present
    // it: identity and text present, generation missing.
    let persistence = persistence(data.path());
    let connection = rusqlite::Connection::open(data.path().join("aura.sqlite3")).unwrap();
    connection.execute("UPDATE workspace_catalog_entries SET change_usn=NULL", []).unwrap();
    drop(connection);
    drop(persistence);

    let runtime = opened(data.path());
    let report = runtime.open_report().expect("report");
    assert_eq!(report.catalog_reused, 0, "a row with no proof is never a hit: {report:?}");
    assert!(report.catalog_rehydrated >= 1);
    drop(runtime);

    let runtime = opened(data.path());
    let report = runtime.open_report().expect("report");
    assert!(report.catalog_reused >= 1, "the re-read row carries a proof now: {report:?}");
}

/// With hundreds of entries the proofs are taken on
/// several threads; every decision is still the one a single pass makes, and in the same order.
#[test]
fn a_large_workspace_reuses_and_reads_again_exactly_what_one_pass_would() {
    let data = tempfile::tempdir().expect("data root");
    let workspace_root = tempfile::tempdir().expect("workspace root");
    let path_of = |index: usize| format!("part_{}/file_{index:03}.txt", index % 7);
    let files = 600_usize;
    for index in 0..files {
        let path = workspace_root.path().join(path_of(index));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("text of file {index}\n")).unwrap();
    }
    let runtime = opened(data.path());
    let workspace_id = create_workspace(&runtime, workspace_root.path());
    drop(runtime);

    let changed = [5_usize, 300, 599];
    for index in changed {
        let text = format!("changed text of file {index}, longer than it was\n");
        std::fs::write(workspace_root.path().join(path_of(index)), text).unwrap();
    }
    let second = opened(data.path());
    let report = second.open_report().expect("report");
    assert_eq!(report.catalog_rehydrated, changed.len() as u64, "{report:?}");
    assert_eq!(report.catalog_reused, (files - changed.len()) as u64, "{report:?}");
    let catalog = second.catalog(workspace_id).expect("catalog");
    for index in 0..files {
        let path = aura_domain_common::WorkspacePath::parse(path_of(index)).expect("path");
        let entry = catalog.entry(&path).expect("entry").expect("present");
        let text = std::fs::read(workspace_root.path().join(path_of(index))).unwrap();
        assert_eq!(entry.byte_length, Some(text.len() as u64), "{}", path_of(index));
        assert!(entry.content_hash.is_some(), "{}", path_of(index));
    }
}
