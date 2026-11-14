//! Bytes on disk of worktree folders. Counting one walks every file in it, build outputs
//! included, so it runs in the background and the last count is shown until the next one ends.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

/// At most this many folders' counts are kept; past it they are counted again when next asked.
const SIZES_MAX: usize = 256;

#[derive(Default)]
pub struct FolderSizes {
    entries: Mutex<HashMap<PathBuf, Entry>>,
}

#[derive(Clone, Copy, Default)]
struct Entry {
    bytes: Option<u64>,
    counted_at: Option<Instant>,
    counting: bool,
}

impl FolderSizes {
    /// The last count of `folder`, and whether a new count is to start now: none is running and
    /// the last is older than `fresh`. A count that starts must end with [`Self::counted`].
    #[must_use]
    pub fn take(&self, folder: &Path, fresh: Duration) -> (Option<u64>, bool) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() >= SIZES_MAX && !entries.contains_key(folder) {
            entries.clear();
        }
        let entry = entries.entry(folder.to_path_buf()).or_default();
        let due = !entry.counting && entry.counted_at.is_none_or(|at| at.elapsed() >= fresh);
        if due {
            entry.counting = true;
        }
        (entry.bytes, due)
    }

    /// Ends a count: its bytes, or none when the folder could not be read.
    pub fn counted(&self, folder: &Path, bytes: Option<u64>) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = entries.entry(folder.to_path_buf()).or_default();
        *entry = Entry { bytes, counted_at: Some(Instant::now()), counting: false };
    }

    /// Forgets a folder that was removed.
    pub fn forget(&self, folder: &Path) {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).remove(folder);
    }
}
