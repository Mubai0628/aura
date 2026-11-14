//! What `cargo xtask test` runs: packages, kinds of test program and the test arguments.
use std::collections::BTreeSet;

use anyhow::{Context, Result, bail, ensure};

use super::artifacts::{Kind, Program};

pub(super) const USAGE: &str = "usage: cargo xtask test [-p <package>]... [--lib] [--bins] \
                                [--tests] [--test <name>]... [-- <test arguments>]";

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Options {
    pub(super) packages: Vec<String>,
    pub(super) lib: bool,
    pub(super) bins: bool,
    pub(super) tests: bool,
    pub(super) named_tests: Vec<String>,
    pub(super) test_arguments: Vec<String>,
}

impl Options {
    pub(super) fn parse(arguments: Vec<String>) -> Result<Self> {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "-p" | "--package" => options.packages.push(
                    arguments
                        .next()
                        .with_context(|| format!("{argument} needs a package\n{USAGE}"))?,
                ),
                "--lib" => options.lib = true,
                "--bins" => options.bins = true,
                "--tests" => options.tests = true,
                "--test" => options.named_tests.push(
                    arguments.next().with_context(|| format!("--test needs a name\n{USAGE}"))?,
                ),
                // What every run is anyway: the whole workspace with all features is built, and
                // every selected program runs even after a failure.
                "--workspace" | "--all-features" | "--no-fail-fast" | "--locked" => {}
                "--" => options.test_arguments = arguments.by_ref().collect(),
                other => bail!(
                    "{other} is not supported: every Rust test run builds the workspace with all \
                     features once and runs the programs it selects\n{USAGE}"
                ),
            }
        }
        Ok(options)
    }

    /// The programs to run, in cargo's order. Without a kind every kind runs; `--test` adds the
    /// named integration test programs.
    pub(super) fn select<'a>(&self, programs: &'a [Program]) -> Result<Vec<&'a Program>> {
        let known: BTreeSet<&str> =
            programs.iter().map(|program| program.package.as_str()).collect();
        for package in &self.packages {
            ensure!(
                known.contains(package.as_str()),
                "no test programs in a package named {package}"
            );
        }
        let every_kind = !self.lib && !self.bins && !self.tests && self.named_tests.is_empty();
        let selected: Vec<&Program> = programs
            .iter()
            .filter(|program| self.packages.is_empty() || self.packages.contains(&program.package))
            .filter(|program| match program.kind {
                Kind::Lib => every_kind || self.lib,
                Kind::Bin => every_kind || self.bins,
                Kind::Test => every_kind || self.tests || self.named_tests.contains(&program.name),
                Kind::Other => every_kind,
            })
            .collect();
        for name in &self.named_tests {
            ensure!(
                selected.iter().any(|program| program.kind == Kind::Test && &program.name == name),
                "no integration test program named {name} in the selected packages"
            );
        }
        ensure!(!selected.is_empty(), "nothing to run\n{USAGE}");
        Ok(selected)
    }
}
