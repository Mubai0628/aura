//! Exporting a plugin as a ZIP file: exactly the files that were read, at the paths they had,
//! written to a temporary file next to the chosen one and then moved over it, so an export that
//! fails leaves the chosen file as it was. What is exported imports again as the same plugin.
mod support;
use aura_plugins::{PluginExportError, export_plugin, unpack_plugin_archive};
use std::fs::File;
use std::path::Path;
use support::{scan, write};

fn entries(path: &Path) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(File::open(path).expect("zip")).expect("a ZIP file");
    (0..archive.len())
        .map(|index| archive.by_index(index).expect("entry").name().to_owned())
        .collect()
}

fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .expect("folder")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn an_exported_plugin_imports_again_as_the_same_plugin() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    support::aura_plugin(&root);
    write(&root, ".git/HEAD", "ref: refs/heads/main\n");
    let package = scan(&root).expect("a plugin");
    let out = temp.path().join("out");
    std::fs::create_dir_all(&out).expect("folder");
    let destination = out.join("review-kit-1.2.0.zip");

    let exported = export_plugin(&package, &destination).expect("exported");
    assert_eq!(exported.files, package.files.len());
    assert_eq!(exported.bytes, package.bytes);
    assert_eq!(exported.archive_bytes, std::fs::metadata(&destination).expect("file").len());
    let expected: Vec<String> = package.files.iter().map(|file| file.path.clone()).collect();
    assert_eq!(entries(&destination), expected, "exactly the files that were read");
    assert_eq!(names_in(&out), ["review-kit-1.2.0.zip"], "nothing else is left beside it");

    let unpacked = tempfile::tempdir().expect("out");
    let root = unpack_plugin_archive(File::open(&destination).expect("zip"), unpacked.path())
        .expect("it imports");
    assert_eq!(scan(&root).expect("a plugin").fingerprint, package.fingerprint);

    // Exporting again replaces the file.
    export_plugin(&package, &destination).expect("exported again");
    assert_eq!(names_in(&out), ["review-kit-1.2.0.zip"]);
}

#[test]
fn a_file_changed_after_it_was_read_stops_the_export_and_leaves_the_chosen_file() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    support::aura_plugin(&root);
    let package = scan(&root).expect("a plugin");
    let destination = temp.path().join("review-kit.zip");
    std::fs::write(&destination, b"an earlier export").expect("an earlier file");

    for (path, text) in [
        // The same size, other bytes.
        ("skills/review-guide/SKILL.md", support::SKILL.replace("diff", "DIFF")),
        // Another size.
        ("bin/server.exe", "a longer program than it was".to_owned()),
    ] {
        let original = std::fs::read(root.join(path)).expect("original");
        write(&root, path, &text);
        let error = export_plugin(&package, &destination).expect_err("changed");
        assert!(matches!(error, PluginExportError::Changed(_)), "{error}");
        assert_eq!(std::fs::read(&destination).expect("kept"), b"an earlier export");
        std::fs::write(root.join(path), original).expect("restored");
    }
    // A file removed after it was read.
    std::fs::remove_file(root.join("bin/server.exe")).expect("removed");
    assert!(matches!(
        export_plugin(&package, &destination).expect_err("gone"),
        PluginExportError::Changed(_)
    ));
    assert_eq!(names_in(temp.path()), ["review-kit", "review-kit.zip"], "no temporary file left");
}

#[test]
fn only_a_zip_file_in_an_existing_folder_outside_the_plugin_is_written() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    support::aura_plugin(&root);
    let package = scan(&root).expect("a plugin");
    std::fs::create_dir_all(temp.path().join("taken.zip")).expect("a folder named like a file");

    for (destination, expected) in [
        (temp.path().join("review-kit.txt"), PluginExportError::NotZip),
        (root.join("review-kit.zip"), PluginExportError::InsidePlugin),
        (root.join("skills").join("review-kit.zip"), PluginExportError::InsidePlugin),
        (temp.path().join("missing").join("review-kit.zip"), PluginExportError::NoFolder),
        (temp.path().join("taken.zip"), PluginExportError::NotAFile),
    ] {
        assert_eq!(
            export_plugin(&package, &destination).expect_err("refused"),
            expected,
            "{}",
            destination.display()
        );
    }
    assert_eq!(scan(&root).expect("unchanged").fingerprint, package.fingerprint);
    // Upper-case extensions are ZIP files too.
    export_plugin(&package, &temp.path().join("REVIEW-KIT.ZIP")).expect("exported");
}
