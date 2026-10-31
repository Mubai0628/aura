//! The remaining §6 repository states, through the same Native mutation path.
use super::*;

#[test]
fn detached_commit_and_switch_receipts_keep_the_before_and_after_heads() {
    let f = base();
    f.git(&["checkout", "--detach", "HEAD"]);
    f.file("tracked", b"detached change\n");
    let mut w = writer(&f, Arc::new(AtomicBool::new(false)));
    let commit = w.commit(&selected()).expect("detached commit");
    assert!(commit.branch.is_none() && commit.status.detached);
    assert_eq!(commit.status.head.as_deref(), Some(commit.commit.commit.as_str()));
    let branch = w
        .branch(&GitBranchInput::Create { name: "from-detached".into(), start: None, switch: true })
        .expect("create from detached HEAD");
    assert!(branch.previous_branch.is_none());
    assert_eq!(branch.previous_head.as_deref(), Some(commit.commit.commit.as_str()));
    assert_eq!(branch.head, branch.previous_head);
    assert_eq!(branch.status.branch.as_deref(), Some("from-detached"));
}

#[test]
fn unresolved_rebase_refuses_commit_without_changing_the_sequencer() {
    let f = base();
    f.git(&["switch", "-c", "side"]);
    f.file("tracked", b"side\n");
    f.commit("side");
    f.git(&["switch", "main"]);
    f.file("tracked", b"main\n");
    f.commit("main");
    let out = f
        .reader
        .runner
        .run_native(
            f.root.path(),
            &["-c".into(), "core.editor=false".into(), "rebase".into(), "side".into()],
            &AtomicBool::new(false),
            Duration::from_secs(10),
            65536,
        )
        .expect("local conflicted rebase");
    assert_eq!(out.exit_code, Some(1));
    let before = f.git(&["rev-parse", "HEAD"]);
    let error = writer(&f, Arc::new(AtomicBool::new(false)))
        .commit(&GitCommitInput { paths: vec![], staged_only: true, ..selected() })
        .expect_err("unresolved rebase")
        .to_string();
    assert!(error.contains("git.operation_in_progress"), "{error}");
    assert_eq!(f.git(&["rev-parse", "HEAD"]), before);
    let status =
        f.reader.status(&aura_workspace_domain::GitStatusInput::default()).expect("status");
    assert_eq!(status.operation.as_deref(), Some("rebase"));
    assert_eq!(status.conflicts, ["tracked"]);
}

#[test]
fn selected_submodule_commits_its_gitlink_and_reports_the_read_limit() {
    let f = base();
    let child = f.root.path().join("child");
    std::fs::create_dir(&child).expect("child repository");
    let path = child.to_str().expect("fixture path");
    f.git(&["-C", path, "init"]);
    std::fs::write(child.join("tracked"), b"child one\n").expect("child file");
    f.git(&["-C", path, "add", "tracked"]);
    f.git(&["-C", path, "commit", "-m", "child one"]);
    f.file(".gitmodules", b"[submodule \"child\"]\npath = child\nurl = ./child\n");
    f.commit("record child");
    std::fs::write(child.join("tracked"), b"child two\n").expect("child file");
    f.git(&["-C", path, "add", "tracked"]);
    f.git(&["-C", path, "commit", "-m", "child two"]);
    let pointer = String::from_utf8(f.git(&["-C", path, "rev-parse", "HEAD"])).expect("gitlink");
    let out = writer(&f, Arc::new(AtomicBool::new(false)))
        .commit(&GitCommitInput { paths: vec!["child".into()], ..selected() })
        .expect("gitlink commit");
    let recorded = String::from_utf8(f.git(&["ls-tree", "HEAD", "child"])).expect("tree");
    assert!(recorded.contains(pointer.trim()), "{recorded}");
    assert!(!out.status.submodule_contents_checked);
    assert_eq!(out.status.total_entries, 0);
}
