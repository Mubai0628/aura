mod support;
use aura_domain_common::{UtcTimestamp, WorkspaceId};
use aura_memory::*;
use support::*;

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
