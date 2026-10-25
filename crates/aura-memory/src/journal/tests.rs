//! A batch ends at each stage exactly as if the process had been terminated
//! there; the next open must leave all of it or none of it.
use super::crash::{self, Stage};
use crate::*;
use aura_domain_common::{WorkspaceId, WorkspacePath};
use std::{path::PathBuf, sync::Arc};

struct Case {
    temp: tempfile::TempDir,
    root: PathBuf,
    scope: VaultScope,
}
impl Case {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("TEMP");
        std::fs::create_dir(temp.path().join("workspace")).expect("workspace");
        Self {
            root: temp.path().join("workspace/.memories"),
            scope: VaultScope::Project { workspace_id: WorkspaceId::new() },
            temp,
        }
    }
    fn open(&self) -> Arc<Vault> {
        Vault::open(self.root.clone(), self.scope, &self.temp.path().join("cache/memory.db"))
            .expect("Vault")
    }
    fn pending(&self) -> usize {
        std::fs::read_dir(self.temp.path().join("cache/memory.records/pending"))
            .map_or(0, |entries| entries.count())
    }
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.root.join(path)).ok()
    }
    /// An existing note, then a batch that updates it and creates another.
    fn seeded(&self, vault: &Vault) -> (Vec<u8>, [MemoryWrite; 2]) {
        let first = note("rules/a.md", "# 原约定\n");
        let id = first.note.metadata.id;
        let hash = vault.write(&[first], &actor()).expect("seed")[0].clone();
        let mut update = vault.read_id(id).expect("seeded note");
        update.note.body = "# 新约定\n".into();
        let batch = [
            MemoryWrite { path: update.path, note: update.note, expected_hash: Some(hash) },
            note("rules/b.md", "# 新增\n"),
        ];
        (self.read("rules/a.md").expect("seeded bytes"), batch)
    }
}
fn note(path: &str, body: &str) -> MemoryWrite {
    MemoryWrite {
        path: WorkspacePath::parse(path).expect("path"),
        note: MemoryDocument {
            metadata: MemoryMetadata::new(MemoryScope::Project, "rules".into()),
            body: body.into(),
        },
        expected_hash: None,
    }
}
fn actor() -> WriteActor {
    WriteActor { kind: "model_tool".into(), ..Default::default() }
}

#[test]
fn a_batch_ended_before_its_commit_is_undone_at_the_next_open() {
    for stage in [Stage::Published(1), Stage::Published(2)] {
        let case = Case::new();
        let vault = case.open();
        let (original, batch) = case.seeded(&vault);
        crash::arm(stage);
        assert!(vault.write(&batch, &actor()).is_err());
        assert!(case.pending() > 0, "{stage:?}: the journal outlives the process");
        drop(vault);
        let vault = case.open();
        assert_eq!(case.read("rules/a.md"), Some(original), "{stage:?}");
        assert_eq!(case.read("rules/b.md"), None, "{stage:?}");
        assert_eq!(case.pending(), 0, "{stage:?}");
        let history = vault.history(None, 50).expect("history");
        assert!(
            history.iter().any(|h| h.action == "interrupted_undo" && h.path == "rules/a.md"),
            "{stage:?}: {history:?}"
        );
        // The undone batch never appears as an update or creation that happened.
        assert!(
            history
                .iter()
                .all(|h| h.action != "update" && !(h.action == "create" && h.path == "rules/b.md")),
            "{stage:?}: {history:?}"
        );
        assert!(history.iter().all(|h| h.actor != "external"), "{stage:?}: {history:?}");
        vault.write(&[note("rules/c.md", "# 之后\n")], &actor()).expect("writes resume");
    }
}

#[test]
fn a_batch_ended_after_its_commit_keeps_every_file_and_records_history_once() {
    for stage in [Stage::Committed, Stage::Recorded] {
        let case = Case::new();
        let vault = case.open();
        let (_, batch) = case.seeded(&vault);
        crash::arm(stage);
        assert!(vault.write(&batch, &actor()).is_err());
        drop(vault);
        let vault = case.open();
        assert!(
            String::from_utf8(case.read("rules/a.md").expect("a"))
                .expect("utf8")
                .contains("新约定")
        );
        assert!(case.read("rules/b.md").is_some(), "{stage:?}");
        assert_eq!(case.pending(), 0, "{stage:?}");
        let history = vault.history(None, 50).expect("history");
        let count = |action: &str, path: &str| {
            history.iter().filter(|h| h.action == action && h.path == path).count()
        };
        assert_eq!(count("update", "rules/a.md"), 1, "{stage:?}: {history:?}");
        assert_eq!(count("create", "rules/b.md"), 1, "{stage:?}: {history:?}");
        assert!(history.iter().all(|h| !h.action.starts_with("interrupted")), "{stage:?}");
        assert!(history.iter().all(|h| h.actor != "external"), "{stage:?}: {history:?}");
        assert_eq!(vault.status().expect("status").valid, 2, "{stage:?}");
    }
}

#[test]
fn an_interrupted_first_write_leaves_no_half_set_up_vault() {
    // The first write publishes the note, README and .gitignore in that order.
    for stage in [Stage::Published(1), Stage::Published(2), Stage::Published(3)] {
        let case = Case::new();
        let vault = case.open();
        crash::arm(stage);
        assert!(vault.write(&[note("rules/a.md", "# 第一条\n")], &actor()).is_err());
        drop(vault);
        let vault = case.open();
        for leaf in ["rules/a.md", "README.md", ".gitignore"] {
            assert_eq!(case.read(leaf), None, "{stage:?}: {leaf}");
        }
        vault.write(&[note("rules/a.md", "# 第一条\n")], &actor()).expect("retry");
        assert_eq!(case.read(".gitignore").as_deref(), Some(b"*\n".as_slice()), "{stage:?}");
        assert!(case.read("README.md").is_some(), "{stage:?}");
    }
}

#[test]
fn a_file_changed_after_the_interruption_is_kept_and_recorded() {
    let case = Case::new();
    let vault = case.open();
    let (original, batch) = case.seeded(&vault);
    crash::arm(Stage::Published(2));
    assert!(vault.write(&batch, &actor()).is_err());
    drop(vault);
    std::fs::write(case.root.join("rules/b.md"), "用户在中断后写下的内容\n").expect("user edit");
    let vault = case.open();
    assert_eq!(case.read("rules/a.md"), Some(original));
    assert_eq!(case.read("rules/b.md").as_deref(), Some("用户在中断后写下的内容\n".as_bytes()));
    let history = vault.history(None, 50).expect("history");
    assert!(history.iter().any(|h| h.action == "interrupted_undo" && h.path == "rules/a.md"));
    assert!(history.iter().any(|h| h.action == "interrupted_kept" && h.path == "rules/b.md"));
}

#[test]
fn an_ordinary_failure_restores_every_changed_file_and_says_so() {
    use aura_windows_platform::publication_fault::{self, Stage as Fault};
    let case = Case::new();
    let vault = case.open();
    let (original, [update, create]) = case.seeded(&vault);
    // The new note publishes first; replacing the seeded note then fails for real.
    let _fault = publication_fault::arm(Fault::DeleteBackupOnCommit, &case.root.join("rules/a.md"));
    let error = vault.write(&[create, update], &actor()).expect_err("second file fails");
    assert!(matches!(error, MemoryError::RolledBack { .. }), "{error:?}");
    assert_eq!(case.read("rules/a.md"), Some(original));
    assert_eq!(case.read("rules/b.md"), None);
    assert_eq!(case.pending(), 0);
    let history = vault.history(None, 50).expect("history");
    assert!(history.iter().all(|h| h.path != "rules/b.md"), "{history:?}");
    drop(_fault);
    vault.write(&[note("rules/c.md", "# 之后\n")], &actor()).expect("writes continue");
}

#[test]
fn a_replacement_ended_between_its_two_renames_is_restored_from_the_journal() {
    let case = Case::new();
    let vault = case.open();
    let (original, batch) = case.seeded(&vault);
    crash::arm(Stage::Published(2));
    assert!(vault.write(&batch, &actor()).is_err());
    drop(vault);
    // The old file was moved aside and the new one had not taken its name yet.
    std::fs::remove_file(case.root.join("rules/a.md")).expect("the window between renames");
    let vault = case.open();
    assert_eq!(case.read("rules/a.md"), Some(original));
    assert_eq!(case.read("rules/b.md"), None);
    let history = vault.history(None, 50).expect("history");
    let undo = history
        .iter()
        .find(|h| h.action == "interrupted_undo" && h.path == "rules/a.md")
        .expect("recorded");
    assert_eq!(undo.before_hash, None, "the record says the file was absent");
}
