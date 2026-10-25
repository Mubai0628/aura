//! A plugin's files: the fingerprint covers every file, `.git` is not part of a plugin, and links
//! or folders nested too deep are refused before anything is read.
mod support;
use aura_plugins::PluginPackageError;
use support::*;

#[test]
fn the_fingerprint_covers_every_file_and_changes_with_one_byte() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    aura_plugin(&root);
    let first = scan(&root).expect("a plugin").fingerprint;
    assert_eq!(scan(&root).expect("again").fingerprint, first, "stable");
    write(&root, "bin/server.exe", "not really a programme");
    let changed = scan(&root).expect("changed").fingerprint;
    assert_ne!(changed, first, "one changed byte");
    write(&root, "notes/extra.txt", "x");
    let added = scan(&root).expect("added");
    assert_ne!(added.fingerprint, changed, "one more file");
    assert!(added.files.iter().any(|f| f.path == "notes/extra.txt"));
    std::fs::rename(root.join("notes/extra.txt"), root.join("notes/moved.txt")).expect("rename");
    assert_ne!(scan(&root).expect("moved").fingerprint, added.fingerprint, "a renamed file");
}

#[test]
fn a_git_folder_is_not_part_of_the_plugin() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    aura_plugin(&root);
    let before = scan(&root).expect("a plugin").fingerprint;
    write(&root, ".git/HEAD", "ref: refs/heads/main\n");
    write(&root, ".git/objects/ab/cdef", "blob");
    let after = scan(&root).expect("a plugin");
    assert_eq!(after.fingerprint, before);
    assert!(after.files.iter().all(|f| !f.path.starts_with(".git")));
}

#[test]
fn a_link_inside_the_plugin_is_refused() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    aura_plugin(&root);
    let elsewhere = temp.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("target");
    junction::create(&elsewhere, root.join("linked")).expect("junction");
    assert!(matches!(scan(&root), Err(PluginPackageError::Linked(path)) if path == "linked"));
}

#[test]
fn folders_nested_too_deep_are_refused() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    aura_plugin(&root);
    let deep = (0..18).map(|n| format!("d{n}")).collect::<Vec<_>>().join("/");
    write(&root, &format!("{deep}/file.txt"), "x");
    assert_eq!(scan(&root).expect_err("too deep"), PluginPackageError::TooLarge);
}

#[test]
fn a_file_is_not_a_plugin_folder() {
    let temp = tempfile::tempdir().expect("temp");
    let file = temp.path().join("plugin.zip");
    std::fs::write(&file, "x").expect("file");
    assert_eq!(scan(&file).expect_err("a file"), PluginPackageError::NotAFolder);
}
