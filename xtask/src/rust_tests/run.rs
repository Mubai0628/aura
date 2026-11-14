//! Running test programs the way `cargo test` runs them: one after another, from the package
//! folder, with the package variables cargo sets, and every program run even after a failure.
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{Context, Result};

use super::artifacts::Program;

/// Runs `programs` with `arguments` and returns the ones that failed.
pub(super) fn run(
    root: &Path,
    programs: &[&Program],
    arguments: &[String],
    environment: &[(&str, &OsStr)],
) -> Result<Vec<String>> {
    let mut failed = Vec::new();
    for program in programs {
        println!(
            "     Running {} ({})",
            relative(root, &program.source).display(),
            relative(root, &program.executable).display()
        );
        let mut command = crate::process::command(super::listeners::launch_path(program));
        command
            .args(arguments)
            .current_dir(&program.manifest_dir)
            .env("CARGO_MANIFEST_DIR", &program.manifest_dir)
            .env("CARGO_MANIFEST_PATH", program.manifest_dir.join("Cargo.toml"))
            .env("CARGO_PKG_NAME", &program.package)
            .env("CARGO_PKG_VERSION", &program.version)
            .env("RUST_TEST_THREADS", super::threads::for_program(program))
            .env("PATH", search_path(program)?)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        for (name, value) in environment {
            command.env(name, value);
        }
        let status = command
            .status()
            .with_context(|| format!("failed to start {}", program.executable.display()))?;
        if !status.success() {
            failed.push(format!("{} {} ({status})", program.package, program.label()));
        }
    }
    Ok(failed)
}

/// The program's own folder first, as cargo puts the dependency folder first for test runs.
fn search_path(program: &Program) -> Result<OsString> {
    let folder = program.executable.parent().context("test program without a folder")?;
    let mut paths = vec![folder.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    Ok(std::env::join_paths(paths)?)
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).map(Path::to_path_buf).unwrap_or_else(|_| path.to_path_buf())
}
