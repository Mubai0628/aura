//! Git's own folder at the workspace root. Its files are indexed, so a lookup inside it works,
//! but a search or a name lookup of the workspace does not offer them among the user's files.
use aura_domain_common::WorkspacePath;
use aura_workspace_domain::CatalogEntryKind;

use super::scan::CatalogEntry;

/// Whether `path` is the root's `.git` folder or lies inside it.
pub(crate) fn inside(path: &WorkspacePath) -> bool {
    path.as_str().split('/').next().is_some_and(|component| component.eq_ignore_ascii_case(".git"))
}

/// Whether a name lookup of `directory` may answer with `entry`: a file, and one of Git's own
/// only when the lookup is itself inside `.git`.
pub(crate) fn findable(entry: &CatalogEntry, directory: &WorkspacePath) -> bool {
    entry.kind == CatalogEntryKind::File && (!entry.hidden_from_search || inside(directory))
}
