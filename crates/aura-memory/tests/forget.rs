//! Removing a Workspace from Aura deletes the project memory index and records Aura kept in
//! its own cache directory; the Vault inside the Workspace belongs to the user and stays.
use aura_domain_common::WorkspaceId;
use aura_memory::*;

fn entries(path: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(path)
        .expect("cache directory")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn forgetting_a_project_deletes_aura_cache_files_and_keeps_the_vault() {
    let temp = tempfile::tempdir().expect("TEMP");
    let cache = temp.path().join("cache");
    std::fs::create_dir_all(&cache).expect("cache");
    let workspace = temp.path().join("workspace");
    let vault_root = workspace.join(".memories");
    std::fs::create_dir_all(&vault_root).expect("vault");
    std::fs::write(vault_root.join("user-note.txt"), "the user's memory").expect("user file");
    let service = MemoryService::new(VaultRoots::new(temp.path().join("global"), cache.clone()));
    let project = MemoryWorkspace { workspace_id: WorkspaceId::new(), root: workspace.clone() };
    drop(service.vault(MemoryScope::Project, &project).expect("open the project Vault"));
    assert!(
        entries(&cache).iter().any(|name| name.ends_with(".db")),
        "opening the Vault made its index: {:?}",
        entries(&cache)
    );

    assert!(service.forget_project(&workspace).expect("forget"));
    assert_eq!(entries(&cache), Vec::<String>::new(), "index and records are gone");
    assert_eq!(
        std::fs::read_to_string(vault_root.join("user-note.txt")).expect("the Vault stays"),
        "the user's memory"
    );
    assert!(!service.forget_project(&workspace).expect("again"), "nothing is left to delete");
    // The Vault opens again from scratch, with a new index.
    drop(service.vault(MemoryScope::Project, &project).expect("reopen"));
    assert!(entries(&cache).iter().any(|name| name.ends_with(".db")));
}
