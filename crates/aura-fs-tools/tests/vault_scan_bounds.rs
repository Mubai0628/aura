use aura_fs_tools::vault::VaultFileSystem;
#[test]
fn empty_directory_trees_cannot_bypass_the_scan_work_bound() {
    let temp = tempfile::tempdir().expect("scan fixture");
    let root = temp.path().join("vault");
    std::fs::create_dir_all(root.join("a/b/c/d")).expect("scan fixture");
    let fs = VaultFileSystem::new(root).expect("scan fixture");
    assert!(
        fs.list_markdown(3).is_err(),
        "indexing must bound visited directories, not only matching Markdown files"
    );
}
