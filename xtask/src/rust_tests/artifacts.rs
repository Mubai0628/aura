//! The test build and the test programs cargo reports for it.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{Context, Result, ensure};
use serde_json::Value;

use super::TEST_BUILD;
use crate::BUILD_JOBS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Lib,
    Bin,
    Test,
    Other,
}

#[derive(Clone, Debug)]
pub(super) struct Program {
    pub(super) package: String,
    pub(super) version: String,
    pub(super) manifest_dir: PathBuf,
    pub(super) kind: Kind,
    pub(super) name: String,
    pub(super) source: PathBuf,
    pub(super) executable: PathBuf,
}

impl Program {
    /// `lib`, `bin <name>` or `test <name>`, as a failure lists it.
    pub(super) fn label(&self) -> String {
        match self.kind {
            Kind::Lib => "lib".to_owned(),
            Kind::Bin => format!("bin {}", self.name),
            Kind::Test => format!("test {}", self.name),
            Kind::Other => self.name.clone(),
        }
    }
}

/// Builds every test program of the test build, as `check-all` does, and returns them in the
/// order cargo runs them: by package, the library first, then programs, then integration tests.
pub(super) fn build(root: &Path) -> Result<Vec<Program>> {
    let packages = packages(root)?;
    let messages = messages(root, &[&["test"][..], &TEST_BUILD, &["--no-run"]].concat())
        .context("the test build failed")?;
    super::used::record(root, super::used::TEST_BUILD, &messages)?;
    let mut programs = parse(&messages, &packages)?;
    programs.sort_by(|left, right| {
        (&left.package, left.kind, &left.name).cmp(&(&right.package, right.kind, &right.name))
    });
    Ok(programs)
}

/// Runs cargo with `arguments` and returns its JSON messages. Diagnostics and progress go to the
/// terminal, rendered as usual; stdout carries only the messages.
pub(super) fn messages(root: &Path, arguments: &[&str]) -> Result<String> {
    let (command, rest) = arguments.split_first().context("a cargo command without arguments")?;
    let arguments = [&[*command, "--message-format=json-render-diagnostics"][..], rest].concat();
    println!("$ cargo {}", arguments.join(" "));
    let output = crate::process::command("cargo")
        .args(&arguments)
        .current_dir(root)
        .env("CARGO_BUILD_JOBS", BUILD_JOBS)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .context("failed to start cargo")?;
    ensure!(output.status.success(), "cargo {command} failed with {}", output.status);
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Package name and version by manifest path, from `cargo metadata`.
fn packages(root: &Path) -> Result<HashMap<PathBuf, (String, String)>> {
    let output = crate::process::command("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .stderr(Stdio::inherit())
        .output()
        .context("failed to start cargo metadata")?;
    ensure!(output.status.success(), "cargo metadata failed with {}", output.status);
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let mut packages = HashMap::new();
    for package in metadata["packages"].as_array().context("cargo metadata lists no packages")? {
        let field = |name: &str| {
            package[name].as_str().map(str::to_owned).with_context(|| format!("package {name}"))
        };
        packages
            .insert(PathBuf::from(field("manifest_path")?), (field("name")?, field("version")?));
    }
    Ok(packages)
}

/// The test programs among cargo's `compiler-artifact` messages.
pub(super) fn parse(
    stdout: &str,
    packages: &HashMap<PathBuf, (String, String)>,
) -> Result<Vec<Program>> {
    let mut programs = Vec::new();
    for line in stdout.lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else { continue };
        if message["reason"] != "compiler-artifact" || message["profile"]["test"] != true {
            continue;
        }
        let Some(executable) = message["executable"].as_str() else { continue };
        let manifest = PathBuf::from(
            message["manifest_path"].as_str().context("artifact without a manifest path")?,
        );
        let (package, version) = packages
            .get(&manifest)
            .with_context(|| format!("{} is not a workspace package", manifest.display()))?
            .clone();
        let kinds = message["target"]["kind"].as_array().cloned().unwrap_or_default();
        let has = |wanted: &[&str]| {
            kinds.iter().any(|kind| kind.as_str().is_some_and(|kind| wanted.contains(&kind)))
        };
        let kind = if has(&["test"]) {
            Kind::Test
        } else if has(&["bin"]) {
            Kind::Bin
        } else if has(&["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"]) {
            Kind::Lib
        } else {
            Kind::Other
        };
        programs.push(Program {
            package,
            version,
            manifest_dir: manifest.parent().context("manifest without a folder")?.to_path_buf(),
            kind,
            name: message["target"]["name"].as_str().unwrap_or_default().to_owned(),
            source: PathBuf::from(message["target"]["src_path"].as_str().unwrap_or_default()),
            executable: PathBuf::from(executable),
        });
    }
    Ok(programs)
}
