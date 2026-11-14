use aura_hooks::{HookPackageError, scan_package};
use std::fs;

const MANIFEST: &str = r#"
schema_version = 1
name = "notify"
version = "1.0.0"
events = ["turn.completed"]
[command]
powershell_script = "scripts/notify.ps1"
"#;

fn package() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temporary package");
    fs::create_dir_all(root.path().join("scripts")).expect("folder");
    fs::write(root.path().join("hook.toml"), MANIFEST).expect("manifest");
    fs::write(root.path().join("scripts/notify.ps1"), "Write-Output '{\"decision\":\"continue\"}'")
        .expect("script");
    root
}

#[test]
fn the_fingerprint_covers_every_file_and_changes_with_any_of_them() {
    let root = package();
    let first = scan_package(root.path()).expect("valid package");
    assert_eq!(first.definition.name, "notify");
    assert_eq!(
        first.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["hook.toml", "scripts/notify.ps1"]
    );
    assert_eq!(scan_package(root.path()).expect("same").fingerprint, first.fingerprint);
    fs::write(root.path().join("README.md"), "notes").expect("extra file");
    let added = scan_package(root.path()).expect("still valid");
    assert_ne!(added.fingerprint, first.fingerprint, "a new file is new content");
    fs::write(root.path().join("scripts/notify.ps1"), "Write-Output 'changed'").expect("edit");
    assert_ne!(scan_package(root.path()).expect("valid").fingerprint, added.fingerprint);
}

#[test]
fn a_missing_program_or_manifest_is_refused_with_the_reason() {
    let root = package();
    fs::remove_file(root.path().join("scripts/notify.ps1")).expect("remove");
    assert!(matches!(
        scan_package(root.path()),
        Err(HookPackageError::Invalid(reason)) if reason.contains("scripts/notify.ps1")
    ));
    fs::remove_file(root.path().join("hook.toml")).expect("remove");
    assert_eq!(
        scan_package(root.path()).expect_err("no manifest"),
        HookPackageError::MissingManifest
    );
    assert_eq!(
        scan_package(&root.path().join("absent")).expect_err("absent"),
        HookPackageError::NotAFolder
    );
}

#[test]
fn a_linked_folder_inside_or_as_the_package_is_never_read() {
    let root = package();
    let outside = tempfile::tempdir().expect("outside");
    fs::write(outside.path().join("secret.txt"), "outside content").expect("outside file");
    junction::create(outside.path(), root.path().join("linked")).expect("junction");
    assert!(
        matches!(scan_package(root.path()), Err(HookPackageError::Linked(path)) if path == "linked")
    );
    let parent = tempfile::tempdir().expect("parent");
    let linked_root = parent.path().join("hook");
    junction::create(package().keep(), &linked_root).expect("junction to a package");
    assert!(matches!(scan_package(&linked_root), Err(HookPackageError::Linked(_))));
}

#[test]
fn too_many_files_are_refused() {
    let root = package();
    for index in 0..=aura_hooks::MAX_PACKAGE_FILES {
        fs::write(root.path().join(format!("f{index}.txt")), "x").expect("file");
    }
    assert_eq!(scan_package(root.path()).expect_err("too many"), HookPackageError::TooLarge);
}
