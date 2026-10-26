//! Bounded descriptions of native watcher events.
use super::artifacts::is_internal_path;
use super::delta::{CatalogChange, is_within};
use super::scan::PathCatalog;
use crate::SecurePathResolver;
use aura_domain_common::WorkspacePath;
use aura_workspace_domain::CatalogEntryKind;
use std::collections::BTreeMap;

/// What a batch of watcher events means for the index.
pub(crate) enum WatchDescription {
    /// Every event in the batch is accounted for. The list holds the bounded
    /// changes it implies; an empty list means the batch described nothing the
    /// index contains — Aura's own staging and temp paths, an access observation,
    /// or a path this workspace does not own.
    Described(Vec<(WorkspacePath, CatalogChange)>),
    /// A mutation named a path that could not be described, so the only safe
    /// recovery is a full rebuild rather than a silent drop.
    Undescribed,
}

/// Maps raw watcher events onto workspace-relative catalog changes.
///
/// Each event is described as narrowly as the event itself allows: a named file
/// is re-read exactly, a removed path takes its subtree with it, and only a
/// directory that really is new or replaced is re-read as a subtree. A directory
/// whose contents changed without the event naming the child is re-listed one
/// level deep, which is the bounded answer for that case. An event that names
/// nothing inside the root is skipped rather than widened.
pub(crate) fn changes_for_watch_events(
    resolver: &SecurePathResolver,
    catalog: &PathCatalog,
    events: &[aura_windows_platform::FileWatchEvent],
) -> WatchDescription {
    use aura_windows_platform::FileWatchEventKind;
    let root = aura_windows_platform::extended_length_path(resolver.canonical_root());
    let mut changes = BTreeMap::new();
    let mut undescribed = false;
    for event in events {
        if !matches!(
            event.kind,
            FileWatchEventKind::Create
                | FileWatchEventKind::Modify
                | FileWatchEventKind::Remove
                | FileWatchEventKind::RenamePair
                // An unclassified mutation still names its paths, so the exact
                // path can be re-read instead of rescanning the whole workspace.
                // An access observation is not a change and is deliberately absent
                // from this list.
                | FileWatchEventKind::UnknownMutation
        ) {
            continue;
        }
        for path in &event.paths {
            // Watcher paths and handle-derived roots use different transport spellings.
            // Normalize only that spelling; preserve each native component name.
            let native_path = aura_windows_platform::extended_length_path(path);
            let Ok(relative) = native_path.strip_prefix(&root) else {
                // A mutation the watcher delivered for a path outside the root
                // cannot be described locally.
                undescribed = true;
                continue;
            };
            let text = relative.to_string_lossy().replace('\\', "/");
            let Ok(relative) = WorkspacePath::parse(text) else {
                undescribed = true;
                continue;
            };
            if is_internal_path(&relative) {
                // Aura's own staging and temp entries are not user content and are
                // never indexed, so an event about one describes nothing.
                continue;
            }
            if changes_ignore_rules(&relative) {
                // Which folders the index enters may have changed anywhere below this one,
                // which only a whole rebuild can redraw.
                undescribed = true;
                continue;
            }
            if relative.is_root() {
                changes.insert(relative, CatalogChange::DirectoryListed);
                continue;
            }
            let known_directory = catalog
                .entry(&relative)
                .ok()
                .flatten()
                .is_some_and(|entry| entry.kind == CatalogEntryKind::Directory);
            let change = match event.kind {
                // A removed path takes its subtree with it.
                FileWatchEventKind::Remove => CatalogChange::Removed,
                _ if known_directory && path.is_dir() => CatalogChange::DirectoryListed,
                _ if known_directory => CatalogChange::Changed,
                // A path the catalog has never seen. A directory new to the index
                // has no described children yet, so its subtree is the scope; a
                // file is still just a file.
                _ if path.is_dir() => CatalogChange::DirectoryRescanned,
                _ => CatalogChange::Changed,
            };
            // A wider scope already covers a narrower one, so an event inside a
            // scope that is being re-read adds nothing.
            let covered = changes.iter().any(|(existing, existing_change)| {
                existing == &relative
                    || (existing_change != &CatalogChange::Changed
                        && is_within(&relative, existing))
            });
            if covered {
                continue;
            }
            changes.insert(relative, change);
        }
    }
    if undescribed {
        return WatchDescription::Undescribed;
    }
    WatchDescription::Described(changes.into_iter().collect())
}

/// A `.gitignore` in any folder, or the root's `.git/info/exclude`: the files that decide what
/// the index skips.
fn changes_ignore_rules(path: &WorkspacePath) -> bool {
    path.as_str().eq_ignore_ascii_case(".git/info/exclude")
        || path
            .as_str()
            .rsplit('/')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case(".gitignore"))
}
