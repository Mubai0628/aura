//! What a patch does to a file.
use crate::FsToolError;
use crate::mutation::patch::{apply_text, parse_patch, prepare_diff, validate_patch_target};
use aura_domain_common::WorkspacePath;

fn patched(base: &str, diff: &str) -> Result<String, FsToolError> {
    let prepared = prepare_diff(diff)?;
    let patch = parse_patch(&prepared.text)?;
    apply_text(base, &patch, prepared.unplaced)
}

fn message(result: Result<String, FsToolError>) -> String {
    match result {
        Ok(text) => panic!("applied: {text:?}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn a_hunk_with_no_line_numbers_is_placed_by_its_lines() {
    let base = "body.picking,\nbody.picking main { cursor: crosshair; }\n\n.toast {\n}\n";
    let diff = "@@\n body.picking,\n body.picking main { cursor: crosshair; }\n+body.picking #scene { pointer-events: none; }\n*** End Patch";
    assert_eq!(
        patched(base, diff).expect("placed"),
        "body.picking,\nbody.picking main { cursor: crosshair; }\nbody.picking #scene { pointer-events: none; }\n\n.toast {\n}\n"
    );
    let enveloped = "*** Begin Patch\n*** Update File: kimi-k3-1/styles.css\n@@ .toast\n .toast {\n+  color: red;\n }\n*** End Patch\n";
    assert_eq!(
        patched(base, enveloped).expect("placed"),
        "body.picking,\nbody.picking main { cursor: crosshair; }\n\n.toast {\n  color: red;\n}\n"
    );
}

#[test]
fn a_diff_that_changes_nothing_is_refused_not_reported_as_applied() {
    let base = "a\nb\n";
    for diff in ["--- a/n.txt\n+++ b/n.txt\n@@ -1,1 +1,1 @@\n a\n", "nothing here\n*** End Patch\n"]
    {
        let text = message(patched(base, diff));
        assert!(text.contains("changes nothing") || text.contains("no hunk"), "{text}");
    }
}

#[test]
fn a_header_naming_the_file_from_its_own_folder_is_the_same_file() {
    let path = WorkspacePath::parse("kimi-k3-1/styles.css").expect("path");
    let same =
        parse_patch("--- a/styles.css\n+++ b/styles.css\n@@ -1 +1 @@\n-a\n+b\n").expect("patch");
    assert!(validate_patch_target(&path, &same, None).is_ok());
    let other = parse_patch("--- a/app.js\n+++ b/app.js\n@@ -1 +1 @@\n-a\n+b\n").expect("patch");
    assert!(validate_patch_target(&path, &other, None).is_err());
    assert!(validate_patch_target(&path, &same, Some("other/styles.css")).is_err());
}

#[test]
fn a_hunk_whose_line_numbers_are_off_applies_where_its_lines_are() {
    let base = "one\ntwo\nthree\nfour\nfive\nsix\n";
    let diff = "--- a/n.txt\n+++ b/n.txt\n@@ -1,2 +1,2 @@\n four\n-five\n+FIVE\n";
    assert_eq!(patched(base, diff).expect("placed"), "one\ntwo\nthree\nfour\nFIVE\nsix\n");
}

#[test]
fn a_hunk_that_does_not_match_says_which_line_and_what_the_file_has() {
    let base = "  const wide = window.innerWidth >= 900;\n  pivot.position.set(wide ? 1.62 : 0, 0, 0);\n  camera.fov = 42;\n";
    let diff = "--- a/app.js\n+++ b/app.js\n@@ -1,3 +1,3 @@\n   const wide = window.innerWidth >= 900;\n-  core.position.set(wide ? 1.95 : 0, 0, 0);\n+  core.position.set(wide ? 1.55 : 0, 0, 0);\n   camera.fov = 42;\n";
    let text = message(patched(base, diff));
    assert!(text.contains("hunk #1"), "{text}");
    assert!(text.contains("line 2"), "{text}");
    assert!(text.contains("core.position.set") && text.contains("pivot.position.set"), "{text}");
}

#[test]
fn lines_that_appear_twice_need_more_context() {
    let base = "a\nx\nb\na\nx\nb\n";
    let text = message(patched(base, "@@\n a\n-x\n+y\n"));
    assert!(text.contains("2 times"), "{text}");
}

#[test]
fn trailing_spaces_and_a_missing_final_newline_do_not_stop_a_hunk() {
    assert_eq!(patched("a  \nb", "@@ -1,2 +1,3 @@\n a\n b\n+c\n").expect("placed"), "a  \nb\nc\n");
}
