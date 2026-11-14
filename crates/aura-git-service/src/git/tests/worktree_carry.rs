//! A new worktree takes the main checkout's uncommitted changes along, and the worktree's own
//! changes go back into a checkout with uncommitted changes as files, without a commit. Over real
//! Git, line endings converted on checkout included.
use super::super::*;
use super::support::Fixture;
use super::worktrees::{BRANCH, add, base, git_in, outside, reader_at, writer_at};

const TAKEN: &str = "Aura：建工作副本时主工作区没提交的改动";

/// A worktree of `f` that took the checkout's uncommitted changes along; its folder, the commit
/// its own work starts from, and the commit it was added at.
fn carried(f: &Fixture, folder: &tempfile::TempDir) -> (std::path::PathBuf, String, String) {
    let path = folder.path().join("a1");
    let start = f.reader.head_commit().expect("head");
    let files = f.reader.changed_from(&start).expect("changed files");
    add(f, &path, BRANCH);
    let base = writer_at(f, &path).take_along(f.root.path(), &files, &start, TAKEN).expect("taken");
    (path, base, start)
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("file")
}

#[test]
fn a_new_worktree_takes_the_uncommitted_changes_along_but_not_ignored_files() {
    let f = base();
    f.file("tracked", b"edited\n");
    std::fs::remove_file(f.root.path().join("other")).expect("deleted");
    std::fs::create_dir_all(f.root.path().join("mini-game")).expect("folder");
    f.file("mini-game/game.html", b"<canvas></canvas>\n");
    f.file("staged", b"staged\n");
    f.git(&["add", "staged"]);
    f.file("local.cfg", b"ignored\n");
    let before = f.reader.head_commit().expect("head");
    assert_eq!(
        f.reader.changed_from(&before).expect("changed"),
        ["mini-game/game.html", "other", "staged", "tracked"]
    );
    let folder = outside();
    let (path, base, start) = carried(&f, &folder);
    assert_ne!(base, start, "the changes are a commit of their own");
    assert_eq!(read(&path.join("tracked")), b"edited\n");
    assert!(!path.join("other").exists());
    assert_eq!(read(&path.join("mini-game/game.html")), b"<canvas></canvas>\n");
    assert_eq!(read(&path.join("staged")), b"staged\n");
    assert!(!path.join("local.cfg").exists(), "ignored files stay out");
    // The worktree's own work starts clean from that commit, made by Aura on its branch.
    let own = reader_at(&f, &path);
    assert!(own.changes_since(&base).expect("changes").is_empty());
    assert_eq!(own.head_commit().expect("head"), base);
    let log = git_in(&f, &path, &["log", "-1", "--format=%an|%P|%s"]);
    assert_eq!(String::from_utf8(log).expect("UTF-8").trim(), format!("Aura|{start}|{TAKEN}"));
    // Added again, the same worktree answers with the same commit.
    let again = writer_at(&f, &path).take_along(f.root.path(), &["tracked".into()], &start, TAKEN);
    assert_eq!(again.expect("again"), base);
    // The main checkout is as it was.
    assert_eq!(f.reader.head_commit().expect("head"), before);
    assert_eq!(read(&f.root.path().join("tracked")), b"edited\n");
    assert!(f.root.path().join("local.cfg").is_file());
}

#[test]
fn a_clean_checkout_gives_a_worktree_at_its_own_commit() {
    let f = base();
    let folder = outside();
    let (_, base, start) = carried(&f, &folder);
    assert_eq!(base, start);
}

#[test]
fn the_worktrees_own_changes_are_written_into_a_checkout_with_uncommitted_ones() {
    let f = base();
    f.file("tracked", b"the user's edit\n");
    f.file("game.html", b"v1\n");
    let head = f.reader.head_commit().expect("head");
    let folder = outside();
    let (path, base, _) = carried(&f, &folder);
    std::fs::write(path.join("game.html"), b"v2\n").expect("edit");
    std::fs::write(path.join("notes.md"), b"notes\n").expect("new file");
    writer_at(&f, &path).commit_all("A1 的改动").expect("commit");
    let outcome = writer_at(&f, f.root.path()).bring_in(BRANCH, &base, "合回 A1").expect("in");
    assert_eq!(
        outcome,
        MergeOutcome::Applied { files: vec!["game.html".into(), "notes.md".into()] }
    );
    assert_eq!(read(&f.root.path().join("game.html")), b"v2\n");
    assert_eq!(read(&f.root.path().join("notes.md")), b"notes\n");
    assert_eq!(read(&f.root.path().join("tracked")), b"the user's edit\n");
    // Nothing was committed: the branch is where it was and the files wait for the user.
    assert_eq!(f.reader.head_commit().expect("head"), head);
    assert_eq!(f.reader.uncommitted_paths().expect("paths"), ["tracked"]);
}

#[test]
fn edits_to_different_lines_of_one_file_are_both_kept() {
    let f = base();
    f.file("other", b"one\ntwo\nthree\nfour\n");
    let folder = outside();
    let (path, base, _) = carried(&f, &folder);
    std::fs::write(path.join("other"), b"one\ntwo\nthree\nFOUR\n").expect("agent's edit");
    writer_at(&f, &path).commit_all("A1").expect("commit");
    f.file("other", b"ONE\ntwo\nthree\nfour\n");
    let outcome = writer_at(&f, f.root.path()).bring_in(BRANCH, &base, "合回 A1").expect("in");
    assert_eq!(outcome, MergeOutcome::Applied { files: vec!["other".into()] });
    assert_eq!(read(&f.root.path().join("other")), b"ONE\ntwo\nthree\nFOUR\n");
}

#[test]
fn a_file_both_changed_where_they_clash_stops_it_and_nothing_is_written() {
    let f = base();
    f.file("tracked", b"the user's edit\n");
    let folder = outside();
    let (path, base, _) = carried(&f, &folder);
    std::fs::write(path.join("tracked"), b"the agent's edit\n").expect("edit");
    std::fs::write(path.join("fresh"), b"new\n").expect("new file");
    writer_at(&f, &path).commit_all("A1").expect("commit");
    f.file("tracked", b"the user's second edit\n");
    let outcome = writer_at(&f, f.root.path()).bring_in(BRANCH, &base, "合回 A1").expect("in");
    assert_eq!(outcome, MergeOutcome::Clashed { files: vec!["tracked".into()] });
    assert_eq!(read(&f.root.path().join("tracked")), b"the user's second edit\n");
    assert!(!f.root.path().join("fresh").exists(), "nothing is written when one file clashes");
}

#[test]
fn an_untracked_file_in_the_way_of_a_clean_merge_is_not_overwritten() {
    let f = base();
    let folder = outside();
    let (path, base, _) = carried(&f, &folder);
    std::fs::write(path.join("fresh"), b"the agent's\n").expect("new file");
    writer_at(&f, &path).commit_all("A1").expect("commit");
    f.file("fresh", b"the user's\n");
    let outcome = writer_at(&f, f.root.path()).bring_in(BRANCH, &base, "合回 A1").expect("in");
    assert_eq!(outcome, MergeOutcome::Clashed { files: vec!["fresh".into()] });
    assert_eq!(read(&f.root.path().join("fresh")), b"the user's\n");
}

#[test]
fn a_clean_checkout_with_the_start_in_its_history_still_gets_a_merge_commit() {
    let f = base();
    let folder = outside();
    let (path, base, _) = carried(&f, &folder);
    std::fs::write(path.join("tracked"), b"changed by the sub-agent\n").expect("edit");
    writer_at(&f, &path).commit_all("A1").expect("commit");
    let outcome = writer_at(&f, f.root.path()).bring_in(BRANCH, &base, "合回 A1").expect("in");
    let MergeOutcome::Merged { head } = outcome else { panic!("{outcome:?}") };
    assert_eq!(f.reader.head_commit().expect("head"), head);
    assert_eq!(read(&f.root.path().join("tracked")), b"changed by the sub-agent\n");
}

#[test]
fn a_checkout_with_converted_line_endings_merges_its_lines_without_a_clash() {
    let f = Fixture::new("[core]\n\tautocrlf = true\n");
    f.file(".gitignore", b"*.cfg\nprofile.json\n");
    f.file("notes.txt", b"a\r\nb\r\nc\r\nd\r\n");
    f.commit("base");
    // Taken along with the user's first line.
    f.file("notes.txt", b"A\r\nb\r\nc\r\nd\r\n");
    let folder = outside();
    let (path, base, _) = carried(&f, &folder);
    assert_eq!(read(&path.join("notes.txt")), b"A\r\nb\r\nc\r\nd\r\n");
    std::fs::write(path.join("notes.txt"), b"A\r\nb\r\nc\r\nD\r\n").expect("agent's edit");
    writer_at(&f, &path).commit_all("A1").expect("commit");
    // The user changes the second line meanwhile.
    f.file("notes.txt", b"A\r\nB\r\nc\r\nd\r\n");
    let outcome = writer_at(&f, f.root.path()).bring_in(BRANCH, &base, "合回 A1").expect("in");
    assert_eq!(outcome, MergeOutcome::Applied { files: vec!["notes.txt".into()] });
    assert_eq!(read(&f.root.path().join("notes.txt")), b"A\r\nB\r\nc\r\nD\r\n");
}

/// A checkout whose files Aura wrote with LF, in a repository that checks files out with CRLF:
/// the worktree's copy of the file has CRLF. Something uncommitted is taken along, so its work
/// comes back as files; the lines decide, and the checkout's file keeps its LF.
fn lf_checkout_with_crlf_worktree() -> (Fixture, tempfile::TempDir, std::path::PathBuf, String) {
    let f = Fixture::new("[core]\n\tautocrlf = true\n");
    f.file(".gitignore", b"*.cfg\nprofile.json\n");
    f.file("game.html", b"a\nb\nc\nd\n");
    f.commit("base");
    f.file("notes.md", b"the user's\n");
    let folder = outside();
    let (path, base, _) = carried(&f, &folder);
    assert_eq!(read(&path.join("game.html")), b"a\r\nb\r\nc\r\nd\r\n");
    std::fs::write(path.join("game.html"), b"a\r\nb\r\nc\r\nD\r\n").expect("agent's edit");
    writer_at(&f, &path).commit_all("A1").expect("commit");
    (f, folder, path, base)
}

#[test]
fn a_file_written_with_lf_takes_the_work_of_a_worktree_checked_out_with_crlf() {
    let (f, _folder, _path, base) = lf_checkout_with_crlf_worktree();
    let outcome = writer_at(&f, f.root.path()).bring_in(BRANCH, &base, "合回 A1").expect("in");
    assert_eq!(outcome, MergeOutcome::Applied { files: vec!["game.html".into()] });
    assert_eq!(read(&f.root.path().join("game.html")), b"a\nb\nc\nD\n");
}

#[test]
fn lf_edits_in_the_checkout_and_crlf_edits_in_the_worktree_are_both_kept() {
    let (f, _folder, _path, base) = lf_checkout_with_crlf_worktree();
    f.file("game.html", b"A\nb\nc\nd\n");
    let outcome = writer_at(&f, f.root.path()).bring_in(BRANCH, &base, "合回 A1").expect("in");
    assert_eq!(outcome, MergeOutcome::Applied { files: vec!["game.html".into()] });
    assert_eq!(read(&f.root.path().join("game.html")), b"A\nb\nc\nD\n");
}

#[test]
fn an_lf_edit_of_the_line_the_worktree_changed_still_clashes() {
    let (f, _folder, _path, base) = lf_checkout_with_crlf_worktree();
    f.file("game.html", b"a\nb\nc\nd!\n");
    let outcome = writer_at(&f, f.root.path()).bring_in(BRANCH, &base, "合回 A1").expect("in");
    assert_eq!(outcome, MergeOutcome::Clashed { files: vec!["game.html".into()] });
    assert_eq!(read(&f.root.path().join("game.html")), b"a\nb\nc\nd!\n");
}
