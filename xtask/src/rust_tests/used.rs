//! The units the last successful test build and Clippy run used, as file names in `deps`.
//!
//! Cargo reports every unit of a build in a `compiler-artifact` message, fresh ones too. Cargo does
//! not rewrite a library that is still current, so a library's age does not show whether a build
//! still reads it; these records do, for the two configurations the Rust checks build.
use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result};
use serde_json::Value;

/// The record of the test build's units.
pub(super) const TEST_BUILD: &str = "test-build";
/// The record of Clippy's units.
pub(super) const CLIPPY: &str = "clippy";

/// What the records name.
pub(crate) struct Used {
    /// The units, as [`unit_names`] reads them.
    pub(crate) units: HashSet<String>,
    /// When the older record was written. A unit last written before it that neither record
    /// names was not part of those builds.
    pub(crate) recorded: SystemTime,
}

fn records_folder(root: &Path) -> PathBuf {
    super::target_folder(root).join("debug").join("xtask-used")
}

fn record_path(root: &Path, record: &str) -> PathBuf {
    records_folder(root).join(format!("{record}.txt"))
}

/// Replaces `record` with the `deps` files of every unit in cargo's JSON `messages`.
pub(super) fn record(root: &Path, record: &str, messages: &str) -> Result<()> {
    let path = record_path(root, record);
    let folder = path.parent().context("a record without a folder")?;
    fs::create_dir_all(folder).with_context(|| format!("failed to create {}", folder.display()))?;
    let mut text = deps_files(messages).into_iter().collect::<Vec<_>>().join("\n");
    text.push('\n');
    fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))
}

/// The file names in `deps` that cargo's `compiler-artifact` messages name.
pub(super) fn deps_files(messages: &str) -> BTreeSet<String> {
    messages
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .flat_map(|message| message["filenames"].as_array().cloned().unwrap_or_default())
        .filter_map(|file| file.as_str().map(PathBuf::from))
        .filter(|file| {
            file.parent().and_then(Path::file_name).is_some_and(|folder| folder == "deps")
        })
        .filter_map(|file| file.file_name().map(|name| name.to_string_lossy().into_owned()))
        .collect()
}

/// The units of both records and of `extra` cargo messages, or `None` while a record is missing.
pub(crate) fn units(root: &Path, extra: &str) -> Option<Used> {
    units_in(&records_folder(root), extra)
}

/// [`units`] with the records in `folder`.
pub(super) fn units_in(folder: &Path, extra: &str) -> Option<Used> {
    let mut files: BTreeSet<String> = deps_files(extra);
    let mut recorded = SystemTime::now();
    for record in [TEST_BUILD, CLIPPY] {
        let path = folder.join(format!("{record}.txt"));
        let text = fs::read_to_string(&path).ok()?;
        let written = fs::metadata(&path).and_then(|metadata| metadata.modified()).ok()?;
        recorded = recorded.min(written);
        files.extend(text.lines().map(str::to_owned));
    }
    let units = files.iter().flat_map(|file| unit_names(file)).map(str::to_owned).collect();
    Some(Used { units, recorded })
}

/// The unit a file in `deps` belongs to, `name-hash`: the name up to its first dot, as written and
/// without a `lib` prefix. A library's files carry the prefix and its dependency file does not, and
/// a crate's own name can start with `lib`, so both readings count.
pub(crate) fn unit_names(file: &str) -> impl Iterator<Item = &str> {
    let stem = file.split('.').next().unwrap_or(file);
    std::iter::once(stem).chain(stem.strip_prefix("lib"))
}
