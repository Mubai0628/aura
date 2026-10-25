use super::patch::{normalize_hunk_counts, parse_patch};

mod placement;

#[test]
fn both_hunks_recount_lines_without_changing_positions_or_context() {
    let diff = "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,99 +1,99 @@\n a\n-b\n+B\n+B2\n c\n@@ -5,90 +6,90 @@\n e\n-f\n+F\n";
    let normalized = normalize_hunk_counts(diff);
    assert!(normalized.contains("@@ -1,3 +1,4 @@"));
    assert!(normalized.contains("@@ -5,2 +6,2 @@"));
    let patch = parse_patch(&normalized).expect("recounted patch");
    assert_eq!(
        diffy::apply("a\nb\nc\nd\ne\nf\n", &patch).expect("exact context"),
        "a\nB\nB2\nc\nd\ne\nF\n"
    );
    assert!(
        diffy::apply("a\nb\nc\nd\nWRONG\nf\n", &patch).is_err(),
        "the second hunk must still match"
    );
}

#[test]
fn missing_final_newlines_and_zero_length_ranges_are_preserved() {
    let diff = "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,8 +1,9 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n";
    let normalized = normalize_hunk_counts(diff);
    let patch = parse_patch(&normalized).expect("newline markers remain valid");
    assert_eq!(diffy::apply("old", &patch).expect("exact old line"), "new");
    let normalized =
        normalize_hunk_counts("--- a/notes.txt\n+++ b/notes.txt\n@@ -0,0 +1,20 @@\n+new\n");
    let patch = parse_patch(&normalized).expect("empty old range");
    assert_eq!(diffy::apply("", &patch).expect("insertion"), "new\n");
}

#[test]
fn malformed_headers_and_line_prefixes_are_still_rejected() {
    for diff in [
        "--- a/notes.txt\n+++ b/notes.txt\n@@ -bad,2 +1,2 @@\n old\n",
        "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,5 +1,5 @@\n!bad prefix\n",
        // A hunk with no body is never recounted into an empty change.
        "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,3 +1,3 @@\n@@ -5 +5 @@\n-e\n+E\n",
    ] {
        let normalized = normalize_hunk_counts(diff);
        assert!(parse_patch(&normalized).is_err(), "{normalized}");
    }
}

/// An empty line of the file, written as a
/// blank line without its leading space, is a context line; blank lines after the last hunk carry
/// nothing.
#[test]
fn a_blank_context_line_without_its_space_is_still_context() {
    let diff = "--- a/game.js\n+++ b/game.js\n@@ -1,4 +1,4 @@\n function start() {\n\n-  run(1);\n+  run(2);\n }\n\n";
    let normalized = normalize_hunk_counts(diff);
    let patch = parse_patch(&normalized).expect("a blank context line is context");
    assert_eq!(
        diffy::apply("function start() {\n\n  run(1);\n}\n", &patch).expect("exact context"),
        "function start() {\n\n  run(2);\n}\n"
    );
    // Its counts are taken from the body as repaired, and CRLF diffs keep their line ending.
    let crlf = "--- a/game.js\r\n+++ b/game.js\r\n@@ -1,9 +1,9 @@\r\n a\r\n\r\n-b\r\n+B\r\n";
    let normalized = normalize_hunk_counts(crlf);
    assert!(normalized.contains("@@ -1,3 +1,3 @@\r\n a\r\n \r\n-b\r\n+B\r\n"), "{normalized:?}");
    // A blank line is context only where the file has one: a different line still fails.
    let normalized = normalize_hunk_counts(diff);
    let patch = parse_patch(&normalized).expect("parsed");
    assert!(diffy::apply("function start() {\n  x();\n  run(1);\n}\n", &patch).is_err());
}
