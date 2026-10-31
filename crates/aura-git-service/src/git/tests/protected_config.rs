use super::support::Fixture;
use aura_workspace_domain::*;
use std::io::Write;

#[test]
fn textconv_is_disabled_in_all_scopes_and_only_affected_files_explain_raw_comparison() {
    let mut f = Fixture::new(
        "[color]\nui = always\n[core]\npager = false\nquotePath = true\n[diff]\nexternal = MUST_NOT_RUN\n[status]\nshowUntrackedFiles = no\n[filter \"DisabledOnly\"]\nrequired = off\n",
    );
    f.file("汉字 space.txt", b"old\n");
    f.file("ordinary.txt", b"old ordinary\n");
    f.commit("base");
    f.file("汉字 space.txt", b"new\n");
    f.file("ordinary.txt", b"new ordinary\n");
    f.file("untracked", b"visible\n");
    f.file(".gitattributes", "汉字* diff=Trusted\nordinary.txt filter=DisabledOnly\n".as_bytes());
    let mut global = std::fs::OpenOptions::new()
        .append(true)
        .open(f.root.path().join("global.cfg"))
        .expect("temporary global");
    writeln!(global, "[diff \"Trusted\"]\ntextconv = MUST_NOT_RUN_GLOBAL\ncachetextconv = true")
        .expect("protected converter");
    f.git(&["config", "diff.Trusted.textconv", "MUST_NOT_RUN_REPO"]);
    f.refresh();
    let index_path = f.root.path().join(".git/index");
    let before = std::fs::metadata(&index_path).expect("index").modified().expect("time");
    let bytes = std::fs::read(&index_path).expect("bytes");
    let status = f.reader.status(&GitStatusInput::default()).expect("interfering config status");
    assert!(status.entries.iter().any(|entry| entry.path == "untracked" && entry.untracked));
    let result = f
        .reader
        .diff(&serde_json::from_value(serde_json::json!({})).expect("defaults"))
        .expect("raw content without converters");
    assert_eq!(result.warnings, [super::super::attributes::RAW_COMPARISON]);
    assert!(result.patch.contains("+new"));
    for files in [
        &status
            .entries
            .iter()
            .map(|file| (file.path.as_str(), file.comparison_note.as_deref()))
            .collect::<Vec<_>>(),
        &result
            .files
            .iter()
            .map(|file| (file.path.as_str(), file.comparison_note.as_deref()))
            .collect::<Vec<_>>(),
    ] {
        assert!(
            files.contains(&("汉字 space.txt", Some(super::super::attributes::RAW_COMPARISON)))
        );
        assert!(files.contains(&("ordinary.txt", None)));
    }
    let ordinary = f
        .reader
        .diff(&GitDiffInput {
            paths: vec!["ordinary.txt".into()],
            ..serde_json::from_value(serde_json::json!({})).expect("defaults")
        })
        .expect("ordinary diff");
    assert!(ordinary.warnings.is_empty() && ordinary.files[0].comparison_note.is_none());
    assert!(!result.patch.contains('\u{1b}'));
    assert_eq!(std::fs::read(&index_path).expect("bytes"), bytes);
    assert_eq!(std::fs::metadata(&index_path).expect("index").modified().expect("time"), before);
    assert!(
        !String::from_utf8(f.git(&["for-each-ref", "--format=%(refname)"]))
            .expect("refs")
            .contains("refs/notes/textconv")
    );
}

#[test]
fn autocrlf_remains_native_but_lfs_is_compared_as_raw_content_without_filters() {
    let lfs = std::path::Path::new("C:/Program Files/Git/mingw64/bin/git-lfs.exe");
    assert!(lfs.is_file(), "the approved existing Git installation supplies LFS; no download");
    let extra = format!(
        "[core]\nautocrlf = true\n[filter \"lfs\"]\nclean = \\\"{}\\\" clean -- %f\nsmudge = \\\"{}\\\" smudge --skip -- %f\nrequired = true\n",
        lfs.display(),
        lfs.display()
    );
    let f = Fixture::new(&extra);
    f.file(".gitattributes", b"*.bin filter=lfs diff=lfs merge=lfs -text\n");
    f.file("line.txt", b"one\r\ntwo\r\n");
    f.file("large.bin", b"local temporary LFS content\0");
    f.commit("CRLF and LFS");
    assert!(
        String::from_utf8(f.git(&["show", "HEAD:large.bin"]))
            .expect("pointer")
            .starts_with("version https://git-lfs.github.com/spec/v1\n")
    );
    let status = f.reader.status(&GitStatusInput::default()).expect("raw CRLF and LFS");
    assert!(!status.entries.iter().any(|entry| entry.path == "line.txt"));
    // Git may accept the existing index stat cache without opening unchanged
    // content. If a change is reported, it must explain the disabled filter.
    if let Some(lfs_change) = status.entries.iter().find(|entry| entry.path == "large.bin") {
        assert_eq!(
            lfs_change.comparison_note.as_deref(),
            Some(super::super::attributes::RAW_COMPARISON)
        );
    }
    f.file("line.txt", b"one\r\nchanged\r\n");
    f.file("large.bin", b"changed temporary LFS content\0");
    let diff = f
        .reader
        .diff(&serde_json::from_value(serde_json::json!({})).expect("defaults"))
        .expect("LFS diff");
    assert!(diff.files.iter().any(|entry| entry.path == "line.txt" && entry.added == Some(1)));
    let lfs_file =
        diff.files.iter().find(|entry| entry.path == "large.bin").expect("binary raw diff");
    assert!(lfs_file.binary);
    assert_eq!(lfs_file.comparison_note.as_deref(), Some(super::super::attributes::RAW_COMPARISON));
    assert!(diff.patch.contains("Binary files"));
}
