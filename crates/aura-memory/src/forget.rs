//! What Aura keeps for a removed Workspace's project memory — the Vault's index and the
//! records beside it, in Aura's own cache directory. The Vault itself (`.memories` inside the
//! Workspace) belongs to the user and is never touched.
use std::{io::ErrorKind, path::Path};

use crate::{MemoryResult, MemoryService, VaultRoots};

impl MemoryService {
    /// Closes the project Vault of `workspace_root` if it is open, then deletes its index and
    /// records. Returns whether anything was there to delete.
    pub fn forget_project(&self, workspace_root: &Path) -> MemoryResult<bool> {
        let root = VaultRoots::project(workspace_root);
        let root = crate::roots::normalize_root(&root).unwrap_or(root);
        let key = crate::service::cache_key(&root);
        // The watcher and the index connection hold the files open.
        drop(self.live.lock().unwrap_or_else(|e| e.into_inner()).remove(&key));
        let cache = &self.roots().cache;
        let mut removed = false;
        for name in [format!("{key}.db"), format!("{key}.db-wal"), format!("{key}.db-shm")] {
            match std::fs::remove_file(cache.join(name)) {
                Ok(()) => removed = true,
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        match std::fs::remove_dir_all(cache.join(format!("{key}.records"))) {
            Ok(()) => removed = true,
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(removed)
    }
}
