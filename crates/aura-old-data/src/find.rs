//! Finding what is left over. Every rule here leaves a folder out when in doubt: an item is only
//! found when everything in it is something Aura wrote and nothing uses.
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use aura_data_location::{
    InstallCopy, LocationRecord, Locations, RECORD_FILE, Roots, verify_location,
};
use aura_data_move::within;
use aura_runtime_contract::{BundleNamespace, PRODUCTION_BUNDLE_NAMESPACE};

use crate::{
    Scope,
    names::{
        CURRENT_LOCAL, CURRENT_ROAMING, EARLY_LOCAL, MOVED, NAMESPACE, SETTINGS, WORKTREES,
        is_leftover, is_namespace_hash, is_one_of,
    },
    walk::{entries, is_empty_folder, measure_all},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// What the versions before namespaces kept at the top of Aura's folders.
    EarlyLayout,
    /// A namespace no installation uses: its folders in the default places and, for data that
    /// was moved, the folder it was moved to.
    Namespace,
    /// Folders whose removal began and did not finish.
    Unfinished,
}

/// One thing to remove, all of it together.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OldData {
    /// `early`, `namespace-<folder name>` or `unfinished`.
    pub id: String,
    pub kind: Kind,
    /// The folders and files it removes.
    pub paths: Vec<PathBuf>,
    pub bytes: u64,
    pub modified: Option<SystemTime>,
}

impl OldData {
    /// The day of the newest change, `2026-09-24`, on a clock `offset` ahead of UTC.
    #[must_use]
    pub fn modified_day(&self, offset: time::UtcOffset) -> Option<String> {
        let date = time::OffsetDateTime::from(self.modified?).checked_to_offset(offset)?.date();
        Some(format!("{}-{:02}-{:02}", date.year(), u8::from(date.month()), date.day()))
    }

    /// The folder name of a namespace's item.
    #[must_use]
    pub fn namespace_hash(&self) -> Option<&str> {
        self.id.strip_prefix("namespace-")
    }
}

/// Everything left over, the early layout first and unfinished removals last.
#[must_use]
pub fn find(scope: &Scope<'_>) -> Vec<OldData> {
    let mut found = Vec::new();
    found.extend(early_layout(scope.roots));
    found.extend(namespaces(scope));
    found.extend(unfinished(scope.roots));
    found
}

fn item(id: String, kind: Kind, paths: Vec<PathBuf>) -> Option<OldData> {
    if paths.is_empty() {
        return None;
    }
    let measured = measure_all(&paths)?;
    // Removing would not follow a link either; a folder holding one is not left for removal.
    if measured.link && kind != Kind::Unfinished {
        return None;
    }
    Some(OldData { id, kind, paths, bytes: measured.bytes, modified: measured.modified })
}

/// The early entries of `folder` when every name in it is Aura's; none otherwise.
fn early_part(folder: &Path, early: &[&str], current: &[&str]) -> Option<Vec<PathBuf>> {
    let listed = entries(folder).ok()?;
    let aura = |name: &str| is_one_of(name, early) || is_one_of(name, current) || is_leftover(name);
    if !listed.iter().all(|(name, _)| aura(name)) {
        return Some(Vec::new());
    }
    let mut paths = Vec::new();
    for (name, path) in listed {
        if !is_one_of(&name, early) {
            continue;
        }
        if name.eq_ignore_ascii_case(WORKTREES) && !is_empty_folder(&path) {
            return Some(Vec::new());
        }
        paths.push(path);
    }
    Some(paths)
}

fn early_layout(roots: &Roots) -> Option<OldData> {
    let mut paths = early_part(&roots.local_aura(), EARLY_LOCAL, CURRENT_LOCAL)?;
    paths.extend(early_part(&roots.roaming_aura(), SETTINGS, CURRENT_ROAMING)?);
    item("early".to_owned(), Kind::EarlyLayout, paths)
}

/// The namespace folder names in `folder`; `None` when it is there and cannot be read.
fn namespace_names(folder: &Path) -> Option<BTreeSet<String>> {
    Some(
        entries(folder)
            .ok()?
            .into_iter()
            .filter(|(name, path)| is_namespace_hash(name) && path.is_dir())
            .map(|(name, _)| name)
            .collect(),
    )
}

fn namespaces(scope: &Scope<'_>) -> Vec<OldData> {
    let [local, roaming] = scope.roots.namespace_folders();
    let (Some(mut hashes), Some(more)) = (namespace_names(&local), namespace_names(&roaming))
    else {
        return Vec::new();
    };
    hashes.extend(more);
    let current = scope.current.object_name_hash();
    let production: BundleNamespace =
        PRODUCTION_BUNDLE_NAMESPACE.parse().expect("the production namespace is valid");
    let production = production.object_name_hash();
    hashes
        .into_iter()
        .filter(|hash| *hash != current && *hash != production)
        .filter(|hash| scope.installed.uses(hash) == Some(false))
        .filter_map(|hash| namespace(scope.roots, &local.join(&hash), &roaming.join(&hash), &hash))
        .collect()
}

/// Whether `folder` holds only `known` names and no working copy; `true` when it is not there.
fn only(folder: &Path, known: &[&str]) -> Option<bool> {
    let listed = entries(folder).ok()?;
    Some(listed.iter().all(|(name, path)| {
        is_one_of(name, known) && (!name.eq_ignore_ascii_case(WORKTREES) || is_empty_folder(path))
    }))
}

fn namespace(roots: &Roots, local: &Path, roaming: &Path, hash: &str) -> Option<OldData> {
    let mut paths = Vec::new();
    if local.exists() {
        if !only(local, NAMESPACE)? {
            return None;
        }
        paths.push(local.to_path_buf());
        if local.join(RECORD_FILE).exists() {
            paths.extend(moved(roots, local, hash)?);
        }
    }
    if roaming.exists() {
        if !only(roaming, SETTINGS)? {
            return None;
        }
        paths.push(roaming.to_path_buf());
    }
    item(format!("namespace-{hash}"), Kind::Namespace, paths)
}

/// For a namespace with a location record: the folder its data is in and the earlier ones it
/// still has to remove, each holding its marker and only what a move puts there; `None` when the
/// record or a folder it names does not say so.
fn moved(roots: &Roots, local: &Path, hash: &str) -> Option<Vec<PathBuf>> {
    let record: LocationRecord =
        serde_json::from_slice(&fs::read(local.join(RECORD_FILE)).ok()?).ok()?;
    if record.namespace.object_name_hash() != hash {
        return None;
    }
    let locations =
        Locations::resolve(roots, &record.namespace, &InstallCopy::NotInstalled).ok()?;
    let mut folders = Vec::new();
    if let Some(location) = &locations.location {
        if !only(location, MOVED)? {
            return None;
        }
        folders.push(location.clone());
    }
    for copy in &locations.pending_removal {
        if within(copy, local) || !copy.exists() {
            continue;
        }
        if verify_location(copy, &record.namespace).is_ok() && only(copy, MOVED)? {
            folders.push(copy.clone());
        }
    }
    Some(folders)
}

fn unfinished(roots: &Roots) -> Option<OldData> {
    let [local, roaming] = roots.namespace_folders();
    let paths = [roots.local_aura(), roots.roaming_aura(), local, roaming]
        .iter()
        .filter_map(|folder| entries(folder).ok())
        .flatten()
        .filter(|(name, _)| is_leftover(name))
        .map(|(_, path)| path)
        .collect();
    item("unfinished".to_owned(), Kind::Unfinished, paths)
}
