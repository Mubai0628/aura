use anyhow::Result;
use std::path::{Path, PathBuf};

pub(super) fn implementation_files(task: &str) -> &'static [&'static str] {
    match task {
        "R01" => &["src/lib.rs"],
        "T01" => &["page.ts"],
        "P01" => &["search.py"],
        "R06" => &["src/parser.rs", "src/limits.rs"],
        "T06" => &["policy.ts", "client.ts"],
        "P06" => &["graph.py", "planner.py"],
        _ => unreachable!("only implemented catalog tasks are graded"),
    }
}

/// What the model changed, for the report. Hidden behavior tests decide completion: a correct
/// fix may live in fewer modules than the reference solution, so this is never a pass condition.
pub(super) struct ModelWork {
    pub(super) implementation_files_changed: usize,
    pub(super) regression_edit: bool,
}

pub(super) fn model_work(
    task: &str,
    language: &str,
    original: &Path,
    candidate: &Path,
    tests: &[PathBuf],
) -> Result<ModelWork> {
    let mut changed = 0;
    for path in implementation_files(task) {
        // A moved or deleted module counts as changed; it must not abort scoring.
        let after = std::fs::read(candidate.join(path)).ok();
        changed +=
            usize::from(after.as_deref() != Some(std::fs::read(original.join(path))?.as_slice()));
    }
    if !matches!(task, "R06" | "T06" | "P06") {
        return Ok(ModelWork { implementation_files_changed: changed, regression_edit: true });
    }
    let mut regression_edit = false;
    for path in tests {
        let before = original.join(path);
        let after = candidate.join(path);
        if language == "Rust" {
            // An unparsable test file is still an edit; the test run decides the result.
            regression_edit |=
                super::super::rust_regression::has_test_edit(&before, &after).unwrap_or(true);
        } else {
            regression_edit |= !before.exists() || std::fs::read(before)? != std::fs::read(after)?;
        }
    }
    Ok(ModelWork { implementation_files_changed: changed, regression_edit })
}
