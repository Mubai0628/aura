use super::support::Fixture;
use aura_workspace_domain::{GitReadInputV1, GitReadResultV1};
use serde_json::json;

#[test]
fn desktop_untracked_diff_is_raw_readonly_and_pages_on_utf8_boundaries() {
    let f = Fixture::new("");
    f.file("tracked.txt", b"base\n");
    f.commit("base");
    let path = "新增 文件.txt";
    f.file(path, "第一行\n第二行\n".repeat(5000).as_bytes());
    let index = f.root.path().join(".git/index");
    let before = (
        std::fs::read(&index).expect("index"),
        std::fs::metadata(&index).expect("metadata").modified().expect("time"),
    );
    let query: GitReadInputV1 = serde_json::from_value(
        json!({"kind":"untracked_diff","input":{"path":path,"max_bytes":1024}}),
    )
    .expect("desktop-only untracked query is supported");
    let GitReadResultV1::Diff(first) = f.reader.read(&query).expect("untracked read") else {
        panic!("diff result");
    };
    assert_eq!(first.files.len(), 1);
    assert_eq!(first.files[0].path, path);
    assert_eq!(first.files[0].added, Some(10_000));
    assert_eq!(first.files[0].deleted, Some(0));
    assert!(first.patch.len() <= 1024 && first.truncated);
    let next = first.next_offset.expect("next part");
    let query: GitReadInputV1 = serde_json::from_value(
        json!({"kind":"untracked_diff","input":{"path":path,"offset_bytes":next,"max_bytes":1024}}),
    )
    .expect("query");
    let GitReadResultV1::Diff(second) = f.reader.read(&query).expect("next part") else {
        panic!("diff result");
    };
    assert_eq!(second.offset_bytes, next);
    assert_eq!(second.total_bytes, first.total_bytes);
    assert!(!second.patch.is_empty());
    assert_eq!(
        (
            std::fs::read(&index).expect("index"),
            std::fs::metadata(&index).expect("metadata").modified().expect("time")
        ),
        before
    );
}

fn query(path: &str) -> GitReadInputV1 {
    serde_json::from_value(json!({"kind":"untracked_diff","input":{"path":path}}))
        .expect("desktop query")
}

#[test]
fn desktop_untracked_diff_handles_binary_and_empty_files_and_refuses_other_scopes() {
    let f = Fixture::new("");
    f.file("tracked.txt", b"base\n");
    f.commit("base");
    f.file("new.bin", &[0, 1, 2, 255]);
    f.file("empty.txt", b"");
    f.file(".gitignore", b"ignored.txt\n");
    f.file("ignored.txt", b"ignored\n");
    let GitReadResultV1::Diff(binary) = f.reader.read(&query("new.bin")).expect("binary") else {
        panic!("diff");
    };
    assert!(binary.files[0].binary);
    assert_eq!((binary.files[0].added, binary.files[0].deleted), (None, None));
    let GitReadResultV1::Diff(empty) = f.reader.read(&query("empty.txt")).expect("empty") else {
        panic!("diff");
    };
    assert_eq!((empty.files[0].added, empty.files[0].deleted), (Some(0), Some(0)));
    assert!(!empty.patch.contains("@@"), "an empty new file can still have mode-only metadata");
    for path in ["tracked.txt", "ignored.txt", "../outside.txt", ".", ".git/config"] {
        assert!(f.reader.read(&query(path)).is_err(), "must refuse {path}");
    }
}

#[test]
fn desktop_untracked_diff_suppresses_required_filters_and_textconv_with_an_accurate_note() {
    let f = Fixture::new(
        "[filter \"Bad\"]\nclean = cmd.exe /d /c echo forbidden > program.marker\nsmudge = cmd.exe /d /c echo forbidden > program.marker\nprocess = cmd.exe /d /c echo forbidden > program.marker\nrequired = true\n[diff \"Bad\"]\ntextconv = cmd.exe /d /c echo forbidden > program.marker\n",
    );
    f.file("tracked.txt", b"base\n");
    f.commit("base");
    f.file(".gitattributes", b"*.raw filter=Bad diff=Bad\n");
    f.file("new.raw", b"raw data\n");
    let GitReadResultV1::Diff(diff) =
        f.reader.read(&query("new.raw")).expect("isolated raw read despite required filter")
    else {
        panic!("diff");
    };
    assert_eq!(diff.files[0].added, Some(1));
    assert_eq!(diff.files[0].comparison_note.as_deref(), Some("未运行外部程序，按原始内容比较"));
    assert!(!f.root.path().join("program.marker").exists());
}
