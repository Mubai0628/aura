//! Where the index stops: a folder `.gitignore` excludes is recorded as itself, and what lies
//! below it is read from disk when asked for.
use super::build::CatalogBuildStatus;
use super::delta::CatalogView;
use super::scan::{
    CatalogEntry, PathCatalog, inherited_ignores, is_generated_path, list_directory_once,
    listing_version,
};
use crate::{SecurePathResolver, WorkspaceError};
use aura_domain_common::{Revision, WorkspacePath};
use aura_workspace_domain::CatalogEntryKind;
use std::collections::{BTreeMap, BTreeSet};

/// Whether a folder above `path` (not `path` itself, not the root) is one the index records
/// as ignored: the index does not reach below such a folder.
pub(super) fn beyond_index(state: &CatalogView, path: &WorkspacePath) -> bool {
    let mut ancestor = path.as_str();
    while let Some((parent, _)) = ancestor.rsplit_once('/') {
        ancestor = parent;
        if WorkspacePath::parse(parent).is_ok_and(|parent| ignored_directory(state, &parent)) {
            return true;
        }
    }
    false
}

/// Whether the index records `path` as a folder `.gitignore` excludes.
pub(super) fn ignored_directory(state: &CatalogView, path: &WorkspacePath) -> bool {
    state
        .entries
        .get(path)
        .is_some_and(|entry| entry.kind == CatalogEntryKind::Directory && entry.ignored)
}

/// The children of `directory` read from disk, one level and one page, when the index stops
/// above them (an ignored folder, or a folder below one); `None` when the index holds them.
/// Nothing is written to the index and nothing is watched.
pub fn children_beyond_index(
    catalog: &PathCatalog,
    resolver: &SecurePathResolver,
    directory: &WorkspacePath,
    after: Option<&WorkspacePath>,
    limit: usize,
) -> Result<Option<(Vec<CatalogEntry>, Revision)>, WorkspaceError> {
    // While a build walk runs, or below a folder the walk did not enter, the disk answers.
    let unbuilt = catalog.build.is_building()? || catalog.build.unindexed(directory)?;
    if !unbuilt && (directory.is_root() || !ignored_or_below(catalog, directory)?) {
        return Ok(None);
    }
    let resolved = resolver.resolve_existing(directory)?;
    if !resolved.info.is_directory || resolved.info.is_reparse_point {
        return Err(WorkspaceError::NotFound { path: directory.clone() });
    }
    let mut ignores = inherited_ignores(resolver, directory)?;
    let (mut pending, mut stats, mut watched) = (Vec::new(), BTreeMap::new(), Vec::new());
    list_directory_once(
        resolver,
        directory,
        directory,
        &mut ignores,
        &mut pending,
        &mut stats,
        &mut watched,
    )?;
    let mut entries = pending.iter().map(CatalogEntry::from_record).collect::<Vec<_>>();
    // The whole folder's version, so a page cursor holds while the folder's children do.
    let version = listing_version(&entries)?;
    entries.retain(|entry| after.is_none_or(|after| &entry.path > after));
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries.truncate(limit);
    Ok(Some((entries, version)))
}

impl PathCatalog {
    /// Where the index build stands and how far it got.
    pub fn build_status(&self) -> Result<CatalogBuildStatus, WorkspaceError> {
        self.build.status()
    }

    /// The folders the index did not enter that a lookup of `directory` would have reached.
    pub fn unindexed_within(
        &self,
        directory: &WorkspacePath,
        recursive: bool,
    ) -> Result<Vec<WorkspacePath>, WorkspaceError> {
        self.build.unindexed_within(directory, recursive)
    }

    /// Waits up to `limit` for a running build; an error says how far it got.
    pub fn wait_until_built(&self, limit: std::time::Duration) -> Result<(), WorkspaceError> {
        self.build.wait(limit)
    }

    /// Waits up to `limit` for any whole walk of the index to end, including the check of a
    /// saved index that queries do not wait for.
    pub fn wait_until_walked(&self, limit: std::time::Duration) -> Result<(), WorkspaceError> {
        self.build.wait_walked(limit)
    }

    /// What the last finished build walk found, compared with the index it started from.
    pub fn last_build_report(&self) -> Option<super::CatalogReconcileReport> {
        self.build.last_report()
    }

    /// The ignored and generated flags of a path the index does not hold because a folder above
    /// it is ignored: such a path is ignored, and generated as its own components say. A path the
    /// index simply does not know is neither.
    pub fn flags_beyond_index(&self, path: &WorkspacePath) -> Result<(bool, bool), WorkspaceError> {
        let beyond = match path.as_str().rsplit_once('/') {
            Some((parent, _)) => WorkspacePath::parse(parent)
                .ok()
                .map(|parent| ignored_or_below(self, &parent))
                .transpose()?
                .unwrap_or(false),
            None => false,
        };
        Ok((beyond, beyond && is_generated_path(path)))
    }
}

/// Whether the index records `directory` or a folder above it as ignored.
fn ignored_or_below(
    catalog: &PathCatalog,
    directory: &WorkspacePath,
) -> Result<bool, WorkspaceError> {
    let mut current = Some(directory.as_str());
    while let Some(path) = current {
        let ignored =
            WorkspacePath::parse(path).ok().map(|path| catalog.entry(&path)).transpose()?;
        if ignored
            .flatten()
            .is_some_and(|entry| entry.kind == CatalogEntryKind::Directory && entry.ignored)
        {
            return Ok(true);
        }
        current = path.rsplit_once('/').map(|(parent, _)| parent);
    }
    Ok(false)
}

pub(super) fn is_catalogued_directory(state: &CatalogView, path: &WorkspacePath) -> bool {
    state.entries.get(path).is_some_and(|entry| entry.kind == CatalogEntryKind::Directory)
}

pub(super) fn remove_subtree(
    state: &CatalogView,
    path: &WorkspacePath,
    removed: &mut BTreeSet<WorkspacePath>,
) {
    let prefix = format!("{path}/");
    let mut found = false;
    for entry in state.entries.keys() {
        if entry == path || entry.as_str().starts_with(&prefix) {
            removed.insert(entry.clone());
            found = true;
        }
    }
    if !found {
        // A path the catalog never recorded still has to leave the durable store
        // so a late delta cannot leave a removed entry behind.
        removed.insert(path.clone());
    }
}

/// Whether a path a change named still exists.
///
/// This is the bounded check that separates a real removal from an observation that
/// has been overtaken. A path that cannot be opened for any reason counts as absent,
/// matching how the other change descriptions treat a path they cannot resolve: the
/// change is then honoured as described rather than silently ignored.
pub(super) fn path_exists(resolver: &SecurePathResolver, path: &WorkspacePath) -> bool {
    resolver.open_existing(path, aura_windows_platform::NativeFileAccess::Metadata).is_ok()
}

pub(super) fn parent_of(path: &WorkspacePath) -> Result<WorkspacePath, WorkspaceError> {
    let mut components = path.as_str().split('/').collect::<Vec<_>>();
    components.pop();
    if components.is_empty() {
        return WorkspacePath::parse(WorkspacePath::ROOT)
            .map_err(|error| WorkspaceError::Watcher(format!("unsafe workspace entry: {error}")));
    }
    WorkspacePath::parse(components.join("/"))
        .map_err(|error| WorkspaceError::Watcher(format!("unsafe workspace entry: {error}")))
}
