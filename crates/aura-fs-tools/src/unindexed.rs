//! Folders the workspace's index did not enter (past its entry cap, or with memory low). A lookup
//! reads them from disk within a budget; the folders that budget did not reach are named among the
//! skipped scopes and the lookup says it is not complete. Listing such a folder reads the disk.
use aura_domain_common::WorkspacePath;
use aura_workspace::SecurePathResolver;
use aura_workspace_domain::{
    FsSearchCompletion, FsSearchIncompleteReason, FsSkippedScope, FsSkippedScopeReason,
};

/// At most this many folders are named.
const NAMED: usize = 50;

pub(crate) fn skipped(
    resolver: &SecurePathResolver,
    folders: &[WorkspacePath],
) -> Vec<FsSkippedScope> {
    folders
        .iter()
        .take(NAMED)
        .map(|path| FsSkippedScope {
            canonical_path: resolver.canonical_root().join(path.as_str()).display().to_string(),
            reason: FsSkippedScopeReason::NotIndexed,
        })
        .collect()
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
