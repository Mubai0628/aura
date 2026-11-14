//! A worktree's folder as a workspace of its own. It is listed under the workspace it was made
//! from, resolved and watched like any workspace, so a path in it is judged by its final path,
//! its handle and the folder's identity. Once its folder is to go it is archived: the watcher
//! stops and its index is cleared, and the record stays for the conversations that worked in it.
use super::*;

impl WorkspaceRuntime {
    /// Registers a worktree's folder as a workspace listed under `parent`: linked before it is
    /// created, so no publication lists it on its own. The same folder again answers with it.
    pub fn create_worktree(
        &self,
        command: WorkspaceCreateCommand,
        parent: WorkspaceId,
        correlation_id: CorrelationId,
    ) -> Result<WorkspaceSummary, WorkspaceError> {
        if command.display_name.trim().is_empty() {
            return Err(WorkspaceError::InvalidRoot);
        }
        let selected = PathBuf::from(&command.selected_root);
        let root = SecurePathResolver::inspect_selected_root(&selected)?;
        let identity = domain_identity(&root.identity);
        let canonical_root = root.final_path.display().to_string();
        if let Some(existing) = self.persistence.find_workspace_by_root(
            &canonical_root,
            Some(&identity.volume_serial_number),
            Some(&identity.file_id),
        )? {
            self.persistence.link_worktree(existing.workspace_id, parent)?;
            if existing.state == WorkspaceState::Archived {
                return self.restore(existing.workspace_id, correlation_id);
            }
            return Ok(existing);
        }
        let workspace = WorkspaceSummary {
            workspace_id: WorkspaceId::new(),
            display_name: command.display_name,
            selected_root: selected.display().to_string(),
            canonical_root,
            root_identity: Some(identity),
            state: WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        };
        self.persistence.link_worktree(workspace.workspace_id, parent)?;
        self.persistence.create_workspace(workspace.clone(), correlation_id)?;
        self.install_online_workspace(workspace.clone(), correlation_id)?;
        self.notify_projection_change()?;
        Ok(self.persistence.workspace(workspace.workspace_id)?)
    }

    /// Takes a worktree's workspace out of use before its folder is removed: its watcher stops,
    /// so nothing holds the folder, and it is archived with its index cleared. Archiving it
    /// again changes nothing.
    pub fn retire_worktree(
        &self,
        workspace_id: WorkspaceId,
        correlation_id: CorrelationId,
    ) -> Result<(), WorkspaceError> {
        let mut workspace = match self.persistence.workspace(workspace_id) {
            Ok(workspace) => workspace,
            // Never registered, or already removed with everything it held.
            Err(aura_persistence::PersistenceError::WorkspaceNotFound) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if workspace.state == WorkspaceState::Archived {
            return Ok(());
        }
        // Its thread is waited for: the folder is removed right after this returns.
        let watcher = self
            .watchers
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace watchers"))?
            .remove(&workspace_id);
        if let Some(watcher) = watcher {
            watcher.signal_stop();
            watcher.stop_and_join();
        }
        self.remove_online_runtime_state(workspace_id)?;
        workspace.state = WorkspaceState::Archived;
        workspace.catalog_freshness = CatalogFreshness::Unavailable;
        workspace.revision = increment(workspace.revision)?;
        workspace.authorization_epoch = increment(workspace.authorization_epoch)?;
        self.persistence.update_workspace(workspace, "workspace.archived", correlation_id, true)?;
        self.clear_archived_index_in_background(vec![workspace_id]);
        self.notify_projection_change()?;
        Ok(())
    }
}
