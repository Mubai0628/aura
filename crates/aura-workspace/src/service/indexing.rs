//! The workspaces' index builds as the runtime reports them.
use super::*;

impl WorkspaceRuntime {
    /// Walks a workspace's folder again after its index failed to build: the index counts as
    /// building at once, and the walk runs on the workspace's watcher thread. Answers false, and
    /// starts nothing, while a walk is already running.
    pub fn rebuild_index(&self, workspace_id: WorkspaceId) -> Result<bool, WorkspaceError> {
        let catalog = self.catalog(workspace_id)?;
        let watchers =
            self.watchers.lock().map_err(|_| WorkspaceError::LockPoisoned("workspace watchers"))?;
        let watcher = watchers.get(&workspace_id).ok_or(WorkspaceError::CatalogUnavailable)?;
        if !catalog.build.begin_requested()? {
            return Ok(false);
        }
        watcher.request_walk();
        drop(watchers);
        let _ = self.projection_signals.bump();
        Ok(true)
    }

    /// Waits up to `limit` for every workspace's running index walk (a first build, or the
    /// check of a saved index); the first one still running then answers how far it got.
    pub fn wait_until_indexed(&self, limit: Duration) -> Result<(), WorkspaceError> {
        let deadline = Instant::now() + limit;
        let catalogs = self
            .catalogs
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace catalogs"))?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for catalog in catalogs {
            catalog.wait_until_walked(deadline.saturating_duration_since(Instant::now()))?;
        }
        Ok(())
    }

    /// The open's own costs, with what each workspace's first walk found once it finished: the
    /// walks run after the open returns.
    pub(super) fn with_walk_reports(&self, mut report: WorkspaceOpenReport) -> WorkspaceOpenReport {
        let Ok(catalogs) = self.catalogs.read() else { return report };
        for walk in catalogs.values().filter_map(|catalog| catalog.last_build_report()) {
            report.catalog_reused += walk.reused;
            report.catalog_rehydrated += walk.rehydrated;
            report.catalog_unproven_volume += walk.unproven_volume;
            report.catalog_removed += walk.removed;
            report.catalog_metadata_changed += walk.metadata_changed;
        }
        report
    }
}

/// How fresh a workspace's index is and what its build is doing: a first build in progress is
/// rebuilding, one that could not cover the whole tree is stale, one that failed is unavailable;
/// a finished build leaves the stored freshness.
pub(super) fn catalog_indexing(
    catalog: &PathCatalog,
    stored: CatalogFreshness,
) -> Result<
    (CatalogFreshness, Option<aura_workspace_domain::WorkspaceIndexingStatus>),
    WorkspaceError,
> {
    use crate::CatalogBuildPhase;
    use aura_workspace_domain::{WorkspaceIndexingState, WorkspaceIndexingStatus};
    let status = catalog.build_status()?;
    let (freshness, state, failure) = match status.phase {
        CatalogBuildPhase::Ready => return Ok((stored, None)),
        CatalogBuildPhase::Building => {
            (CatalogFreshness::Rebuilding, WorkspaceIndexingState::Building, None)
        }
        CatalogBuildPhase::Incomplete => {
            (CatalogFreshness::Stale, WorkspaceIndexingState::Incomplete, None)
        }
        CatalogBuildPhase::Failed { reason } => {
            (CatalogFreshness::Unavailable, WorkspaceIndexingState::Failed, Some(reason))
        }
    };
    Ok((
        freshness,
        Some(WorkspaceIndexingStatus {
            state,
            listed_entries: status.listed_entries,
            unindexed_directories: status.unindexed_directories,
            failure,
        }),
    ))
}
