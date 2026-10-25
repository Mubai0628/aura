mod support;
use aura_domain_common::WorkspacePath;
use aura_memory::{MemorySearch, VaultWatcher};
use std::time::{Duration, Instant};
use support::{Fixture, actor};

#[test]
fn watcher_tracks_external_create_rename_invalid_edit_and_removal() {
    let f = Fixture::project();
    let _watcher = VaultWatcher::start(&f.vault).expect("watch absent Vault");
    assert!(!f.root.exists());
    let note = f.write("conventions/a.md", "# Rust 构建约定\n");
    let id = note.note.metadata.id;
    f.vault.write(&[note], &actor()).expect("write");
    std::fs::rename(f.root.join("conventions/a.md"), f.root.join("conventions/b.md"))
        .expect("rename");
    until(|| f.vault.read_id(id).is_ok_and(|r| r.path.as_str() == "conventions/b.md"));
    std::fs::write(f.root.join("conventions/b.md"), "not frontmatter").expect("invalid");
    until(|| f.vault.status().is_ok_and(|s| s.invalid == 1));
    assert_eq!(
        std::fs::read_to_string(f.root.join("conventions/b.md")).expect("unchanged"),
        "not frontmatter"
    );
    std::fs::remove_file(f.root.join("conventions/b.md")).expect("remove fixture");
    until(|| f.vault.status().is_ok_and(|s| s.invalid == 0));
}

#[test]
fn linked_files_and_reparse_directories_cannot_escape_vault() {
    let f = Fixture::project();
    let note = f.write("conventions/a.md", "# original\n");
    f.vault.write(&[note], &actor()).expect("write");
    let outside = f.temp.path().join("outside");
    std::fs::create_dir(&outside).expect("outside fixture");
    std::fs::hard_link(f.root.join("conventions/a.md"), outside.join("alias.md"))
        .expect("hardlink fixture");
    assert!(f.vault.read_path(&WorkspacePath::parse("conventions/a.md").expect("path")).is_err());
    junction::create(&outside, f.root.join("link")).expect("junction fixture");
    let bad = f.write("link/escape.md", "# forbidden\n");
    assert!(f.vault.write(&[bad], &actor()).is_err());
    assert!(!outside.join("escape.md").exists());
    for path in ["../other.md", "C:/outside.md", "/outside.md", "x:stream.md", "NUL.md"] {
        assert!(WorkspacePath::parse(path).is_err());
    }
}

#[test]
fn test_constructor_never_discovers_or_falls_back_to_real_profile() {
    // The service has no profile lookup or environment override: every call is
    // rooted under this exact TEMP tree, including failures/rebuild/watching.
    // The guard deliberately never opens or enumerates the owner's Vault.
    let f = Fixture::project();
    assert!(f.root.starts_with(f.temp.path()));
    let missing = f.temp.path().join("missing-cache");
    assert!(
        aura_memory::Vault::open(std::path::PathBuf::from("relative"), f.vault.scope(), &missing)
            .is_err()
    );
    f.vault.validate().expect("read-only validation");
    assert!(!f.root.exists());
    let _watcher = VaultWatcher::start(&f.vault).expect("watch injected root");
    f.vault
        .write(&[f.write("rules/temp.md", "# Only temporary test content\n")], &actor())
        .expect("write");
    let found = f.vault.search(&MemorySearch::default()).expect("search");
    assert_eq!(found.hits.len(), 1);
    assert!(f.root.join("rules/temp.md").exists());
}

fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "watcher state timeout");
        std::thread::sleep(Duration::from_millis(25));
    }
}
