use aura_tool_eval::fixtures::{prepare, task};
use std::path::Path;

fn inventory(path: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(path).expect("fixture directory") {
        let entry = entry.expect("fixture entry");
        if entry.file_type().expect("entry kind").is_dir() {
            inventory(&entry.path(), files);
        } else {
            files.push(entry.path());
        }
    }
}

#[test]
fn real_scale_fixtures_are_ready_and_keep_graders_out_of_the_agent_workspace() {
    for id in ["R06", "T06", "P06"] {
        let root = tempfile::tempdir().expect("isolated fixture copy");
        let fixture = prepare(root.path(), task(id).expect("catalog task"))
            .expect("implemented real-scale fixture");
        let mut files = vec![];
        inventory(&fixture.workspace, &mut files);
        assert!((5..=15).contains(&files.len()), "{id} has a representative multi-file workspace");
        let lines: usize = files
            .iter()
            .filter(|path| path.extension().is_some_and(|v| v == "rs" || v == "ts" || v == "py"))
            .map(|path| {
                std::fs::read_to_string(path).expect("source or public test").lines().count()
            })
            .sum();
        assert!((300..=1000).contains(&lines), "{id} source plus public tests: {lines}");
        assert!(!fixture.grading.exists(), "{id} hidden tests only arrive after stopping");
        assert!(
            !files.iter().any(|path| path
                .file_name()
                .is_some_and(|v| v.to_string_lossy().contains("hidden")))
        );
        assert!(
            !fixture.task.prompt.contains(".rs")
                && !fixture.task.prompt.contains(".ts")
                && !fixture.task.prompt.contains(".py"),
            "the issue description does not name implementation files"
        );
    }
}

#[test]
fn every_rust_fixture_carries_the_parallel_limits_into_model_shells() {
    for id in ["R01", "R06"] {
        let root = tempfile::tempdir().expect("fixture root");
        let fixture =
            prepare(root.path(), task(id).expect("Rust task")).expect("copy Rust fixture");
        let config = std::fs::read_to_string(fixture.workspace.join(".cargo/config.toml"))
            .expect("repository-local Cargo policy");
        assert!(config.contains("jobs = 2"), "{id}: Cargo controls model child processes");
        assert!(config.contains("RUST_TEST_THREADS = \"2\""), "{id}: libtest is bounded too");
    }
}
