use aura_tool_eval::fixtures::{PreparedTask, prepare, score, task};
use std::path::Path;

fn copy_reference(id: &str, fixture: &PreparedTask, include_test: bool) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(id).join("reference");
    for entry in std::fs::read_dir(base).expect("reference solution") {
        let entry = entry.expect("reference entry");
        let name = entry.file_name();
        let is_test = name.to_string_lossy().contains("regression");
        if include_test || !is_test {
            std::fs::copy(entry.path(), fixture.workspace.join(name))
                .expect("apply isolated reference");
        }
    }
}

#[test]
fn realistic_reference_solutions_pass_and_their_tests_catch_both_original_defects() {
    // Rust grading is covered through real Core replay after the outer Cargo process ends.
    for id in ["T06", "P06"] {
        let root = tempfile::tempdir().expect("isolated candidate");
        let fixture = prepare(root.path(), task(id).expect("task")).expect("copy fixture");
        copy_reference(id, &fixture, true);
        let scored = score(&fixture).expect("score a corrected multi-file candidate");
        assert!(scored.passed, "{id}: reference must pass public and hidden tests");
        assert!(
            scored.public_tests_catch_original_bug,
            "{id}: regression restores all buggy modules"
        );
    }
}

#[test]
fn realistic_correct_code_without_a_new_regression_is_not_completion() {
    for id in ["T06", "P06"] {
        let root = tempfile::tempdir().expect("isolated candidate");
        let fixture = prepare(root.path(), task(id).expect("task")).expect("copy fixture");
        copy_reference(id, &fixture, false);
        let scored = score(&fixture).expect("score missing regression");
        assert!(!scored.passed, "{id}: existing tests are insufficient");
        let log = std::fs::read(scored.log).expect("original score evidence bytes");
        assert!(
            log.windows(b"eval.regression_edit_required".len())
                .any(|window| window == b"eval.regression_edit_required")
        );
    }
}

#[test]
fn restoring_either_required_module_breaks_the_realistic_solution() {
    for (id, modules) in [("T06", ["policy.ts", "client.ts"]), ("P06", ["graph.py", "planner.py"])]
    {
        for module in modules {
            let root = tempfile::tempdir().expect("isolated candidate");
            let fixture = prepare(root.path(), task(id).expect("task")).expect("copy fixture");
            copy_reference(id, &fixture, true);
            let original = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures")
                .join(id)
                .join("workspace")
                .join(module);
            std::fs::copy(original, fixture.workspace.join(module))
                .expect("omit one required implementation");
            let scored = score(&fixture).expect("score partial multi-file solution");
            assert!(!scored.passed, "{id}: leaving {module} unchanged must fail");
            assert_ne!(
                scored.exit_code,
                Some(0),
                "{id}: behavior tests catch the remaining defect"
            );
            assert_eq!(scored.implementation_files_changed, 1);
        }
    }
}

#[test]
fn a_behavior_correct_fix_in_fewer_modules_than_the_reference_still_passes() {
    // Refusing cycles inside graph.ordered() leaves planner.py untouched yet meets every rule.
    let root = tempfile::tempdir().expect("isolated candidate");
    let fixture = prepare(root.path(), task("P06").expect("task")).expect("copy fixture");
    copy_reference("P06", &fixture, true);
    let original = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/P06/workspace");
    std::fs::copy(original.join("planner.py"), fixture.workspace.join("planner.py"))
        .expect("keep the original planner");
    let graph = std::fs::read_to_string(fixture.workspace.join("graph.py")).expect("graph");
    let anchor = "def ordered(graph: Graph) -> list[str]:\n";
    assert_eq!(graph.matches(anchor).count(), 1);
    let refusing = "    cycle = find_cycle(graph)\n    if cycle:\n        from model import CycleError\n        raise CycleError(cycle)\n";
    std::fs::write(
        fixture.workspace.join("graph.py"),
        graph.replace(anchor, &(anchor.to_owned() + refusing)),
    )
    .expect("single-module solution");
    let scored = score(&fixture).expect("score an alternative correct solution");
    assert!(scored.passed, "hidden behavior tests, not the reference layout, decide completion");
    assert_eq!(scored.implementation_files_changed, 1);
}
