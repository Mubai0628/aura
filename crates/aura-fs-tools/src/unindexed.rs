//! Folders the workspace's index did not enter (past its entry cap, or with memory low). A
//! lookup does not reach into them, so a find or a search names them among the skipped scopes and
//! says it is not complete; listing such a folder reads the disk.
use aura_domain_common::WorkspacePath;
use aura_workspace::{PathCatalog, SecurePathResolver};
use aura_workspace_domain::{
    FsSearchCompletion, FsSearchIncompleteReason, FsSkippedScope, FsSkippedScopeReason,
};

use crate::FsToolError;

/// At most this many folders are named.
const NAMED: usize = 50;

pub(crate) fn skipped(
    catalog: &PathCatalog,
    resolver: &SecurePathResolver,
    directory: &WorkspacePath,
    recursive: bool,
) -> Result<Vec<FsSkippedScope>, FsToolError> {
    Ok(catalog
        .unindexed_within(directory, recursive)?
        .into_iter()
        .take(NAMED)
        .map(|path| FsSkippedScope {
            canonical_path: resolver.canonical_root().join(path.as_str()).display().to_string(),
            reason: FsSkippedScopeReason::NotIndexed,
        })
        .collect())
}

pub(crate) fn partial(completion: &mut FsSearchCompletion) {
    match completion {
        FsSearchCompletion::Partial { reasons } => {
            if !reasons.contains(&FsSearchIncompleteReason::ScopeExcluded) {
                reasons.push(FsSearchIncompleteReason::ScopeExcluded);
            }
        }
        FsSearchCompletion::Complete => {
            *completion = FsSearchCompletion::Partial {
                reasons: vec![FsSearchIncompleteReason::ScopeExcluded],
            };
        }
    }
}
