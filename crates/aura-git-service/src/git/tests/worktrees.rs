//! Worktrees a writing sub-agent works in: added on their own branch, merged back into the
//! main checkout's branch, compared with where they started, and removed with everything in them.
use super::super::*;
use super::support::Fixture;
use std::sync::atomic::AtomicBool;

pub(super) const BRANCH: &str = "aura/agent/0a1b2c3d";
const OTHER: &str = "aura/agent/4e5f6071";

pub(super) fn runner(f: &Fixture) -> GitRunner {
    let selected = std::env::var_os("AURA_GIT_TEST_BINARY")
        .unwrap_or_else(|| "C:/Program Files/Git/cmd/git.exe".into());
    GitRunner::isolated(Path::new(&selected), &f.root.path().join("profile.json"))
        .expect("isolated Native Git")
}

pub(super) fn reader_at(f: &Fixture, root: &Path) -> GitReader {
    GitReader::with_runner(root, runner(f), Arc::new(AtomicBool::new(false))).expect("reader")
}

pub(super) fn writer_at(f: &Fixture, root: &Path) -> GitWriter {
    GitWriter::new(reader_at(f, root), || {})
}

pub(super) fn base() -> Fixture {
    let f = Fixture::new("");
    f.file(".gitignore", b"target/\n*.cfg\nprofile.json\n");
    f.file("tracked", b"base\n");
    f.file("other", b"one\ntwo\n");
    f.commit("base");
    f
}

/// A folder outside the repository for the worktrees, removed with the fixture's.
pub(super) fn outside() -> tempfile::TempDir {
    tempfile::Builder::new().prefix("aura-git-test-wt-").tempdir().expect("worktree folder")
}

pub(super) fn add(f: &Fixture, path: &Path, branch: &str) -> WorktreeEntry {
    let start = f.reader.head_commit().expect("head");
    writer_at(f, f.root.path())
        .add_worktree(&WorktreeSpec { path: path.to_owned(), branch: branch.into(), start })
        .expect("worktree added")
}

pub(super) fn git_in(f: &Fixture, root: &Path, args: &[&str]) -> Vec<u8> {
    let out = runner(f)
        .run_native(
            root,
            &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
            &AtomicBool::new(false),
            Duration::from_secs(10),
            1024 * 1024,
        )
        .expect("git");
    assert_eq!(out.exit_code, Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

#[test]
fn a_worktree_is_added_on_its_own_branch_from_the_commit_given() {
    let f = base();
    let folder = outside();
    let path = folder.path().join("a1");
    let head = f.reader.head_commit().expect("head");
    let entry = add(&f, &path, BRANCH);
    assert_eq!(entry.branch.as_deref(), Some(BRANCH));
    assert_eq!(entry.head.as_deref(), Some(head.as_str()));
    assert_eq!(std::fs::read(path.join("tracked")).expect("checked out"), b"base\n");
    let listed = f.reader.worktrees().expect("list");
    assert!(
        listed
            .iter()
            .any(|item| item.branch.as_deref() == Some(BRANCH) && same_folder(&item.path, &path)),
        "{listed:?}"
    );
    // The main checkout stays on its own branch.
    assert_eq!(f.git(&["symbolic-ref", "--short", "HEAD"]), b"main\n");
}

#[test]
fn adding_again_for_the_same_path_and_branch_answers_with_that_worktree() {
    let f = base();
    let folder = outside();
    let path = folder.path().join("a1");
    let first = add(&f, &path, BRANCH);
    let again = add(&f, &path, BRANCH);
    assert_eq!(first.branch, again.branch);
    assert!(same_folder(&first.path, &again.path));
    assert_eq!(f.reader.worktrees().expect("list").len(), 2);
}

#[test]
fn a_worktree_inside_the_repository_or_on_another_branch_family_is_refused() {
    let f = base();
    let start = f.reader.head_commit().expect("head");
    let inside = writer_at(&f, f.root.path())
        .add_worktree(&WorktreeSpec {
            path: f.root.path().join("nested"),
            branch: BRANCH.into(),
            start: start.clone(),
        })
        .expect_err("inside the repository")
        .to_string();
    assert!(inside.contains("git.worktree_inside"), "{inside}");
    let folder = outside();
    let branch = writer_at(&f, f.root.path())
        .add_worktree(&WorktreeSpec {
            path: folder.path().join("a1"),
            branch: "main".into(),
            start,
        })
        .expect_err("not an agent branch")
        .to_string();
    assert!(branch.contains("git.worktree_branch"), "{branch}");
}

#[test]
fn merging_brings_the_branch_into_the_main_checkouts_branch() {
    let f = base();
    let folder = outside();
    let path = folder.path().join("a1");
    add(&f, &path, BRANCH);
    std::fs::write(path.join("tracked"), b"changed by the sub-agent\n").expect("edit");
    std::fs::write(path.join("added"), b"new\n").expect("new file");
    let committed = writer_at(&f, &path).commit_all("A1 的改动").expect("commit");
    assert!(committed.is_some());
    // Nothing left to commit: no second commit.
    assert_eq!(writer_at(&f, &path).commit_all("again").expect("clean"), None);
    let outcome = writer_at(&f, f.root.path()).merge(BRANCH, "合回 A1").expect("merge");
    let MergeOutcome::Merged { head } = outcome else { panic!("{outcome:?}") };
    assert_eq!(f.reader.head_commit().expect("head"), head);
    assert_eq!(
        std::fs::read(f.root.path().join("tracked")).expect("merged"),
        b"changed by the sub-agent\n"
    );
    assert!(f.root.path().join("added").is_file());
    assert_eq!(f.git(&["symbolic-ref", "--short", "HEAD"]), b"main\n");
}

#[test]
fn a_conflicting_merge_names_the_files_and_leaves_the_main_checkout_as_it_was() {
    let f = base();
    let folder = outside();
    let (first, second) = (folder.path().join("a1"), folder.path().join("a2"));
    add(&f, &first, BRANCH);
    add(&f, &second, OTHER);
    // Both change the same file, each in its own worktree, and neither sees the other.
    std::fs::write(first.join("tracked"), b"first\n").expect("edit");
    std::fs::write(second.join("tracked"), b"second\n").expect("edit");
    assert_eq!(std::fs::read(first.join("tracked")).expect("own"), b"first\n");
    assert_eq!(std::fs::read(second.join("tracked")).expect("own"), b"second\n");
    writer_at(&f, &first).commit_all("A1").expect("commit");
    writer_at(&f, &second).commit_all("A2").expect("commit");
    let merged = writer_at(&f, f.root.path()).merge(BRANCH, "合回 A1").expect("first merge");
    assert!(matches!(merged, MergeOutcome::Merged { .. }), "{merged:?}");
    let before = f.reader.head_commit().expect("head");
    let outcome = writer_at(&f, f.root.path()).merge(OTHER, "合回 A2").expect("reported");
    assert_eq!(outcome, MergeOutcome::Conflicted { files: vec!["tracked".into()] });
    assert_eq!(f.reader.head_commit().expect("head"), before);
    assert!(!f.root.path().join(".git/MERGE_HEAD").exists());
    assert_eq!(f.git(&["status", "--porcelain", "--untracked-files=no"]), b"");
    assert_eq!(std::fs::read(f.root.path().join("tracked")).expect("kept"), b"first\n");
}

#[test]
fn merging_stops_first_when_the_main_checkout_has_uncommitted_changes() {
    let f = base();
    let folder = outside();
    let path = folder.path().join("a1");
    add(&f, &path, BRANCH);
    std::fs::write(path.join("other"), b"one\ntwo\nthree\n").expect("edit");
    writer_at(&f, &path).commit_all("A1").expect("commit");
    f.file("tracked", b"the user's own edit\n");
    let before = f.reader.head_commit().expect("head");
    let error = writer_at(&f, f.root.path())
        .merge(BRANCH, "合回 A1")
        .expect_err("uncommitted changes")
        .to_string();
    assert!(error.contains("git.main_uncommitted") && error.contains("tracked"), "{error}");
    assert_eq!(f.reader.head_commit().expect("head"), before);
    assert_eq!(
        std::fs::read(f.root.path().join("tracked")).expect("kept"),
        b"the user's own edit\n"
    );
    assert_eq!(f.reader.uncommitted_paths().expect("paths"), vec!["tracked".to_owned()]);
}

#[test]
fn changes_since_the_start_list_committed_uncommitted_and_new_files_but_not_ignored_ones() {
    let f = base();
    let folder = outside();
    let path = folder.path().join("a1");
    let start = add(&f, &path, BRANCH).head.expect("head");
    std::fs::write(path.join("tracked"), b"committed change\n").expect("edit");
    writer_at(&f, &path).commit_all("A1").expect("commit");
    std::fs::write(path.join("other"), b"one\n").expect("uncommitted edit");
    std::fs::write(path.join("fresh"), b"a\nb\n").expect("new file");
    std::fs::create_dir_all(path.join("target/debug")).expect("build output");
    std::fs::write(path.join("target/debug/out.bin"), b"ignored").expect("ignored");
    let changes = reader_at(&f, &path).changes_since(&start).expect("changes");
    assert_eq!(
        changes,
        vec![
            FileChange {
                path: "fresh".into(),
                kind: ChangeKind::Added,
                added: Some(2),
                removed: Some(0)
            },
            FileChange {
                path: "other".into(),
                kind: ChangeKind::Modified,
                added: Some(0),
                removed: Some(1)
            },
            FileChange {
                path: "tracked".into(),
                kind: ChangeKind::Modified,
                added: Some(1),
                removed: Some(1)
            },
        ]
    );
}

#[test]
fn a_checkout_is_estimated_from_the_sizes_of_the_tracked_files() {
    let f = base();
    let head = f.reader.head_commit().expect("head");
    // .gitignore (27 bytes), tracked (5) and other (8).
    assert_eq!(f.reader.checkout_size(&head).expect("size"), 27 + 5 + 8);
}

#[test]
fn removing_a_worktree_deletes_its_folder_with_build_outputs_and_its_branch() {
    let f = base();
    let folder = outside();
    let path = folder.path().join("a1");
    add(&f, &path, BRANCH);
    std::fs::write(path.join("tracked"), b"never merged\n").expect("edit");
    std::fs::create_dir_all(path.join("target/debug/deps")).expect("outputs");
    std::fs::write(path.join("target/debug/deps/lib.rlib"), vec![0_u8; 4096]).expect("output");
    std::fs::write(path.join("untracked"), b"left over\n").expect("untracked");
    writer_at(&f, f.root.path()).remove_worktree(&path, BRANCH).expect("removed");
    assert!(!path.exists(), "the folder and everything in it is gone");
    assert!(
        f.reader
            .worktrees()
            .expect("list")
            .iter()
            .all(|item| item.branch.as_deref() != Some(BRANCH))
    );
    let branches =
        String::from_utf8(git_in(&f, f.root.path(), &["branch", "--list"])).expect("utf8");
    assert!(!branches.contains(BRANCH), "{branches}");
    // Gone already: removing again changes nothing and is not an error.
    writer_at(&f, f.root.path()).remove_worktree(&path, BRANCH).expect("again");
}

#[test]
fn removing_refuses_a_branch_aura_did_not_make() {
    let f = base();
    let folder = outside();
    let path = folder.path().join("a1");
    add(&f, &path, BRANCH);
    let error = writer_at(&f, f.root.path())
        .remove_worktree(&path, "main")
        .expect_err("not an agent branch")
        .to_string();
    assert!(error.contains("git.worktree_branch"), "{error}");
    assert!(path.join("tracked").is_file());
}

fn same_folder(left: &Path, right: &Path) -> bool {
    let normal = |path: &Path| {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_owned())
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase()
    };
    normal(left) == normal(right)
}
