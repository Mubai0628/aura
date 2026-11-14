//! What the launcher asks a version's Core before the desktop starts, answered without starting
//! Core and on standard output:
//! - `--store-migration-check <data folder> [--replacing <version>]`: which stores this version's
//!   first start would migrate, or write records into that the version it replaces refuses, so the
//!   launcher copies only those;
//! - `--store-slim <data folder>`: clearing what older versions left in the stores and nothing reads
//!   again, then rebuilding the files;
//! - `--store-check <data folder>`: SQLite's quick check of the stores, as a move checks its copy.
#![forbid(unsafe_code)]

use std::{io::Write, path::PathBuf};

mod migration_check;
mod slim;
mod store_check;

pub use store_check::check_stores;

/// The database files under the data folder the launcher names.
const LEGACY: &str = "data/aura.db";
const LEGACY_ARTIFACTS: &str = "data/artifact-objects";
const EXECUTION: &str = "data/v2-execution";

/// Answers the command `arguments` (the whole command line) name, writing to `out`: the exit code,
/// or `None` when they name neither command.
pub fn command(arguments: &[String], out: &mut dyn Write) -> Option<u8> {
    let after = |flag: &str| {
        let at = arguments.iter().position(|argument| argument == flag)?;
        Some(arguments.get(at + 1).map(PathBuf::from))
    };
    if let Some(root) = after("--store-migration-check") {
        let Some(root) = root else {
            eprintln!("Aura Core argument error: --store-migration-check needs the data folder");
            return Some(2);
        };
        let replacing = after("--replacing").flatten();
        let replacing = replacing.as_ref().and_then(|version| version.to_str());
        return Some(migration_check::answer(&root, replacing, out));
    }
    if let Some(root) = after("--store-slim") {
        let Some(root) = root else {
            eprintln!("Aura Core argument error: --store-slim needs the data folder");
            return Some(2);
        };
        return Some(slim::run(&root, &aura_windows_platform::free_disk_space, out));
    }
    if let Some(root) = after("--store-check") {
        let Some(root) = root else {
            eprintln!("Aura Core argument error: --store-check needs the data folder");
            return Some(2);
        };
        return Some(store_check::run(&root, out));
    }
    None
}
