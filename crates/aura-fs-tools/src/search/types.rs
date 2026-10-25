use aura_domain_common::{ContentHash, Revision, SessionId, WorkspaceId, WorkspacePath};
use aura_workspace_domain::{
    FileStorageSource, FsSearchCompletion, FsSearchIncompleteReason, FsSearchTextMatch,
};

#[derive(Clone, Debug)]
pub(crate) struct FileOutcome {
    pub(crate) ordinal: usize,
    #[allow(dead_code)]
    pub(crate) path: WorkspacePath,
    pub(crate) matches: Vec<FsSearchTextMatch>,
    pub(crate) scanned: bool,
    pub(crate) skipped: bool,
    pub(crate) bytes: u64,
    pub(crate) reasons: Vec<FsSearchIncompleteReason>,
}

impl FileOutcome {
    pub(crate) fn skipped(
        ordinal: usize,
        path: WorkspacePath,
        bytes: u64,
        reasons: Vec<FsSearchIncompleteReason>,
    ) -> Self {
        Self { ordinal, path, matches: Vec::new(), scanned: false, skipped: true, bytes, reasons }
    }
}

#[derive(Clone, Debug)]
pub struct EngineDiagnostics {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub elapsed_ms: u128,
    pub cancelled: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedMatch {
    pub(crate) path: WorkspacePath,
    pub(crate) line_index: u64,
    pub(crate) column_character_offset: u64,
    pub(crate) match_character_length: u64,
    pub(crate) excerpt: String,
    pub(crate) excerpt_start_character_offset: u64,
    pub(crate) excerpt_truncated_before: bool,
    pub(crate) excerpt_truncated_after: bool,
}

pub(crate) fn to_wire_match(
    prepared: PreparedMatch,
    source: FileStorageSource,
    logical_hash: ContentHash,
    buffer_revision: Option<Revision>,
    ignored: bool,
    generated: bool,
) -> FsSearchTextMatch {
    FsSearchTextMatch {
        path: prepared.path,
        line_index: prepared.line_index,
        column_character_offset: prepared.column_character_offset,
        match_character_length: prepared.match_character_length,
        excerpt: prepared.excerpt,
        excerpt_start_character_offset: prepared.excerpt_start_character_offset,
        excerpt_truncated_before: prepared.excerpt_truncated_before,
        excerpt_truncated_after: prepared.excerpt_truncated_after,
        source,
        logical_hash,
        buffer_revision,
        ignored,
        generated,
    }
}

pub(crate) fn completion(reasons: Vec<FsSearchIncompleteReason>) -> FsSearchCompletion {
    if reasons.is_empty() {
        FsSearchCompletion::Complete
    } else {
        let mut reasons = reasons;
        reasons.sort_by_key(|reason| format!("{reason:?}"));
        reasons.dedup();
        FsSearchCompletion::Partial { reasons }
    }
}
