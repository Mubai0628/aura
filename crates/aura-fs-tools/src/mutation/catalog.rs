//! Catalog updates derived from committed mutation paths.
use super::*;

impl FsToolService {
    /// Re-reads only the directories a committed changeset touched.
    ///
    /// The write is already durable by the time this runs, so a failure here is a
    /// recoverable indexing follow-up, never a reason to tell the model the write
    /// did not happen.
    pub(super) fn refresh_catalog_after_mutation(
        &self,
        catalog: &aura_workspace::PathCatalog,
        resolver: &SecurePathResolver,
        prepared: &[PreparedOperation],
        affected_paths: &[WorkspacePath],
        operation_name: &str,
        correlation_id: CorrelationId,
    ) -> Result<aura_domain_common::Revision, FsToolError> {
        let mut paths = affected_paths.to_vec();
        if paths.is_empty() {
            // A changeset whose caller did not list its paths still has to be
            // indexed, so the prepared operations supply the scope.
            for operation in prepared {
                paths.extend(operation.paths());
            }
        }
        let changes = changeset_catalog_changes(prepared, &paths);
        if changes.is_empty() {
            // Every prepared mutation has a bounded path description. An empty
            // changeset is a no-op, not a lost watcher event.
            return Ok(catalog.revision()?);
        }
        Ok(catalog.apply_changes(
            resolver,
            aura_workspace::CatalogUpdate::new(changes, operation_name.to_owned()),
            &catalog.options(),
            correlation_id,
        )?)
    }
}

/// Bounded catalog scope for one committed changeset.
///
/// Every affected path is described exactly: the entry that changed is re-read,
/// a deleted path takes its subtree with it, and nothing else in the workspace is
/// touched. A write therefore re-reads the file it wrote and its parent
/// directory's own metadata, not the directory listing and not any other subtree.
/// The scope is derived from the operation identity, which makes the same
/// committed change produce the same delta no matter how many times it is
/// observed.
/// The bounded catalog description of one committed changeset.
///
/// A deletion is a subtree removal, and only the prepared operation knows that: a
/// deleted path whose parent is merely re-listed loses its own entry but keeps its
/// descendants, because a listing is authoritative only for the directory it read.
/// Everything else is described as a change to the path, which re-reads what holds it
/// and refreshes the parent's own metadata.
pub(super) fn changeset_catalog_changes(
    prepared: &[PreparedOperation],
    affected_paths: &[WorkspacePath],
) -> Vec<(WorkspacePath, aura_workspace::CatalogChange)> {
    use aura_workspace::CatalogChange;
    let gone = prepared
        .iter()
        .filter_map(|operation| match operation {
            PreparedOperation::Delete { target, .. } => Some(target.clone()),
            // A move leaves its source behind, exactly like a delete.
            PreparedOperation::Move { source, .. } => Some(source.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut changes = std::collections::BTreeMap::new();
    for path in affected_paths {
        if gone.contains(path) {
            changes.insert(path.clone(), CatalogChange::Removed);
            // The directory that held it still needs its own metadata refreshed.
            if let Some((parent, _)) = path.as_str().rsplit_once('/')
                && let Ok(directory) = WorkspacePath::parse(parent)
            {
                changes.entry(directory).or_insert(CatalogChange::DirectoryListed);
            }
            continue;
        }
        let change = CatalogChange::Changed;
        changes.insert(path.clone(), change);
        if let Some((parent, _)) = path.as_str().rsplit_once('/')
            && let Ok(directory) = WorkspacePath::parse(parent)
        {
            // The parent's own metadata (a child count or timestamp) is part of
            // the listing, and re-reading it does not descend into anything.
            changes.entry(directory).or_insert(CatalogChange::DirectoryListed);
        }
    }
    changes.into_iter().collect()
}
