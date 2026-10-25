mod support;
use support::{Fixture, actor};

#[test]
fn deleting_only_derived_sqlite_rebuilds_notes_and_metadata_only_history_from_files() {
    let fixture = Fixture::project();
    let write =
        fixture.write("rules/a.md", "# Original title\nBody must not be in the history journal.\n");
    let Fixture { temp, root, vault } = fixture;
    let id = write.note.metadata.id;
    let scope = vault.scope();
    let hashes = vault.write(&[write], &actor()).expect("create");
    vault.delete(&[(id, hashes[0].clone())], &actor()).expect("delete");
    assert_eq!(vault.history(Some(id), 10).expect("history").len(), 2);
    drop(vault);
    let records = temp.path().join("cache/memory.records/history");
    for file in std::fs::read_dir(records).expect("metadata journal") {
        let text = std::fs::read_to_string(file.expect("entry").path()).expect("metadata only");
        assert!(!text.contains("Body must not be"));
        assert!(text.contains("Original title"));
    }
    std::fs::remove_file(temp.path().join("cache/memory.db")).expect("delete derived cache only");
    let reopened = aura_memory::Vault::open(root, scope, &temp.path().join("cache/memory.db"))
        .expect("rebuild");
    let history = reopened.history(Some(id), 10).expect("replayed metadata history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].action, "delete");
    assert_eq!(reopened.status().expect("status").valid, 0);
}

#[test]
fn history_stays_in_monthly_segments_instead_of_a_file_per_write() {
    let fixture = Fixture::project();
    for i in 0..40 {
        let write = fixture.write(
            &format!("rules/n{i}.md"),
            &format!(
                "# 第 {i} 条
"
            ),
        );
        fixture.vault.write(&[write], &actor()).expect("write");
    }
    let Fixture { temp, root, vault } = fixture;
    let records = temp.path().join("cache/memory.records/history");
    assert_eq!(
        std::fs::read_dir(&records).expect("history").count(),
        1,
        "one segment, not a file per batch"
    );
    assert_eq!(vault.history(None, 100).expect("history").len(), 40);
    let scope = vault.scope();
    drop(vault);
    // Rebuilding from the segment yields the same records, each once.
    std::fs::remove_file(temp.path().join("cache/memory.db")).expect("derived cache only");
    let reopened = aura_memory::Vault::open(root, scope, &temp.path().join("cache/memory.db"))
        .expect("rebuild");
    assert_eq!(reopened.history(None, 100).expect("history").len(), 40);
}
