//! A workspace's place in the runtime while it is online: installing it, leaving out one that
//! cannot be opened, and removing it again.
use super::*;

impl WorkspaceRuntime {
    /// Installs a workspace found online when the runtime opens. One that cannot be opened is left
    /// out and reported on its own, so every other workspace still opens.
    pub(super) fn install_at_open(&self, workspace: WorkspaceSummary) -> Option<InstallMs> {
        let workspace_id = workspace.workspace_id;
        match self.install_online_workspace(workspace, CorrelationId::new()) {
            Ok(install) => Some(install),
            Err(error) => {
                let _ = self.remove_online_runtime_state(workspace_id);
                crate::diagnostics::append_workspace_log(
                    self.persistence.database_path(),
                    "workspace_open_failed",
                    serde_json::json!({
                        "workspace_id": workspace_id.to_string(),
                        "reason": error.to_string(),
                    }),
                );
                None
            }
        }
    }

    pub(super) fn install_online_workspace(
        &self,
        workspace: WorkspaceSummary,
        correlation_id: CorrelationId,
    ) -> Result<InstallMs, WorkspaceError> {
        let resolver = match SecurePathResolver::from_workspace(&workspace) {
            Ok(resolver) => Arc::new(resolver),
            Err(_error) => {
                let mut offline = workspace;
                offline.state = WorkspaceState::Offline;
                offline.catalog_freshness = CatalogFreshness::Unavailable;
                offline.revision = increment(offline.revision)?;
                self.persistence.update_workspace(
                    offline,
                    "workspace.offline",
                    correlation_id,
                    false,
                )?;
                return Ok(InstallMs::default());
            }
        };
        let catalog = Arc::new(PathCatalog::open(
            workspace.workspace_id,
            Arc::clone(&self.persistence),
            self.options.catalog.clone(),
        )?);
        // The walk runs on the workspace's own thread (`watching::first_walk`): a fresh
        // workspace is usable at once, and a saved index answers while it is checked.
        catalog.build.begin(catalog.open_report().entries == 0)?;
        self.resolvers
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace resolvers"))?
            .insert(workspace.workspace_id, Arc::clone(&resolver));
        let report = catalog.open_report();
        self.catalogs
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace catalogs"))?
            .insert(workspace.workspace_id, Arc::clone(&catalog));
        let started = Instant::now();
        let watcher = WorkspaceWatchHandle::spawn(
            workspace.workspace_id,
            resolver,
            catalog,
            Arc::clone(&self.persistence),
            self.projection_signals.clone(),
            self.options.watcher_coalesce,
            self.options.watcher_poll_interval,
            self.options.watcher_rescan_attempts,
            self.options.watcher_batch_hook.clone(),
        )?;
        self.watchers
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace watchers"))?
            .insert(workspace.workspace_id, watcher);
        Ok(InstallMs {
            catalog: report,
            rebuild_ms: 0,
            watcher_spawn_ms: elapsed_ms(started),
            reconcile: Default::default(),
        })
    }

    /// Removes archived workspaces' stored index rows on a thread of its own, in jobs on the
    /// registry's index lane, so other registry writes run between them.
    pub(super) fn clear_archived_index_in_background(&self, workspaces: Vec<WorkspaceId>) {
        if workspaces.is_empty() {
            return;
        }
        let persistence = Arc::clone(&self.persistence);
        let _ = thread::Builder::new().name("aura-workspace-archived-index".to_owned()).spawn(
            move || {
                for workspace_id in workspaces {
                    let _ = persistence.clear_archived_workspace_index(workspace_id);
                }
            },
        );
    }

    pub(super) fn remove_online_runtime_state(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<(), WorkspaceError> {
        let watcher = self
            .watchers
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace watchers"))?
            .remove(&workspace_id);
        // Stopped once the lock is released: workspace projections read watcher states under it.
        drop(watcher);
        self.catalogs
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace catalogs"))?
            .remove(&workspace_id);
        self.resolvers
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace resolvers"))?
            .remove(&workspace_id);
        Ok(())
    }
}
