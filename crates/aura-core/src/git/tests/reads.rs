use super::support::Fixture;
use aura_workspace_domain::*;

#[test]
fn native_reads_empty_unicode_rename_binary_paging_and_root_commit() {
    let fixture = Fixture::new("");
    assert!(
        fixture.reader.status(&GitStatusInput::default()).expect("empty status").head.is_none()
    );
    assert!(
        fixture
            .reader
            .log(&serde_json::from_value(serde_json::json!({})).expect("log"))
            .expect("unborn log")
            .commits
            .is_empty()
    );
    fixture.file("汉字 space.txt", "first\nsecond\n".as_bytes());
    fixture.file("binary.bin", &[0, 1, 2, 3]);
    let root = fixture.commit("root subject\n\ncomplete body");
    let root_diff = fixture
        .reader
        .diff(&GitDiffInput {
            target: GitDiffTargetV1::Commit { commit: root.clone() },
            paths: vec![],
            offset_bytes: 0,
            max_bytes: 65536,
        })
        .expect("root diff");
    assert!(
        root_diff.files.iter().any(|file| file.path == "汉字 space.txt" && file.added == Some(2))
    );
    assert!(root_diff.files.iter().any(|file| file.path == "binary.bin" && file.binary));
    fixture.git(&["mv", "汉字 space.txt", "renamed 汉字.txt"]);
    fixture.file("new untracked.txt", b"untracked\n");
    let status = fixture
        .reader
        .status(&GitStatusInput { include_branches: true, ..Default::default() })
        .expect("status");
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert!(
        status.entries.iter().any(|entry| entry.original_path.as_deref() == Some("汉字 space.txt"))
    );
    assert!(
        status.entries.iter().any(|entry| entry.untracked && entry.path == "new untracked.txt")
    );
    assert!(!status.submodule_contents_checked);
    let page = fixture
        .reader
        .status(&GitStatusInput { limit: 1, ..Default::default() })
        .expect("status page");
    assert_eq!(page.next_offset, Some(1));
    let input = GitDiffInput {
        target: GitDiffTargetV1::Staged {},
        paths: vec![],
        offset_bytes: 0,
        max_bytes: 7,
    };
    let mut patch = String::new();
    let mut offset = 0;
    loop {
        let page = fixture
            .reader
            .diff(&GitDiffInput { offset_bytes: offset, ..input.clone() })
            .expect("UTF8 patch page");
        patch.push_str(&page.patch);
        if let Some(next) = page.next_offset {
            assert!(next > offset);
            offset = next;
        } else {
            break;
        }
    }
    assert!(patch.contains("rename from 汉字 space.txt"));
    let log = fixture
        .reader
        .log(&serde_json::from_value(serde_json::json!({"commit":root})).expect("show input"))
        .expect("show");
    assert_eq!(log.commits.len(), 1);
    assert!(log.commits[0].body.contains("complete body") && !log.commits[0].files.is_empty());
}

#[test]
fn ranges_paths_log_limits_detached_and_first_parent_merge() {
    let fixture = Fixture::new("");
    fixture.file("a", b"base\n");
    let base = fixture.commit("base");
    fixture.git(&["switch", "-c", "topic"]);
    fixture.file("topic", b"topic\n");
    let topic = fixture.commit("topic");
    fixture.git(&["switch", "main"]);
    fixture.file("main", b"main\n");
    let first = fixture.commit("first parent");
    fixture.git(&["merge", "--no-ff", "topic", "-m", "merge"]);
    let merge = fixture
        .reader
        .diff(&GitDiffInput {
            target: GitDiffTargetV1::Commit { commit: "HEAD".into() },
            paths: vec![],
            offset_bytes: 0,
            max_bytes: 65536,
        })
        .expect("first-parent merge diff");
    assert_eq!(merge.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["topic"]);
    let range = fixture
        .reader
        .diff(&GitDiffInput {
            target: GitDiffTargetV1::Range { from: base, to: topic.clone() },
            paths: vec!["topic".into()],
            offset_bytes: 0,
            max_bytes: 65536,
        })
        .expect("range");
    assert_eq!(range.files.len(), 1);
    let log = fixture
        .reader
        .log(&serde_json::from_value(serde_json::json!({"limit":1})).expect("limit input"))
        .expect("log");
    assert_eq!(log.commits.len(), 1);
    assert!(log.truncated);
    fixture.git(&["checkout", "--detach", &first]);
    let status = fixture.reader.status(&GitStatusInput::default()).expect("detached");
    assert!(status.detached && status.branch.is_none());
    assert!(
        fixture
            .reader
            .diff(&GitDiffInput {
                target: GitDiffTargetV1::Commit { commit: "--output=canary".into() },
                paths: vec![],
                offset_bytes: 0,
                max_bytes: 65536
            })
            .is_err()
    );
}
