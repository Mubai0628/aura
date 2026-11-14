//! Rust tests through one build configuration.
//!
//! Cargo names every compiled unit by a hash of its package, its features, its profile (debug
//! level and incremental setting included) and the hashes of its dependencies, and the features a
//! crate gets depend on the packages a command selects. `cargo test -p a` and
//! `cargo test --workspace --all-features` therefore build two copies of most crates and of every
//! test program, every further selection, feature or `CARGO_INCREMENTAL` / `CARGO_PROFILE_*`
//! override adds another, and cargo deletes none of them. A [`Session`] builds what `check-all`
//! builds and runs only the test programs asked for, so stage runs, fix checks and the full gate
//! share one set of build outputs.
mod alone;
mod artifacts;
mod fixtures;
mod listeners;
mod options;
mod run;
mod run_temp;
mod space;
#[cfg(test)]
mod tests;
mod threads;
mod used;

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};

use artifacts::Program;
use options::Options;
use run_temp::RunTemp;

pub(crate) use fixtures::target_folder;
pub(crate) use space::free_gb;
pub(crate) use used::{unit_names, units as used_units};

/// What every Rust test build selects: the packages and features `check-all` tests.
pub(crate) const TEST_BUILD: [&str; 2] = ["--workspace", "--all-features"];

/// `cargo xtask test [-p <package>]... [--lib] [--bins] [--tests] [--test <name>]... [-- <test
/// arguments>]`.
pub(crate) fn test(root: &Path, arguments: Vec<String>) -> Result<()> {
    let options = Options::parse(arguments)?;
    let mut session = Session::start(root)?;
    session.run_options(&options, &[])?;
    session.finish()
}

/// `check-all`'s test step: every test program and the documentation tests, all of them run even
/// after a failure, so one run shows every failing program.
pub(crate) fn workspace(root: &Path) -> Result<()> {
    let mut session = Session::start(root)?;
    session.run_options(&Options::default(), &[])?;
    session.doc_tests()?;
    session.finish()
}

/// Clippy as `check-all` runs it; a narrower selection would check other feature combinations.
pub(crate) fn clippy(root: &Path) -> Result<()> {
    let arguments =
        [&["clippy"][..], &TEST_BUILD, &["--all-targets", "--", "-D", "warnings"]].concat();
    let messages = artifacts::messages(root, &arguments)?;
    used::record(root, used::CLIPPY, &messages)
}

/// The JSON messages of a cargo build started as xtask starts its own.
pub(crate) fn cargo_messages(root: &Path, arguments: &[&str]) -> Result<String> {
    artifacts::messages(root, arguments)
}

/// One test build, the TEMP folder its runs share and the runs that failed so far.
pub(crate) struct Session {
    root: PathBuf,
    programs: Vec<Program>,
    temp: RunTemp,
    environment: Vec<(&'static str, OsString)>,
    failed: Vec<String>,
}

impl Session {
    /// Checks the free space and builds every test program of the test build.
    pub(crate) fn start(root: &Path) -> Result<Self> {
        space::ensure(root)?;
        let programs = artifacts::build(root)?;
        fixtures::check(root)?;
        let temp = RunTemp::create()?;
        let environment = temp.environment()?;
        Ok(Self { root: root.to_path_buf(), programs, temp, environment, failed: Vec::new() })
    }

    /// Runs what `arguments` select, written as `cargo xtask test` takes them. Returns whether
    /// every selected program passed; a failure is also kept for [`Self::finish`].
    pub(crate) fn run(&mut self, arguments: &[&str]) -> Result<bool> {
        self.run_with_env(arguments, &[])
    }

    /// As [`Self::run`], with extra variables for the test programs (a probe's report path).
    pub(crate) fn run_with_env(
        &mut self,
        arguments: &[&str],
        extra: &[(&str, &OsStr)],
    ) -> Result<bool> {
        let options = Options::parse(arguments.iter().map(|&value| value.to_owned()).collect())?;
        self.run_options(&options, extra)
    }

    fn run_options(&mut self, options: &Options, extra: &[(&str, &OsStr)]) -> Result<bool> {
        let selected = options.select(&self.programs)?;
        let mut environment: Vec<(&str, &OsStr)> =
            self.environment.iter().map(|(name, value)| (*name, value.as_os_str())).collect();
        environment.extend_from_slice(extra);
        let mut failed = Vec::new();
        if options.test_arguments.is_empty() {
            // Tests with a time budget run once the others have ended, one at a time.
            let alone = alone::in_selection(&selected);
            let skipped = alone::skip_arguments(&alone);
            failed.extend(run::run(&self.root, &selected, &skipped, &environment)?);
            for (program, test) in alone {
                let arguments = alone::run_arguments(test);
                failed.extend(run::run(&self.root, &[program], &arguments, &environment)?);
            }
        } else {
            failed.extend(run::run(&self.root, &selected, &options.test_arguments, &environment)?);
        }
        let passed = failed.is_empty();
        self.failed.extend(failed);
        Ok(passed)
    }

    /// The documentation tests of every library, built from the libraries the test build made.
    pub(crate) fn doc_tests(&mut self) -> Result<bool> {
        let arguments = [&["test"][..], &TEST_BUILD, &["--doc"]].concat();
        let environment: Vec<(&str, &OsStr)> =
            self.environment.iter().map(|(name, value)| (*name, value.as_os_str())).collect();
        let passed = crate::process::run_with_env(&self.root, "cargo", &arguments, &environment)
            .inspect_err(|error| eprintln!("{error:#}"))
            .is_ok();
        if !passed {
            self.failed.push("documentation tests".to_owned());
        }
        Ok(passed)
    }

    /// Deletes the run's TEMP folder and fails if any run failed.
    pub(crate) fn finish(self) -> Result<()> {
        let Self { failed, temp, .. } = self;
        drop(temp);
        if !failed.is_empty() {
            bail!("{} failed:\n  {}", plural(failed.len()), failed.join("\n  "));
        }
        Ok(())
    }
}

fn plural(count: usize) -> String {
    if count == 1 { "1 test program".to_owned() } else { format!("{count} test programs") }
}
