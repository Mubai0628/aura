//! Where the index stops: a folder `.gitignore` excludes is recorded as itself, and what lies
//! below it is read from disk when asked for.
use super::artifacts::is_internal_component;
use super::build::CatalogBuildStatus;
use super::delta::CatalogView;
use super::scan::{
    CatalogEntry, EntryFacts, PathCatalog, build_entry, inherited_ignores, is_generated_path,
    push_ignore_matcher, version_of,
};
use crate::error::io_error;
use crate::{SecurePathResolver, WorkspaceError};
use aura_domain_common::{Revision, WorkspacePath};
use aura_workspace_domain::CatalogEntryKind;
use std::collections::BTreeSet;

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
///
/// Only the page's entries are inspected. Naming the folder's children is cheap; inspecting one
/// (its identity, metadata and ignore rules) is the cost, which made a 10,001-file folder take
/// about two seconds to list however few entries were returned. The folder's version is taken
/// from the names and kinds of all its children, so a page cursor still holds exactly while the
/// set of children does.
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
    let absolute = resolver.absolute_path(directory);
    let children = named_children(directory, &absolute)?;
    let facts = children
        .iter()
        .map(|(path, kind)| (path.as_str(), *kind, None, None, None))
        .collect::<Vec<EntryFacts<'_>>>();
    let version = version_of(&facts)?;
    let start = after.map_or(0, |after| children.partition_point(|(path, _)| path <= after));
    let mut ignores = inherited_ignores(resolver, directory)?;
    let pushed = push_ignore_matcher(&absolute, directory.is_root(), &mut ignores)?;
    let mut entries = Vec::new();
    for (path, _) in children.iter().skip(start).take(limit) {
        let name = path.as_str().rsplit('/').next().unwrap_or(path.as_str()).to_owned();
        let child_absolute = absolute.join(&name);
        // The listed folder is the partition, as a one-level listing of it always was.
        let pending = build_entry(
            &ignores,
            directory.clone(),
            directory.clone(),
            path.clone(),
            name,
            &child_absolute,
        )?;
        #[cfg(test)]
        INSPECTED.with(|count| count.set(count.get() + 1));
        entries.push(CatalogEntry::from_record(&pending));
    }
    if pushed {
        ignores.pop();
    }
    Ok(Some((entries, version)))
}

#[cfg(test)]
thread_local! {
    /// The entries `children_beyond_index` inspected on this thread, for the tests that bound it.
    pub(super) static INSPECTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The index entries `PathCatalog::children_after` iterated on this thread.
    pub(super) static VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A folder's children by name, in path order, with the kind the enumeration itself reports.
/// Aura's own staging entries are never listed.
fn named_children(
    directory: &WorkspacePath,
    absolute: &std::path::Path,
) -> Result<Vec<(WorkspacePath, CatalogEntryKind)>, WorkspaceError> {
    let mut children = Vec::new();
    for child in std::fs::read_dir(absolute)
        .map_err(|source| io_error("enumerate workspace directory", absolute, source))?
    {
        let child =
            child.map_err(|source| io_error("read workspace directory entry", absolute, source))?;
        let name = child.file_name().into_string().map_err(|name| {
            WorkspaceError::Watcher(format!(
                "workspace path is not representable in JSON: {}",
                std::path::PathBuf::from(name).display()
            ))
        })?;
        if is_internal_component(&name) {
            continue;
        }
        let path = if directory.is_root() {
            WorkspacePath::parse(&name)
        } else {
            WorkspacePath::parse(format!("{directory}/{name}"))
        }
        .map_err(|error| WorkspaceError::Watcher(format!("unsafe workspace entry: {error}")))?;
        let kind = match child.file_type() {
            Ok(kind) if kind.is_symlink() => CatalogEntryKind::ReparsePoint,
            Ok(kind) if kind.is_dir() => CatalogEntryKind::Directory,
            _ => CatalogEntryKind::File,
        };
        children.push((path, kind));
    }
    children.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(children)
}

impl PathCatalog {
    /// One page of `parent`'s indexed children in path order, after `after`, without the folder's
    /// version (the file tree pages with `after` and needs none). Only the part of the index from
    /// the cursor through the end of `parent`'s subtree is walked.
    pub fn children_after(
        &self,
        parent: &WorkspacePath,
        after: Option<&WorkspacePath>,
        limit: usize,
    ) -> Result<(Vec<CatalogEntry>, Revision), WorkspaceError> {
        use std::ops::Bound::{Excluded, Included, Unbounded};
        let state = self.state.read().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
        // Below the root, a folder's subtree is exactly the index range from `dir/` up to `dir0`,
        // '0' being the character after '/': a sibling such as `src.bak` sorts before it and
        // `src2` after it, and an empty folder's range is empty. The root's children are spread
        // over the whole index, which is walked from its start.
        let (start, end) = if parent.is_root() {
            (None, None)
        } else {
            (Some(format!("{parent}/")), Some(format!("{parent}0")))
        };
        let lower = match (after, &start) {
            (Some(after), _) => Excluded(after.as_str()),
            (None, Some(start)) => Included(start.as_str()),
            (None, None) => Unbounded,
        };
        let upper = end.as_deref().map_or(Unbounded, Excluded);
        let mut entries = Vec::new();
        for (_, entry) in state.entries.range::<str, _>((lower, upper)) {
            #[cfg(test)]
            VISITED.with(|count| count.set(count.get() + 1));
            if &entry.parent_path == parent {
                entries.push(entry.clone());
                if entries.len() == limit {
                    break;
                }
            }
        }
        Ok((entries, state.revision))
    }
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
