//! Starting the commands xtask runs. `cargo xtask` starts xtask through `cargo run`, which gives it
//! the package variables of xtask itself (`CARGO_MANIFEST_DIR`, `CARGO_PKG_*`); a command started
//! from here does not inherit them. A build script that tracks one of them (ring tracks
//! `CARGO_MANIFEST_DIR`) would otherwise see another value than under a cargo started from a
//! shell, and nearly the whole workspace would be rebuilt each time the two take turns.
use std::{
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, ensure};

use crate::{BUILD_JOBS, TEST_THREADS};

/// `program`, without the package variables cargo set for xtask.
pub(crate) fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        if name.starts_with("CARGO_PKG_")
            || matches!(
                &*name,
                "CARGO_MANIFEST_DIR"
                    | "CARGO_MANIFEST_PATH"
                    | "CARGO_CRATE_NAME"
                    | "CARGO_BIN_NAME"
                    | "CARGO_PRIMARY_PACKAGE"
            )
        {
            command.env_remove(&*name);
        }
    }
    command
}

pub(crate) fn run(root: &Path, program: &str, args: &[&str]) -> Result<()> {
    run_with_env(root, program, args, &[])
}

pub(crate) fn run_with_env(
    root: &Path,
    program: &str,
    args: &[&str],
    environment: &[(&str, &std::ffi::OsStr)],
) -> Result<()> {
    println!("$ {program} {}", args.join(" "));
    let mut command = command(program);
    command.args(args);
    if program == "cargo" {
        command.env("CARGO_BUILD_JOBS", BUILD_JOBS).env("RUST_TEST_THREADS", TEST_THREADS);
    }
    for (name, value) in environment {
        command.env(name, value);
    }
    let status = command
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to start {program}"))?;
    ensure!(status.success(), "command failed with {status}: {program} {}", args.join(" "));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_started_command_leaves_out_the_package_variables_cargo_set_for_xtask() {
        // `cargo test` runs this with xtask's package variables, as `cargo xtask` runs xtask.
        assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
        let removed: Vec<String> = super::command("cargo")
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        for name in ["CARGO_MANIFEST_DIR", "CARGO_PKG_NAME", "CARGO_PKG_VERSION"] {
            assert!(
                removed.iter().any(|gone| gone.eq_ignore_ascii_case(name)),
                "{name}: {removed:?}"
            );
        }
        assert!(!removed.iter().any(|gone| gone.eq_ignore_ascii_case("PATH")));
    }
}
