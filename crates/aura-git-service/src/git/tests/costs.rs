//! One review read
//! started about ten Git processes. Opening a repository now takes two, and each read two more at
//! most; the review page lists files by their line counts alone.
use super::support::Fixture;
use aura_workspace_domain::{GitDiffResultV1, GitReadInputV1, GitReadResultV1};
use serde_json::{Value, json};

fn query(value: Value) -> GitReadInputV1 {
    serde_json::from_value(value).expect("desktop query")
}

fn diff(result: GitReadResultV1) -> GitDiffResultV1 {
    match result {
        GitReadResultV1::Diff(diff) => diff,
        _ => panic!("not a diff"),
    }
}

#[test]
fn a_review_read_starts_at_most_four_git_processes() {
    let f = Fixture::new("");
    f.file("tracked.txt", b"base\n");
    f.file("renamed-from.txt", b"one\ntwo\nthree\n");
    f.commit("base");
    f.file("tracked.txt", b"base\nchanged\n");
    f.git(&["mv", "renamed-from.txt", "renamed-to.txt"]);
    f.file("new.txt", b"new\n");
    for (name, read) in [
        (
            "status",
            json!({"kind":"status","input":{"include_untracked":true,"include_branches":true,"offset":0,"limit":2000}}),
        ),
        (
            "file list",
            json!({"kind":"review_diff","input":{"base":null,"paths":[],"stats_only":true,"max_bytes":1}}),
        ),
        (
            "one file",
            json!({"kind":"review_diff","input":{"base":null,"paths":["tracked.txt"],"max_bytes":65536}}),
        ),
        ("untracked file", json!({"kind":"untracked_diff","input":{"path":"new.txt"}})),
    ] {
        let reader = f.reopen();
        reader.facts().expect("repository facts");
        reader.read(&query(read)).expect(name);
        assert!(reader.processes() <= 4, "{name}: {} Git processes", reader.processes());
    }
}

#[test]
fn the_file_list_has_every_file_and_its_counts_but_no_patch() {
    let f = Fixture::new("");
    f.file("tracked.txt", b"base\n");
    f.file("renamed-from.txt", b"one\ntwo\nthree\n");
    f.file("image.bin", &[0, 1, 2]);
    f.commit("base");
    f.file("tracked.txt", b"base\nchanged\n");
    f.git(&["mv", "renamed-from.txt", "renamed-to.txt"]);
    f.file("image.bin", &[0, 1, 2, 3]);
    let list = diff(
        f.reader
            .read(&query(json!({"kind":"review_diff","input":{"stats_only":true,"max_bytes":1}})))
            .expect("file list"),
    );
    let full = diff(
        f.reader
            .read(&query(json!({"kind":"review_diff","input":{"max_bytes":65536}})))
            .expect("whole comparison"),
    );
    assert_eq!(serde_json::to_value(&list.files).ok(), serde_json::to_value(&full.files).ok());
    assert!(list.files.iter().any(|file| file.path == "renamed-to.txt"
        && file.original_path.as_deref() == Some("renamed-from.txt")));
    assert!(list.files.iter().any(|file| file.path == "image.bin" && file.binary));
    assert_eq!((list.patch.as_str(), list.total_bytes, list.next_offset), ("", 0, None));
    assert!(full.patch.contains("+changed") && full.total_bytes > 0);
}
