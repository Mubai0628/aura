//! Removing the copy a move left behind, once the new place's Core is ready. Only what the move
//! recorded is removed, and only where Aura's data can be: inside the folder that never moves
//! (never the Chrome folder or the records there), the default window profile and global memory,
//! the default log for the released product only, or an old location that still holds this
//! installation's marker.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use aura_data_location::{
    LocationRecord, Locations, MARKER_FILE, RECORD_FILE, Roots, current_record, store_record,
    verify_location,
};
use aura_runtime_contract::{BundleNamespace, PRODUCTION_BUNDLE_NAMESPACE};

use crate::{copy::remove_file, journal::JOURNAL_FILE, plan::within};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Removal {
    pub removed: Vec<PathBuf>,
    /// Kept for the next start: what could not be removed, and why.
    pub failed: Vec<(PathBuf, String)>,
    /// Dropped from the record without being touched: not a place Aura's data can be.
    pub refused: Vec<PathBuf>,
}

/// Removes the copies `current`'s record lists and writes the record without them.
pub fn remove_old_copies(
    roots: &Roots,
    namespace: &BundleNamespace,
    current: &Locations,
) -> io::Result<Removal> {
    let mut removal = Removal::default();
    let Some(record) = current_record(&current.record, namespace).map_err(io::Error::other)? else {
        return Ok(removal);
    };
    let defaults = Locations::for_place(roots, namespace, None)
        .with_global_memory(roots, namespace)
        .map_err(io::Error::other)?;
    let mut kept = Vec::new();
    for copy in &record.pending_removal {
        let Some(kind) = removable(copy, current, &defaults, namespace) else {
            removal.refused.push(copy.clone());
            continue;
        };
        let removed = match kind {
            Kind::Default => remove_any(copy),
            Kind::OldLocation => remove_location(copy),
        };
        match removed {
            Ok(()) => removal.removed.push(copy.clone()),
            Err(error) => {
                removal.failed.push((copy.clone(), error.to_string()));
                kept.push(copy.clone());
            }
        }
    }
    store_record(
        &current.record,
        &LocationRecord::with_removals(namespace.clone(), record.location.clone(), kept),
    )?;
    Ok(removal)
}

/// Where a copy that may be removed is.
enum Kind {
    /// One of the default places.
    Default,
    /// A folder the data was moved to before, which still holds this installation's marker.
    OldLocation,
}

fn removable(
    copy: &Path,
    current: &Locations,
    defaults: &Locations,
    namespace: &BundleNamespace,
) -> Option<Kind> {
    // What is in use now is never a copy.
    let in_use = [&current.data_root, &current.ui_profile, &current.fixed_root];
    if in_use.iter().any(|used| within(used, copy))
        || current.location.as_deref().is_some_and(|location| within(location, copy))
        || within(&current.core_log, copy)
        || current.global_memory.as_deref().is_some_and(|memory| within(memory, copy))
    {
        return None;
    }
    let fixed = &defaults.fixed_root;
    if let Ok(below) = copy.strip_prefix(fixed) {
        let first = below.components().next().map(|part| part.as_os_str().to_string_lossy());
        let kept = |name: &str| {
            ["chrome", RECORD_FILE, JOURNAL_FILE].iter().any(|kept| kept.eq_ignore_ascii_case(name))
        };
        return first.is_some_and(|name| !kept(&name)).then_some(Kind::Default);
    }
    let same = |left: &Path, right: &Path| within(left, right) && within(right, left);
    if same(copy, &defaults.core_log) {
        // Every installation at the default places writes this log; only the released product's
        // move takes it along.
        return (namespace.as_str() == PRODUCTION_BUNDLE_NAMESPACE).then_some(Kind::Default);
    }
    if same(copy, &defaults.ui_profile)
        || defaults.global_memory.as_deref().is_some_and(|memory| same(copy, memory))
    {
        return Some(Kind::Default);
    }
    verify_location(copy, namespace).is_ok().then_some(Kind::OldLocation)
}

/// Removes an old location with its marker last: a removal cut short leaves the marker that lets
/// the next start recognise the folder and finish.
fn remove_location(path: &Path) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().eq_ignore_ascii_case(MARKER_FILE) {
            continue;
        }
        remove_any(&entry.path())?;
    }
    remove_file(&path.join(MARKER_FILE))?;
    fs::remove_dir(path)
}

fn remove_any(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(metadata) if metadata.is_dir() => remove_folder(path),
        Ok(_) => remove_file(path),
    }
}

/// Removes a folder and everything in it, read-only files included.
fn remove_folder(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    remove_folder(&entry.path())?;
                } else {
                    remove_file(&entry.path())?;
                }
            }
            fs::remove_dir(path)
        }
    }
}
