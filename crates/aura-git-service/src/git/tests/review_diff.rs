//! The review page compares the working tree with HEAD (the uncommitted
//! changes) or with where HEAD left a branch (all changes of this work), and can ignore
//! whitespace. Untracked files are listed by status and read one by one, as before.
use super::support::Fixture;
use aura_workspace_domain::{GitDiffResultV1, GitReadInputV1, GitReadResultV1};
use serde_json::{Value, json};

fn review(f: &Fixture, input: Value) -> anyhow::Result<GitDiffResultV1> {
    let query: GitReadInputV1 =
        serde_json::from_value(json!({"kind":"review_diff","input":input}))?;
    match f.reader.read(&query)? {
        GitReadResultV1::Diff(diff) => Ok(diff),
        _ => anyhow::bail!("not a diff"),
    }
}

fn paths(diff: &GitDiffResultV1) -> Vec<&str> {
    let mut paths: Vec<&str> = diff.files.iter().map(|file| file.path.as_str()).collect();
    paths.sort_unstable();
    paths
}

#[test]
fn uncommitted_changes_are_the_working_tree_against_head_staged_or_not() {
    let f = Fixture::new("");
    f.file("staged.txt", b"one\n");
    f.file("unstaged.txt", b"one\n");
    f.commit("base");
    f.file("staged.txt", b"one\ntwo\n");
    f.git(&["add", "staged.txt"]);
    f.file("unstaged.txt", b"uno\n");
    let diff = review(&f, json!({})).expect("uncommitted changes");
    assert_eq!(paths(&diff), ["staged.txt", "unstaged.txt"]);
    let unstaged = diff.files.iter().find(|file| file.path == "unstaged.txt").expect("listed");
    assert_eq!((unstaged.added, unstaged.deleted), (Some(1), Some(1)));
    assert!(diff.patch.contains("+two") && diff.patch.contains("+uno"));
}

#[test]
fn all_changes_against_a_branch_start_where_head_left_it() {
    let f = Fixture::new("");
    f.file("shared.txt", b"base\n");
    f.commit("base");
    f.git(&["switch", "-c", "feature"]);
    f.file("feature.txt", b"feature work\n");
    f.commit("feature work");
    f.git(&["switch", "main"]);
    f.file("main-later.txt", b"landed on main afterwards\n");
    f.commit("main moves on");
    f.git(&["switch", "feature"]);
    f.file("shared.txt", b"base\nnot committed yet\n");
    let diff = review(&f, json!({"base":"main"})).expect("all changes against main");
    // The branch's own commit and the uncommitted edit; not what main gained after the split.
    assert_eq!(paths(&diff), ["feature.txt", "shared.txt"]);
    assert!(!diff.patch.contains("main-later.txt"));
}

#[test]
fn whitespace_only_edits_drop_out_when_whitespace_is_ignored() {
    let f = Fixture::new("");
    f.file("spaced.txt", b"let x = 1;\n");
    f.file("real.txt", b"a\n");
    f.commit("base");
    f.file("spaced.txt", b"let  x  =  1;  \n");
    f.file("real.txt", b"b\n");
    assert_eq!(paths(&review(&f, json!({})).expect("all")), ["real.txt", "spaced.txt"]);
    let ignoring = review(&f, json!({"ignore_whitespace": true})).expect("ignoring whitespace");
    assert_eq!(paths(&ignoring), ["real.txt"]);
    assert!(!ignoring.patch.contains("spaced.txt"));
}

#[test]
fn a_repository_without_a_commit_compares_with_the_empty_tree() {
    let f = Fixture::new("");
    f.file("first.txt", b"hello\n");
    f.git(&["add", "first.txt"]);
    let diff = review(&f, json!({})).expect("no HEAD yet");
    assert_eq!(paths(&diff), ["first.txt"]);
    assert_eq!(diff.files[0].added, Some(1));
}

#[test]
fn a_base_that_is_not_a_revision_of_this_repository_is_refused() {
    let f = Fixture::new("");
    f.file("a.txt", b"a\n");
    f.commit("base");
    for base in ["no-such-branch", "--output=C:/x", ""] {
        assert!(review(&f, json!({ "base": base })).is_err(), "{base:?} must be refused");
    }
}
