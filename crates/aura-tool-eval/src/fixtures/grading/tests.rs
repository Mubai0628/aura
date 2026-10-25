use super::*;

#[test]
fn restoring_the_rust_bug_preserves_inline_regression_tests() {
    let root = tempfile::tempdir().expect("isolated scorer files");
    let original = root.path().join("original.rs");
    let candidate = root.path().join("candidate.rs");
    std::fs::write(&original, "pub fn page() -> usize { 0 }").expect("original buggy function");
    std::fs::write(&candidate, "pub fn page() -> usize { 1 }\n#[cfg(test)] mod tests { #[test] fn regression() { assert_eq!(super::page(), 1); } }").expect("correct function with inline tests");
    restore_implementation(&original, &candidate, "Rust").expect("restore only the function");
    let result = std::fs::read_to_string(candidate).expect("restored scorer source");
    assert!(result.contains("regression"), "inline tests must still run against the original bug");
    assert!(result.contains('0'), "the original buggy function was restored");
}

#[test]
fn restoring_multiple_rust_functions_preserves_each_regression_test() {
    let root = tempfile::tempdir().expect("isolated multi-function scorer");
    let original = root.path().join("original.rs");
    let candidate = root.path().join("candidate.rs");
    std::fs::write(&original, "pub fn a() -> usize { 0 }\npub fn b() -> usize { 0 }")
        .expect("two buggy functions");
    std::fs::write(&candidate, "pub fn a() -> usize { 1 }\npub fn b() -> usize { 2 }\n#[cfg(test)] mod tests { #[test] fn both() { assert_eq!(super::a()+super::b(), 3); } }").expect("candidate with regression");
    restore_implementation(&original, &candidate, "Rust").expect("restore every original function");
    let result = std::fs::read_to_string(candidate).expect("restored text");
    assert!(result.contains("both"), "candidate tests must survive restoring original functions");
    assert_eq!(result.matches('0').count(), 2, "both original defects are restored");
}

#[test]
fn a_renamed_unfinished_or_deleted_rust_module_is_restored_whole_without_failing_scoring() {
    let root = tempfile::tempdir().expect("isolated scorer files");
    let original = root.path().join("original.rs");
    let candidate = root.path().join("candidate.rs");
    let source = "pub fn append() -> usize { 0 }";
    std::fs::write(&original, source).expect("original buggy module");
    for changed in ["pub fn append_bounded() -> usize { 1 }", "pub fn append() -> usize { 1 "] {
        std::fs::write(&candidate, changed).expect("renamed or unfinished candidate");
        assert!(
            restore_implementation(&original, &candidate, "Rust").expect("scoring continues"),
            "the whole-file restore is reported"
        );
        assert_eq!(std::fs::read_to_string(&candidate).expect("restored"), source);
    }
    std::fs::remove_file(&candidate).expect("the model deleted the module");
    assert!(restore_implementation(&original, &candidate, "Rust").expect("module restored"));
    assert_eq!(std::fs::read_to_string(&candidate).expect("restored"), source);
}

#[test]
fn a_deleted_module_or_unparsable_test_is_counted_without_failing_scoring() {
    let root = tempfile::tempdir().expect("isolated work comparison");
    let (original, candidate) = (root.path().join("original"), root.path().join("candidate"));
    for directory in [&original, &candidate] {
        std::fs::create_dir_all(directory.join("src")).expect("source directory");
        std::fs::create_dir_all(directory.join("tests")).expect("test directory");
    }
    std::fs::write(original.join("src/parser.rs"), "pub fn parse() {}").expect("parser");
    std::fs::write(original.join("src/limits.rs"), "pub fn append() {}").expect("limits");
    std::fs::write(candidate.join("src/parser.rs"), "pub fn parse() { let _fixed = 1; }")
        .expect("changed parser; limits.rs was deleted");
    std::fs::write(candidate.join("tests/regression.rs"), "#[test] fn unfinished( {")
        .expect("unparsable new test");
    let tests = [PathBuf::from("tests/regression.rs")];
    let work = super::requirements::model_work("R06", "Rust", &original, &candidate, &tests)
        .expect("scoring continues");
    assert_eq!(work.implementation_files_changed, 2);
    assert!(work.regression_edit, "the test run, not the parser, decides an unfinished test");
}

#[test]
fn a_rust_production_edit_does_not_count_as_a_regression_edit() {
    let root = tempfile::tempdir().expect("isolated test comparison");
    let original = root.path().join("original.rs");
    let candidate = root.path().join("candidate.rs");
    let tests = "#[cfg(test)] mod tests { #[test] fn unchanged() { assert_eq!(1, 1); } }";
    std::fs::write(&original, format!("pub fn run() -> usize {{ 0 }} {tests}"))
        .expect("old production body");
    std::fs::write(&candidate, format!("pub fn run() -> usize {{ 1 }} {tests}"))
        .expect("correct production body only");
    assert!(
        !super::super::rust_regression::has_test_edit(&original, &candidate)
            .expect("compare test items")
    );
    std::fs::write(&candidate, "pub fn run() -> usize { 1 } #[cfg(test)] mod tests { #[test] fn added() { assert_eq!(super::run(), 1); } }").expect("candidate regression");
    assert!(
        super::super::rust_regression::has_test_edit(&original, &candidate)
            .expect("new test detected")
    );
}
