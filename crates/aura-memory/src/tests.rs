use super::*;
#[test]
fn search_reads_previous_index_during_rebuild_and_marks_results_incomplete() {
    let temp = tempfile::tempdir().expect("TEMP");
    let vault = Vault::open(
        temp.path().join("global"),
        VaultScope::Global,
        &temp.path().join("index/memory.db"),
    )
    .expect("Vault");
    vault
        .write(
            &[MemoryWrite {
                path: aura_domain_common::WorkspacePath::parse("rules/test.md").expect("path"),
                note: MemoryDocument {
                    metadata: MemoryMetadata::new(MemoryScope::Global, "rules".into()),
                    body: "# 构建约定\n".into(),
                },
                expected_hash: None,
            }],
            &WriteActor::default(),
        )
        .expect("write");
    let _writer = vault.lock();
    vault.rebuilding.store(true, std::sync::atomic::Ordering::Release);
    let result = vault
        .search(&MemorySearch { query: "约定".into(), ..Default::default() })
        .expect("independent readonly SQLite reader");
    assert!(result.incomplete);
    assert_eq!(result.hits.len(), 1);
}
