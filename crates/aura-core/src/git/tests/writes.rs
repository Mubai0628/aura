use super::super::*;
use super::support::Fixture;
use aura_workspace_domain::{GitBranchInput, GitCommitInput};
use std::sync::atomic::Ordering;
mod edge_cases;
mod repository_cases;

fn writer(f: &Fixture, cancel: Arc<AtomicBool>) -> GitWriter {
    let selected = std::env::var_os("AURA_GIT_TEST_BINARY")
        .unwrap_or_else(|| "C:/Program Files/Git/cmd/git.exe".into());
    let runner = GitRunner::isolated(Path::new(&selected), &f.root.path().join("profile.json"))
        .expect("isolated Native Git");
    GitWriter::new(GitReader::with_runner(f.root.path(), runner, cancel).expect("metadata"), || {})
}
fn selected() -> GitCommitInput {
    GitCommitInput {
        message: "fixture commit".into(),
        paths: vec!["tracked".into()],
        staged_only: false,
    }
}
fn base() -> Fixture {
    let f = Fixture::new("");
    f.file("tracked", b"base\n");
    f.commit("base");
    f
}

#[test]
fn hook_rejection_is_explained_without_bypassing_it() {
    let f = base();
    let before = f.git(&["rev-parse", "HEAD"]);
    f.file("tracked", b"changed\n");
    f.file(".git/hooks/pre-commit", b"#!/bin/sh\nprintf ran >hook.marker\necho 'pre-commit hook rejected fixture' >&2\nexit 1\n");
    let mut w = writer(&f, Arc::new(AtomicBool::new(false)));
    let error = w.commit(&selected()).expect_err("hook refuses").to_string();
    assert!(
        error.contains("git.hook_rejected") && error.contains("pre-commit hook rejected fixture"),
        "{error}"
    );
    assert_eq!(std::fs::read(f.root.path().join("hook.marker")).expect("hook child ran"), b"ran");
    assert_eq!(f.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(w.side_effect(false), aura_execution_contract::SideEffectStateV1::Unknown);
}

#[test]
fn existing_index_lock_is_preserved_and_never_deleted() {
    let f = base();
    f.file("tracked", b"changed\n");
    f.file(".git/index.lock", b"temporary test lock");
    let error = writer(&f, Arc::new(AtomicBool::new(false)))
        .commit(&selected())
        .expect_err("lock")
        .to_string();
    assert!(error.contains("index.lock") && error.contains("不会删除锁文件"), "{error}");
    assert_eq!(
        std::fs::read(f.root.path().join(".git/index.lock")).expect("untouched lock"),
        b"temporary test lock"
    );
}

#[test]
fn unresolved_merge_refuses_commit_and_retains_merge_state() {
    let f = base();
    f.git(&["branch", "side"]);
    f.file("tracked", b"main change\n");
    f.commit("main change");
    f.git(&["switch", "side"]);
    f.file("tracked", b"side change\n");
    f.commit("side change");
    f.git(&["switch", "main"]);
    let merge = f
        .reader
        .runner
        .run_native(
            f.root.path(),
            &["merge".into(), "side".into()],
            &AtomicBool::new(false),
            Duration::from_secs(10),
            65536,
        )
        .expect("local merge fixture");
    assert_eq!(merge.exit_code, Some(1));
    let before = f.git(&["rev-parse", "HEAD"]);
    let input = GitCommitInput { paths: vec![], staged_only: true, ..selected() };
    let error = writer(&f, Arc::new(AtomicBool::new(false)))
        .commit(&input)
        .expect_err("conflicts")
        .to_string();
    assert!(error.contains("git.operation_in_progress"), "{error}");
    assert!(f.root.path().join(".git/MERGE_HEAD").is_file());
    assert_eq!(f.git(&["rev-parse", "HEAD"]), before);
}

#[test]
fn switching_never_discards_conflicting_local_changes() {
    let f = base();
    f.git(&["switch", "-c", "side"]);
    f.file("tracked", b"side\n");
    f.commit("side");
    f.git(&["switch", "main"]);
    f.file("tracked", b"keep local\n");
    let error = writer(&f, Arc::new(AtomicBool::new(false)))
        .branch(&GitBranchInput::Switch { name: "side".into() })
        .expect_err("local changes")
        .to_string();
    assert!(error.contains("git.local_changes"), "{error}");
    assert_eq!(std::fs::read(f.root.path().join("tracked")).expect("worktree"), b"keep local\n");
    assert_eq!(f.git(&["symbolic-ref", "--short", "HEAD"]), b"main\n");
}

#[test]
fn native_commit_keeps_sh_hooks_crlf_and_real_lfs_clean_filter() {
    let f = base();
    let lfs = "C:/Program Files/Git/mingw64/bin/git-lfs.exe";
    assert!(Path::new(lfs).is_file(), "existing local Git LFS required; never downloaded");
    f.git(&["config", "core.autocrlf", "true"]);
    f.git(&["config", "filter.lfs.clean", &format!("\"{lfs}\" clean -- %f")]);
    f.git(&["config", "filter.lfs.smudge", &format!("\"{lfs}\" smudge -- %f")]);
    f.git(&["config", "filter.lfs.process", &format!("\"{lfs}\" filter-process")]);
    f.git(&["config", "filter.lfs.required", "true"]);
    f.file(".gitattributes", b"asset.bin filter=lfs diff=lfs -text\n");
    f.file("asset.bin", b"temporary binary fixture content\0\xff");
    f.file("tracked", b"changed with CRLF\r\n");
    f.file(".git/hooks/pre-commit", b"#!/bin/sh\nprintf ran >hook.marker\n");
    let input = GitCommitInput {
        message: "LFS and hook".into(),
        paths: vec!["tracked".into(), ".gitattributes".into(), "asset.bin".into()],
        staged_only: false,
    };
    let result =
        writer(&f, Arc::new(AtomicBool::new(false))).commit(&input).expect("native programs run");
    assert_eq!(result.commit.subject, "LFS and hook");
    assert!(
        String::from_utf8(f.git(&["show", "HEAD:asset.bin"]))
            .expect("pointer")
            .starts_with("version https://git-lfs.github.com/spec/v1\n")
    );
    assert_eq!(f.git(&["show", "HEAD:tracked"]), b"changed with CRLF\n");
    assert!(f.root.path().join("hook.marker").is_file());
}

#[test]
fn cancelling_a_running_hook_is_unknown_and_stops_its_descendants() {
    let f = base();
    f.file("tracked", b"changed\n");
    f.file(
        ".git/hooks/pre-commit",
        b"#!/bin/sh\nprintf started >hook.started\nsleep 30\nprintf finished >hook.finished\n",
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    let marker = f.root.path().join("hook.started");
    let trigger = std::thread::spawn(move || {
        for _ in 0..500 {
            if marker.exists() {
                signal.store(true, Ordering::Release);
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        signal.store(true, Ordering::Release);
        false
    });
    let mut w = writer(&f, cancel);
    let began = std::time::Instant::now();
    let error = w.commit(&selected()).expect_err("cancel running hook");
    assert!(trigger.join().expect("trigger"), "hook reached its real child start");
    assert!(error.is::<GitWriteInterrupted>(), "{error}");
    assert!(error.to_string().contains("禁止自动重放"));
    assert!(error.to_string().contains("中断后 HEAD 未移动"), "{error}");
    assert_eq!(w.side_effect(false), aura_execution_contract::SideEffectStateV1::Unknown);
    assert!(began.elapsed() < Duration::from_secs(8));
    assert!(!f.root.path().join("hook.finished").exists());
}

#[test]
fn local_git_passes_soft_offline_environment_to_hooks_and_forbids_git_transports() {
    let f = base();
    f.file("tracked", b"local change\n");
    f.file(
        ".git/hooks/pre-commit",
        br#"#!/bin/sh
printf '%s\n' "$HTTP_PROXY" "$ALL_PROXY" "$NO_PROXY" >offline.marker
if git ls-remote . >offline.remote 2>offline.error; then
  echo 'fixture: local Git transport unexpectedly allowed' >&2
  exit 1
fi
"#,
    );
    writer(&f, Arc::new(AtomicBool::new(false)))
        .commit(&selected())
        .expect("hook runs with transport disabled");
    let marker = std::fs::read_to_string(f.root.path().join("offline.marker")).expect("hook env");
    let lines = marker.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3, "{marker}");
    assert!(lines[0].starts_with("http://127.0.0.1:"), "{marker}");
    assert_eq!(lines[0], lines[1]);
    assert!(lines[2].is_empty());
    let diagnostic =
        std::fs::read_to_string(f.root.path().join("offline.error")).expect("Git refusal");
    assert!(diagnostic.contains("transport 'file' not allowed"), "{diagnostic}");
}

#[test]
fn invalid_selection_and_cancel_before_start_have_no_effect() {
    let f = base();
    for input in [
        GitCommitInput { paths: vec![], staged_only: false, ..selected() },
        GitCommitInput { staged_only: true, ..selected() },
        GitCommitInput { message: " \n".into(), ..selected() },
        GitCommitInput { paths: vec!["../outside".into()], ..selected() },
    ] {
        let mut w = writer(&f, Arc::new(AtomicBool::new(false)));
        assert!(w.commit(&input).is_err());
        assert_eq!(w.side_effect(false), aura_execution_contract::SideEffectStateV1::NoEffect);
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut w = writer(&f, cancelled.clone());
    cancelled.store(true, Ordering::Release);
    assert!(w.commit(&selected()).is_err());
    assert_eq!(w.side_effect(false), aura_execution_contract::SideEffectStateV1::NoEffect);
}
