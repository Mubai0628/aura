use aura_tool_eval::fixtures::{catalog, prepare, score, task};

#[test]
fn fixtures_are_copied_and_graders_only_appear_after_execution() {
    let root = tempfile::tempdir().expect("isolated evaluation root");
    let fixture =
        prepare(root.path(), task("P01").expect("implemented Python task")).expect("copy task");
    assert!(!fixture.grading.exists(), "hidden tests are absent during execution");
    let original =
        std::fs::read_to_string(fixture.workspace.join("search.py")).expect("buggy fixture");
    let failed = score(&fixture).expect("objective baseline scorer");
    assert!(!failed.passed, "unchanged bug must fail hidden behavior tests");
    assert_eq!(
        std::fs::read_to_string(fixture.workspace.join("search.py")).expect("candidate untouched"),
        original
    );
    assert_eq!(catalog().len(), 18);
    assert_eq!(catalog().iter().filter(|t| t.implemented).count(), 6);
}
#[test]
fn changed_python_candidate_passes_hidden_behavior_tests() {
    let root = tempfile::tempdir().expect("isolated evaluation root");
    let fixture = prepare(root.path(), task("P01").expect("task")).expect("copy task");
    std::fs::write(fixture.workspace.join("search.py"),"def find_positions(text, query):\n    if not query:\n        return []\n    positions, start = [], 0\n    while (offset := text.find(query, start)) != -1:\n        positions.append(offset)\n        start = offset + len(query)\n    return positions\n").expect("correct candidate");
    std::fs::write(fixture.workspace.join("test_search.py"), "import unittest\nfrom search import find_positions\nclass Regression(unittest.TestCase):\n    def test_unicode(self): self.assertEqual(find_positions('甲a甲','甲'), [0,2])\n").expect("model-added public regression test");
    assert!(score(&fixture).expect("hidden score").passed);
}
#[test]
fn s09_a_correct_change_without_a_regression_test_is_not_task_completion() {
    let root = tempfile::tempdir().expect("isolated evaluation");
    let fixture = prepare(root.path(), task("P01").expect("Python task")).expect("copy fixture");
    std::fs::write(fixture.workspace.join("search.py"),"def find_positions(text, query):\n    if not query:\n        return []\n    positions, start = [], 0\n    while (offset := text.find(query, start)) != -1:\n        positions.append(offset)\n        start = offset + len(query)\n    return positions\n").expect("correct implementation without the requested test");
    assert!(
        !score(&fixture).expect("objective task score").passed,
        "the task explicitly requires adding a regression test"
    );
}
#[test]
fn aura_worktree_and_unimplemented_tasks_are_refused() {
    let aura = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(prepare(&aura, task("R01").expect("task")).is_err());
    let excluded = aura
        .canonicalize()
        .expect("repo")
        .parent()
        .expect("repo parent")
        .join("AuraTest/s09-must-not-create");
    assert!(
        prepare(&excluded, task("R01").expect("task")).is_err(),
        "AuraTest is refused before creating directories"
    );
    let root = tempfile::tempdir().expect("root");
    assert!(prepare(root.path(), task("R02").expect("planned task")).is_err());
}
#[test]
fn a_trivial_added_test_cannot_replace_a_regression_test() {
    let root = tempfile::tempdir().expect("isolated evaluation");
    let fixture = prepare(root.path(), task("P01").expect("Python task")).expect("copy fixture");
    std::fs::write(fixture.workspace.join("search.py"), "def find_positions(text, query):\n    if not query:\n        return []\n    positions, start = [], 0\n    while (offset := text.find(query, start)) != -1:\n        positions.append(offset)\n        start = offset + len(query)\n    return positions\n").expect("correct candidate");
    std::fs::write(fixture.workspace.join("test_placeholder.py"), "import unittest\nclass Placeholder(unittest.TestCase):\n    def test_true(self): self.assertTrue(True)\n").expect("irrelevant test");
    let score = score(&fixture).expect("objective grade");
    assert!(!score.passed && score.public_tests_present && !score.public_tests_catch_original_bug);
}
