//! Source-built fixture programs. Package program tests run `aura-stdio-fixture` and
//! `aura-command-fixture` from `target/debug`. Cargo builds a package's programs for its
//! integration tests and copies them there, so the test build makes both as programs of
//! `aura-tool-fixtures`; a session checks for them before any test runs.
use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};

const FIXTURES: [&str; 2] = ["aura-stdio-fixture", "aura-command-fixture"];

pub(super) fn check(root: &Path) -> Result<()> {
    let debug = target_folder(root).join("debug");
    for name in FIXTURES {
        let program = debug.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        ensure!(
            program.is_file(),
            "the source-built fixture {} is missing after the test build; it is a program of \
             aura-tool-fixtures, which the build makes for that package's integration tests",
            program.display()
        );
    }
    Ok(())
}

/// The target folder cargo uses here: `CARGO_TARGET_DIR`, or `target` as `.cargo/config.toml`
/// sets.
pub(crate) fn target_folder(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || root.join("target"),
        |value| {
            let path = PathBuf::from(value);
            if path.is_absolute() { path } else { root.join(path) }
        },
    )
}
