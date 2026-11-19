use crate::support;
use aura_domain_common::{UtcTimestamp, WorkspaceId};
use aura_memory::*;
use support::*;

#[test]
fn keyword_recall_finds_reordered_terms_and_file_names_without_inactive_notes() {
    let f = Fixture::project();
    let mut replaced = f.write("rules/replaced.md", "# Atlas retrieval handbook\nOld value");
    replaced.note.metadata.status = MemoryStatus::Superseded;
    f.vault
        .write(
            &[
                f.write("rules/current.md", "# Handbook\nThe Atlas project tracks retrieval."),
                f.write("rules/exact-file.md", "# Unrelated title\nStable format"),
                replaced,
            ],
            &actor(),
        )
        .expect("notes");
    for (query, expected) in [
        ("retrieval Atlas handbook", "rules/current.md"),
        ("rules/exact-file.md", "rules/exact-file.md"),
    ] {
        let result = f
            .vault
            .search(&MemorySearch { query: query.into(), ..Default::default() })
            .expect("indexed recall");
        assert_eq!(result.hits.first().map(|hit| hit.path.as_str()), Some(expected));
        assert!(result.hits.iter().all(|hit| hit.path != "rules/replaced.md"));
    }
}

#[test]
fn final_path_aliases_share_one_writer_and_cannot_change_the_vault_scope() {
    let f = Fixture::project();
    f.vault.write(&[f.write("rules/a.md", "# alias\n")], &actor()).expect("write");
    let canonical = std::fs::canonicalize(&f.root).expect("final path");
    let alias = Vault::open(canonical.clone(), f.vault.scope(), &f.temp.path().join("alias.db"))
        .expect("same Vault");
    assert!(
        std::sync::Arc::ptr_eq(&alias, &f.vault),
        "a final-path spelling must not create a second writer"
    );
    assert!(
        Vault::open(
            canonical,
            VaultScope::Project { workspace_id: WorkspaceId::new() },
            &f.temp.path().join("other-scope.db")
        )
        .is_err(),
        "one directory cannot acquire a second workspace authority"
    );
}

#[test]
fn list_recent_is_chronological_instead_of_importance_ranked() {
    let f = Fixture::project();
    let mut old = f.write("rules/old.md", "# old\n");
    old.note.metadata.importance = 5;
    old.note.metadata.updated_at = "2026-10-01T00:00:00Z".parse().expect("date");
    let mut new = f.write("rules/new.md", "# new\n");
    new.note.metadata.importance = 1;
    new.note.metadata.updated_at = UtcTimestamp::now();
    f.vault.write(&[old, new], &actor()).expect("notes");
    let service = MemoryService::new(VaultRoots::new(
        f.temp.path().join("global"),
        f.temp.path().join("index"),
    ));
    let VaultScope::Project { workspace_id } = f.vault.scope() else { panic!("project") };
    let workspace =
        MemoryWorkspace { workspace_id, root: f.root.parent().expect("workspace").into() };
    let value = service
        .invoke(
            "memory.list_recent",
            &serde_json::json!({"scope":"project","limit":1}),
            &workspace,
            &actor(),
        )
        .expect("recent");
    assert_eq!(value["hits"][0]["title"], "new");
}
