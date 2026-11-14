//! Removing what the user picked. Each item's folders are first renamed beside themselves, all of
//! them or none: Windows refuses to rename a folder while a file inside is open or a program works
//! in it, so a refusal means the item is in use and nothing of it goes. Once renamed, nothing that
//! looks for the old names finds them, and they are deleted.
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    Scope,
    find::{Kind, find},
    names::REMOVING_SUFFIX,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Removed {
    pub removed: Vec<String>,
    /// Kept whole: a file in it is open, or a program works in it.
    pub in_use: Vec<String>,
    /// Why an item was not removed, or not completely.
    pub failed: Vec<(String, String)>,
}

/// Removes the items `ids` names that are still found; an id no longer found is left alone.
#[must_use]
pub fn remove(scope: &Scope<'_>, ids: &[String]) -> Removed {
    let mut removed = Removed::default();
    for item in find(scope).into_iter().filter(|item| ids.contains(&item.id)) {
        let outcome = match item.kind {
            Kind::Unfinished => delete_each(&item.paths),
            Kind::EarlyLayout | Kind::Namespace => match set_aside(&item.paths) {
                Ok(aside) => delete_each(&aside),
                Err(error) if in_use(&error) => {
                    removed.in_use.push(item.id);
                    continue;
                }
                Err(error) => Err(error.to_string()),
            },
        };
        match outcome {
            Ok(()) => removed.removed.push(item.id),
            Err(reason) => removed.failed.push((item.id, reason)),
        }
    }
    removed
}

/// A sharing violation, or the access refused while something inside is open.
fn in_use(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32))
}

fn aside_name(path: &Path, stamp: u128) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy();
    Some(path.with_file_name(format!("{name}.{stamp}{REMOVING_SUFFIX}")))
}

/// Renames every path beside itself; when one cannot be renamed, puts back the ones that were.
fn set_aside(paths: &[PathBuf]) -> io::Result<Vec<PathBuf>> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis());
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for path in paths {
        let renamed = aside_name(path, stamp)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a path without a name"))
            .and_then(|aside| fs::rename(path, &aside).map(|()| aside));
        match renamed {
            Ok(aside) => done.push((path.clone(), aside)),
            Err(error) => {
                for (path, aside) in done.iter().rev() {
                    let _ = fs::rename(aside, path);
                }
                return Err(error);
            }
        }
    }
    Ok(done.into_iter().map(|(_, aside)| aside).collect())
}

fn delete_each(paths: &[PathBuf]) -> Result<(), String> {
    let mut failures = Vec::new();
    for path in paths {
        if let Err(error) = delete(path) {
            failures.push(format!("{}: {error}", aura_domain_common::display_path(path)));
        }
    }
    if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
}

/// Removes a file or a folder with everything in it, read-only files included; a link inside is
/// removed itself, never what it points at.
fn delete(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        // A link to a folder is removed as a folder, a link to a file as a file.
        return fs::remove_dir(path).or_else(|_| fs::remove_file(path));
    }
    if !metadata.is_dir() {
        return remove_entry(path, &metadata);
    }
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => {
            for entry in fs::read_dir(path)? {
                delete(&entry?.path())?;
            }
            fs::remove_dir(path)
        }
    }
}

fn remove_entry(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    if metadata.permissions().readonly() {
        let mut permissions = metadata.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    fs::remove_file(path)
}
