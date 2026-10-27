//! The folders the index did not enter (past its entry cap, or with memory low), read from disk
//! for one lookup: breadth first, within a budget, entering no folder the index would not enter
//! (an ignored one, a link), and writing nothing to the index.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use aura_domain_common::WorkspacePath;
use aura_workspace_domain::CatalogEntryKind;

use super::boundary::children_beyond_index;
use super::scan::{CatalogEntry, NameMatchMode, NamePattern, PathCatalog};
use crate::{SecurePathResolver, WorkspaceError};

/// Entries one lookup reads from disk at most, and for how long.
const MAX_ENTRIES: usize = 20_000;
const MAX_TIME: Duration = Duration::from_secs(2);

/// What a lookup read below the index, and the folders it had no budget left for.
#[derive(Debug, Default)]
pub struct UnindexedListing {
    pub entries: Vec<CatalogEntry>,
    /// Folders not read, or not read to the end: what they hold is missing from `entries`.
    pub unfinished: Vec<WorkspacePath>,
}

impl PathCatalog {
    /// What a lookup of `directory` would have found in the folders the index did not enter:
    /// below them when `recursive`, or `directory`'s own children when it is one of them.
    pub fn read_unindexed(
        &self,
        resolver: &SecurePathResolver,
        directory: &WorkspacePath,
        recursive: bool,
    ) -> Result<UnindexedListing, WorkspaceError> {
        self.read_unindexed_within(resolver, directory, recursive, MAX_ENTRIES)
    }

    pub(crate) fn read_unindexed_within(
        &self,
        resolver: &SecurePathResolver,
        directory: &WorkspacePath,
        recursive: bool,
        budget: usize,
    ) -> Result<UnindexedListing, WorkspaceError> {
        let mut queue = if self.build.unindexed(directory)? {
            VecDeque::from([directory.clone()])
        } else if recursive {
            VecDeque::from(self.build.unindexed_within(directory, true)?)
        } else {
            // A one-level lookup of an indexed folder holds its unindexed folders as entries.
            VecDeque::new()
        };
        let mut listing = UnindexedListing::default();
        let started = Instant::now();
        while let Some(folder) = queue.pop_front() {
            let room = budget.saturating_sub(listing.entries.len());
            if room == 0 || started.elapsed() >= MAX_TIME {
                listing.unfinished.push(folder);
                listing.unfinished.extend(queue);
                break;
            }
            let children = match children_beyond_index(self, resolver, &folder, None, room + 1) {
                Ok(Some((children, _))) => children,
                // The index holds the folder after all, or it is gone: nothing is missing.
                Ok(None) | Err(WorkspaceError::NotFound { .. }) => continue,
                Err(_) => {
                    listing.unfinished.push(folder);
                    continue;
                }
            };
            if children.len() > room {
                listing.unfinished.push(folder);
            }
            for child in children.into_iter().take(room) {
                if recursive && child.kind == CatalogEntryKind::Directory && !child.ignored {
                    queue.push_back(child.path.clone());
                }
                listing.entries.push(child);
            }
        }
        Ok(listing)
    }

    /// The files named `file_names` in the folders a lookup of `directory` reaches that the
    /// index did not enter, in path order, and the folders the read did not finish.
    pub fn find_names_unindexed(
        &self,
        resolver: &SecurePathResolver,
        directory: &WorkspacePath,
        file_names: &[String],
        recursive: bool,
        match_mode: NameMatchMode,
        case_sensitive: bool,
    ) -> Result<(Vec<CatalogEntry>, Vec<WorkspacePath>), WorkspaceError> {
        let patterns = file_names
            .iter()
            .map(|name| NamePattern::compile(name, match_mode, case_sensitive))
            .collect::<Result<Vec<_>, _>>()?;
        let listing = self.read_unindexed(resolver, directory, recursive)?;
        let mut matches = listing
            .entries
            .into_iter()
            .filter(|entry| {
                entry.kind == CatalogEntryKind::File
                    && patterns.iter().any(|pattern| pattern.matches(&entry.file_name))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| left.path.cmp(&right.path));
        Ok((matches, listing.unfinished))
    }
}

#[cfg(any(test, feature = "test-hooks"))]
impl PathCatalog {
    /// Caps every later walk of this index at `entries`, as an index of a huge workspace is.
    pub fn cap_walks_for_test(&self, entries: u64) {
        self.build.progress().cap_at(entries);
    }
}
