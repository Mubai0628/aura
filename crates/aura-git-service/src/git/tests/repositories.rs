use super::{super::*, support::Fixture};
use aura_workspace_domain::*;

#[test]
fn linked_worktree_separate_git_dir_and_alternates_use_native_repository_identity() {
    let fixture = Fixture::new("");
    fixture.file("tracked", b"base\n");
    let oid = fixture.commit("base");
    let linked = fixture.root.path().join("linked worktree");
    fixture.git(&["worktree", "add", "-b", "linked", linked.to_str().expect("path")]);
    let reader = GitReader::with_runner(
        &linked,
        GitRunner::isolated(
            Path::new(&fixture.reader.runner.image_binding().path),
            &fixture.root.path().join("profile.json"),
        )
        .expect("isolated linked Git"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("linked reader");
    assert_eq!(
        reader.repository_identity().expect("linked identity").identity,
        fixture.reader.repository_identity().expect("main identity").identity
    );
    assert_eq!(
        reader.status(&GitStatusInput::default()).expect("linked status").branch.as_deref(),
        Some("linked")
    );
    let separate = fixture.root.path().join("separate");
    std::fs::create_dir(&separate).expect("separate directory");
    let git_dir = fixture.root.path().join("external-git-dir");
    fixture.git(&[
        "init",
        "--separate-git-dir",
        git_dir.to_str().expect("path"),
        separate.to_str().expect("path"),
    ]);
    let standalone = GitReader::with_runner(
        &separate,
        GitRunner::isolated(
            Path::new(&fixture.reader.runner.image_binding().path),
            &fixture.root.path().join("profile.json"),
        )
        .expect("isolated separate Git"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("separate reader");
    assert_ne!(
        reader.repository_identity().expect("linked").identity,
        standalone.repository_identity().expect("separate").identity
    );
    // The alternate object database is outside this worktree. No network or copy is involved.
    std::fs::write(
        git_dir.join("objects/info/alternates"),
        format!(
            "{}\n",
            fixture.root.path().join(".git/objects").display().to_string().replace('\\', "/")
        ),
    )
    .expect("alternates");
    let log = standalone
        .log(&GitLogInput {
            commit: Some(oid),
            ..serde_json::from_value(serde_json::json!({})).expect("defaults")
        })
        .expect("alternate object log");
    assert_eq!(log.commits[0].subject, "base");
}

#[test]
fn real_merge_conflicts_and_rebase_operation_are_reported() {
    let fixture = Fixture::new("");
    fixture.file("conflict", b"base\n");
    fixture.commit("base");
    fixture.git(&["switch", "-c", "topic"]);
    fixture.file("conflict", b"topic\n");
    fixture.commit("topic");
    fixture.git(&["switch", "main"]);
    fixture.file("conflict", b"main\n");
    fixture.commit("main");
    let out = fixture
        .reader
        .runner
        .run_native(
            fixture.root.path(),
            &["merge".into(), "topic".into(), "--no-edit".into()],
            &AtomicBool::new(false),
            Duration::from_secs(5),
            65536,
        )
        .expect("local conflict");
    assert_eq!(out.exit_code, Some(1));
    let status = fixture.reader.status(&GitStatusInput::default()).expect("conflict status");
    assert_eq!(status.operation.as_deref(), Some("merge"));
    assert_eq!(status.conflicts, ["conflict"]);
    assert!(status.entries[0].conflict);
    fixture.git(&["merge", "--abort"]);
    let out = fixture
        .reader
        .runner
        .run_native(
            fixture.root.path(),
            &["-c".into(), "core.editor=false".into(), "rebase".into(), "topic".into()],
            &AtomicBool::new(false),
            Duration::from_secs(5),
            65536,
        )
        .expect("local rebase conflict");
    assert_eq!(out.exit_code, Some(1));
    let status = fixture.reader.status(&GitStatusInput::default()).expect("rebase status");
    assert_eq!(status.operation.as_deref(), Some("rebase"));
    assert_eq!(status.conflicts, ["conflict"]);
}

#[test]
fn submodules_report_pointer_changes_without_running_their_programs() {
    let fixture = Fixture::new("");
    let child = fixture.root.path().join("child");
    std::fs::create_dir(&child).expect("child");
    fixture.git(&["-C", child.to_str().expect("path"), "init"]);
    std::fs::write(child.join("tracked"), b"one\n").expect("tracked");
    fixture.git(&["-C", child.to_str().expect("path"), "add", "tracked"]);
    fixture.git(&["-C", child.to_str().expect("path"), "commit", "-m", "child one"]);
    fixture.file(".gitmodules", b"[submodule \"child\"]\npath = child\nurl = ./child\n");
    fixture.commit("parent pointer");
    std::fs::write(child.join("tracked"), b"dirty\n").expect("dirty child");
    let canary = fixture.root.path().join("child-program-ran");
    fixture.git(&[
        "-C",
        child.to_str().expect("path"),
        "config",
        "core.fsmonitor",
        &format!("echo forbidden > '{}'", canary.display().to_string().replace('\\', "/")),
    ]);
    let status = fixture.reader.status(&GitStatusInput::default()).expect("pointer only");
    assert!(!status.entries.iter().any(|entry| entry.path == "child"));
    assert!(!status.submodule_contents_checked && !canary.exists());
    fixture.git(&["-C", child.to_str().expect("path"), "add", "tracked"]);
    fixture.git(&["-C", child.to_str().expect("path"), "commit", "-m", "child two"]);
    let status = fixture.reader.status(&GitStatusInput::default()).expect("changed pointer");
    assert!(status.entries.iter().any(|entry| entry.path == "child" && entry.submodule));
    assert!(!canary.exists());
}
