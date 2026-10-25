use aura_domain_common::WorkspacePath;
use aura_fs_tools::vault::{VaultChange, VaultFileSystem};
use aura_windows_platform::publication_fault::{self, Stage};

fn change(path: &str, old: Option<&[u8]>, bytes: &[u8]) -> VaultChange {
    VaultChange {
        path: WorkspacePath::parse(path).expect("path"),
        expected_hash: old.map(|b| blake3::hash(b).to_hex().to_string()),
        bytes: Some(bytes.to_vec()),
    }
}
#[test]
fn failure_in_second_publication_compensates_first_without_changing_originals() {
    let temp = tempfile::tempdir().expect("TEMP");
    let root = temp.path().join("vault");
    let fs = VaultFileSystem::new(root.clone()).expect("explicit root");
    fs.apply(&[change("a.md", None, b"original a"), change("b.md", None, b"original b")])
        .expect("create");
    // Use the existing real native publication fault, not a stand-in writer.
    let _fault = publication_fault::arm(Stage::DeleteBackupOnCommit, &root.join("b.md"));
    assert!(
        fs.apply(&[
            change("a.md", Some(b"original a"), b"new a"),
            change("b.md", Some(b"original b"), b"new b")
        ])
        .is_err()
    );
    assert_eq!(std::fs::read(root.join("a.md")).expect("a"), b"original a");
    assert_eq!(std::fs::read(root.join("b.md")).expect("b"), b"original b");
}

#[test]
fn existing_vault_below_a_junction_ancestor_is_rejected() {
    let temp = tempfile::tempdir().expect("TEMP");
    let actual = temp.path().join("actual");
    std::fs::create_dir_all(actual.join("memories")).expect("actual fixture");
    let alias = temp.path().join("alias");
    junction::create(&actual, &alias).expect("fixture junction");
    assert!(
        VaultFileSystem::new(alias.join("memories")).is_err(),
        "a normal leaf must not conceal a junction ancestor"
    );
}
