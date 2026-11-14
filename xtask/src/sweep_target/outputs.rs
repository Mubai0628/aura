//! Compiled units in `deps`: the stale ones, and test programs nobody has built for a while.
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
    time::SystemTime,
};

use anyhow::Result;

use super::{Removal, dependency_info};

pub(super) const STALE: &str = "units older than their sources";
pub(super) const IDLE_PROGRAM: &str = "test programs not built within the idle period";
pub(super) const UNUSED: &str =
    "units the test build, Clippy and xtask do not use, from before them";

/// Size and modification time of a file.
type Stamp = (u64, SystemTime);

/// Units whose dependency file names a source that is gone or newer than every output of the
/// unit. Cargo rebuilds such a unit before using it, so its outputs only take space.
pub(super) fn stale_units(root: &Path, debug: &Path) -> Result<Vec<Removal>> {
    let deps = debug.join("deps");
    let files = listing(&deps)?;
    let uplifted = uplifted(debug)?;
    let mut removals = Vec::new();
    for (name, stamp) in files.iter().filter(|(name, _)| name.ends_with(".d")) {
        let unit = &name[..name.len() - 2];
        let outputs: Vec<(&String, &Stamp)> =
            files.iter().filter(|(other, _)| belongs(other, unit)).collect();
        // A program cargo also keeps under `target/debug` is in use there; deleting the `deps`
        // copy would free nothing.
        if outputs.iter().any(|(_, stamp)| uplifted.contains(stamp)) {
            continue;
        }
        let built = outputs.iter().map(|(_, (_, modified))| *modified).max();
        let text = fs::read_to_string(deps.join(name)).unwrap_or_default();
        let stale = built.is_none_or(|built| {
            dependency_info::sources(&text, root).iter().any(|source| {
                fs::metadata(source)
                    .and_then(|metadata| metadata.modified())
                    .map_or(true, |changed| changed > built)
            })
        });
        if !stale {
            continue;
        }
        for (output, (bytes, _)) in outputs.into_iter().chain([(name, stamp)]) {
            removals.push(Removal { path: deps.join(output), bytes: *bytes, reason: STALE });
        }
    }
    Ok(removals)
}

/// Test programs and their debug files not written since `idle_before`. A program cargo runs
/// is relinked when needed; libraries stay, since cargo does not rewrite a library that is still
/// current and a rebuilt one makes everything above it rebuild.
pub(super) fn idle_test_programs(debug: &Path, idle_before: SystemTime) -> Result<Vec<Removal>> {
    let deps = debug.join("deps");
    let files = listing(&deps)?;
    let uplifted = uplifted(debug)?;
    let mut removals = Vec::new();
    for (name, (bytes, modified)) in &files {
        let Some(stem) = name.strip_suffix(".exe") else { continue };
        if stem.starts_with("xtask-")
            || *modified >= idle_before
            || uplifted.contains(&(*bytes, *modified))
        {
            continue;
        }
        removals.push(Removal { path: deps.join(name), bytes: *bytes, reason: IDLE_PROGRAM });
        let debug_file = format!("{stem}.pdb");
        if let Some((bytes, _)) = files.get(&debug_file) {
            removals.push(Removal {
                path: deps.join(&debug_file),
                bytes: *bytes,
                reason: IDLE_PROGRAM,
            });
        }
    }
    Ok(removals)
}

/// Files in `deps` of units outside `used`, not written since `before`.
pub(super) fn unused_units(
    debug: &Path,
    before: SystemTime,
    used: &HashSet<String>,
) -> Result<Vec<Removal>> {
    let files = listing(&debug.join("deps"))?;
    let uplifted = uplifted(debug)?;
    Ok(unused_files(&files, &uplifted, before, used)
        .into_iter()
        .map(|(name, bytes)| Removal { path: debug.join("deps").join(name), bytes, reason: UNUSED })
        .collect())
}

/// [`unused_units`] over a listing: names and sizes.
fn unused_files(
    files: &BTreeMap<String, Stamp>,
    uplifted: &HashSet<Stamp>,
    before: SystemTime,
    used: &HashSet<String>,
) -> Vec<(String, u64)> {
    files
        .iter()
        .filter(|(name, stamp)| {
            stamp.1 < before
                && !uplifted.contains(stamp)
                && !crate::rust_tests::unit_names(name).any(|unit| used.contains(unit))
        })
        .map(|(name, (bytes, _))| (name.clone(), *bytes))
        .collect()
}

/// Whether `file` in `deps` is an output of `unit` (`name-hash`): `unit.exe`, `unit.pdb`,
/// `libunit.rlib`, `libunit.rmeta` and the like, not the unit's own `.d` file.
pub(super) fn belongs(file: &str, unit: &str) -> bool {
    let stem = file.strip_prefix("lib").filter(|rest| rest.starts_with(unit)).unwrap_or(file);
    stem.strip_prefix(unit).is_some_and(|rest| rest.starts_with('.') && rest != ".d")
}

/// Size and modification time of every file in a folder, by name.
fn listing(folder: &Path) -> Result<BTreeMap<String, Stamp>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            files.insert(
                entry.file_name().to_string_lossy().into_owned(),
                (metadata.len(), metadata.modified()?),
            );
        }
    }
    Ok(files)
}

/// The files cargo also keeps in `target/debug` itself, by size and time. They are hard links of
/// outputs in `deps` (the programs `pnpm dev` and the E2E tests start), so the `deps` file with
/// the same size and time is the same file.
fn uplifted(debug: &Path) -> Result<HashSet<Stamp>> {
    Ok(listing(debug)?.into_values().collect())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, HashSet},
        time::{Duration, SystemTime},
    };

    #[test]
    fn an_unused_unit_goes_once_idle_and_a_used_new_or_linked_one_stays() {
        let now = SystemTime::now();
        let old = now - Duration::from_secs(10 * 24 * 60 * 60);
        let idle_before = now - Duration::from_secs(3 * 24 * 60 * 60);
        let files: BTreeMap<String, (u64, SystemTime)> = [
            ("libused-0123.rlib", (1, old)),
            ("libused-0123.rmeta", (2, old)),
            ("used-0123.d", (3, old)),
            ("libgone-4567.rlib", (4, old)),
            ("gone-4567.d", (5, old)),
            ("fresh-89ab.exe", (6, now)),
            ("aura-core.exe", (7, old)),
        ]
        .into_iter()
        .map(|(name, stamp)| (name.to_owned(), stamp))
        .collect();
        let uplifted = HashSet::from([(7, old)]);
        let used = HashSet::from(["used-0123".to_owned()]);
        let listed: Vec<String> = super::unused_files(&files, &uplifted, idle_before, &used)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(listed, ["gone-4567.d", "libgone-4567.rlib"]);
    }

    #[test]
    fn a_unit_owns_its_program_debug_file_and_library_files_only() {
        let unit = "aura_fs_tools-0123";
        for file in [
            "aura_fs_tools-0123.exe",
            "aura_fs_tools-0123.pdb",
            "libaura_fs_tools-0123.rlib",
            "libaura_fs_tools-0123.rmeta",
        ] {
            assert!(super::belongs(file, unit), "{file}");
        }
        for file in [
            "aura_fs_tools-0123.d",
            "aura_fs_tools-01234.exe",
            "aura_fs_tools-0123",
            "libaura_fs_tools-0124.rlib",
            "aura_fs_tools_more-0123.exe",
        ] {
            assert!(!super::belongs(file, unit), "{file}");
        }
    }
}
