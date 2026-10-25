use super::{PreparedTask, workspace::copy_tree};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
mod requirements;

#[derive(Serialize)]
pub struct Score {
    pub task: String,
    pub passed: bool,
    pub public_tests_present: bool,
    pub public_tests_catch_original_bug: bool,
    /// Reported only; behavior tests decide completion.
    pub implementation_files_changed: usize,
    pub exit_code: Option<i32>,
    pub log: PathBuf,
}

/// Score the finished copy, never the Agent's live workspace. Public regression
/// tests must also fail against the original buggy implementation.
pub fn score(prepared: &PreparedTask) -> Result<Score> {
    ensure!(!prepared.grading.exists(), "eval.already_scored");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(prepared.task.id);
    copy_tree(&source.join("grader"), &prepared.grading)?;
    let copied = prepared.grading.join("candidate");
    copy_tree(&prepared.workspace, &copied)?;
    let tests = public_tests(&copied, prepared.task.language)?;
    let present = !tests.is_empty();
    let work = requirements::model_work(
        prepared.task.id,
        prepared.task.language,
        &source.join("workspace"),
        &copied,
        &tests,
    )?;
    let mut log = Vec::new();
    let regression_catches = if present {
        let regression = prepared.grading.join("original-bug");
        copy_tree(&copied, &regression)?;
        for relative in requirements::implementation_files(prepared.task.id) {
            if restore_implementation(
                &source.join("workspace").join(relative),
                &regression.join(relative),
                prepared.task.language,
            )? {
                log.extend_from_slice(
                    format!("eval.note_whole_file_restored {relative}\n").as_bytes(),
                );
            }
        }
        let output = crate::process::run(
            &command(prepared.task.language, &regression, &tests),
            Duration::from_secs(120),
        )?;
        log.extend_from_slice(b"Public regression tests against original bug:\n");
        log.extend(output.stdout);
        log.extend(output.stderr);
        !output.timed_out && output.exit_code.is_some_and(|v| v != 0)
    } else {
        false
    };
    let mut all_tests = tests;
    match prepared.task.language {
        "Rust" => {
            std::fs::create_dir_all(copied.join("tests"))?;
            std::fs::copy(prepared.grading.join("hidden.rs"), copied.join("tests/hidden.rs"))?;
        }
        "TypeScript" => {
            std::fs::copy(prepared.grading.join("hidden.mjs"), copied.join("hidden.test.mjs"))?;
            all_tests.push(PathBuf::from("hidden.test.mjs"));
        }
        "Python" => {
            std::fs::copy(prepared.grading.join("hidden.py"), copied.join("test_hidden.py"))?;
        }
        _ => unreachable!("fixed catalog"),
    }
    let output = crate::process::run(
        &command(prepared.task.language, &copied, &all_tests),
        Duration::from_secs(120),
    )?;
    log.extend_from_slice(b"Candidate public and hidden behavior tests:\n");
    log.extend(output.stdout);
    log.extend(output.stderr);
    if !present {
        log.extend_from_slice(b"eval.public_regression_tests_missing\n");
    } else if !regression_catches {
        log.extend_from_slice(b"eval.public_tests_do_not_catch_original_bug\n");
    }
    if work.implementation_files_changed
        < requirements::implementation_files(prepared.task.id).len()
    {
        log.extend_from_slice(b"eval.note_fewer_implementation_files_than_reference\n");
    }
    if !work.regression_edit {
        log.extend_from_slice(b"eval.regression_edit_required\n");
    }
    let path = prepared.grading.join("score.log");
    std::fs::write(&path, log)?;
    Ok(Score {
        task: prepared.task.id.into(),
        passed: present
            && regression_catches
            && work.regression_edit
            && !output.timed_out
            && output.exit_code == Some(0),
        public_tests_present: present,
        public_tests_catch_original_bug: regression_catches,
        implementation_files_changed: work.implementation_files_changed,
        exit_code: output.exit_code,
        log: path,
    })
}
fn command(language: &str, root: &Path, tests: &[PathBuf]) -> Command {
    let mut c = match language {
        "Rust" => {
            let mut c = Command::new("cargo");
            c.args(["test", "--offline", "--locked"])
                .env("CARGO_BUILD_JOBS", "2")
                .env("RUST_TEST_THREADS", "2");
            c
        }
        "TypeScript" => {
            let mut c = Command::new("node");
            c.arg("--test").args(tests);
            c
        }
        "Python" => {
            let mut c = Command::new("python");
            c.args(["-m", "unittest", "discover", "-p", "test*.py"]);
            c
        }
        _ => unreachable!("fixed catalog"),
    };
    c.current_dir(root);
    c
}

/// Returns true when a Rust file had to be restored whole: the candidate no longer parses or
/// lacks an original function, for example after a rename. Scoring continues either way.
fn restore_implementation(original: &Path, candidate: &Path, language: &str) -> Result<bool> {
    if language == "Rust" && super::rust_regression::restore(original, candidate).is_ok() {
        return Ok(false);
    }
    std::fs::copy(original, candidate)?;
    Ok(language == "Rust")
}

#[cfg(test)]
mod tests;
fn public_tests(root: &Path, language: &str) -> Result<Vec<PathBuf>> {
    let mut paths = vec![];
    walk(root, root, language, &mut paths)?;
    paths.sort();
    Ok(paths)
}
fn walk(base: &Path, path: &Path, language: &str, paths: &mut Vec<PathBuf>) -> Result<()> {
    for item in std::fs::read_dir(path)? {
        let item = item?;
        let kind = item.file_type()?;
        ensure!(!kind.is_symlink(), "eval.fixture_links_refused");
        if kind.is_dir() {
            walk(base, &item.path(), language, paths)?;
        } else if kind.is_file() {
            let name = item.file_name().to_string_lossy().into_owned();
            let matched = match language {
                "Rust" => {
                    name.ends_with(".rs")
                        && std::fs::read_to_string(item.path())?.contains("#[test]")
                }
                "TypeScript" => {
                    [".test.ts", ".test.mjs", ".test.js", ".spec.ts", ".spec.mjs", ".spec.js"]
                        .iter()
                        .any(|suffix| name.ends_with(suffix))
                }
                "Python" => name.starts_with("test") && name.ends_with(".py"),
                _ => false,
            };
            if matched {
                paths.push(item.path().strip_prefix(base)?.to_owned());
            }
        }
    }
    Ok(())
}
