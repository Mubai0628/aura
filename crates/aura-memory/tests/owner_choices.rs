mod support;
use aura_memory::MemoryWrite;
use support::{Fixture, actor};

#[test]
fn deleting_vault_gitignore_to_opt_into_versioning_survives_future_memory_writes() {
    let f = Fixture::project();
    f.vault.write(&[f.write("rules/first.md", "# First\n")], &actor()).expect("first write");
    std::fs::remove_file(f.root.join(".gitignore")).expect("owner opts into Git");
    f.vault.write(&[f.write("rules/next.md", "# Next\n")], &actor()).expect("next write");
    assert!(
        !f.root.join(".gitignore").exists(),
        "Aura must preserve the owner's documented opt-in"
    );
}

#[test]
fn superseding_a_note_atomically_marks_the_old_note_superseded() {
    let f = Fixture::project();
    let original = f.write("rules/original.md", "# Old decision\n");
    let id = original.note.metadata.id;
    f.vault.write(&[original], &actor()).expect("old note");
    let mut newer = f.write("rules/new.md", "# New verified decision\n");
    newer.note.metadata.supersedes_ids.push(id);
    f.vault.write(&[newer], &actor()).expect("supersede");
    assert_eq!(
        f.vault.read_id(id).expect("old note preserved").note.metadata.status,
        aura_memory::MemoryStatus::Superseded
    );
}

#[test]
fn an_obsidian_property_added_by_the_owner_stays_valid_and_survives_a_model_update() {
    let f = Fixture::project();
    let write = f.write("rules/a.md", "# 构建约定\n");
    let id = write.note.metadata.id;
    f.vault.write(&[write], &actor()).expect("create");
    let path = f.root.join("rules/a.md");
    let text = std::fs::read_to_string(&path).expect("note");
    std::fs::write(&path, text.replacen("---\n#", "aliases: [build]\n---\n#", 1)).expect("edit");
    f.vault.rebuild().expect("rebuild");
    assert_eq!(
        f.vault.status().expect("status").invalid,
        0,
        "the owner's property is not an error"
    );
    let mut read = f.vault.read_id(id).expect("still recallable");
    read.note.body = "# 构建约定\n更新。\n".into();
    let update = MemoryWrite { path: read.path, note: read.note, expected_hash: Some(read.hash) };
    f.vault.write(&[update], &actor()).expect("update");
    assert!(std::fs::read_to_string(&path).expect("note").contains("aliases: [build]"));
}

#[test]
fn an_empty_vault_folder_left_behind_still_gets_readme_and_gitignore() {
    // A folder without notes or README (left by a failed first write, or made by
    // hand) is not a set-up Vault; its first write must keep memories out of Git.
    let f = Fixture::project();
    std::fs::create_dir_all(f.root.join("rules")).expect("an empty .memories folder");
    f.vault
        .write(
            &[f.write(
                "rules/first.md",
                "# First
",
            )],
            &actor(),
        )
        .expect("first write");
    assert_eq!(
        std::fs::read_to_string(f.root.join(".gitignore")).expect("ignore"),
        "*
"
    );
    assert!(f.root.join("README.md").exists());
}
