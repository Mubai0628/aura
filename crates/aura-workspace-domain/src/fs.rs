use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    BinaryDisplayFormat, FileStorageSource, LineEnding, TextEncoding, TextReadRange,
    WorkspaceFileTarget,
};
use aura_domain_common::{
    ArtifactId, ContentHash, DeferredMutationId, ExternalGrantId, Revision, SessionId, WorkspaceId,
    WorkspacePath,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FsNodeKind {
    File,
    Directory,
    ReparsePoint,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "expected_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FsExpectedState {
    Absent,
    FileHash { hash: ContentHash },
    Directory,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsStatRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: WorkspaceFileTarget,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsStatResult {
    pub path: WorkspacePath,
    pub kind: FsNodeKind,
    pub byte_length: Option<u64>,
    pub modified_at: Option<String>,
    pub content_hash: Option<ContentHash>,
    pub ignored: bool,
    pub generated: bool,
    pub reparse_tag: Option<u32>,
    pub active_editor_session: Option<SessionId>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsListRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: WorkspaceFileTarget,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsListEntry {
    pub path: WorkspacePath,
    pub name: String,
    pub kind: FsNodeKind,
    pub byte_length: Option<u64>,
    pub ignored: bool,
    pub generated: bool,
    pub reparse_tag: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsListResult {
    pub entries: Vec<FsListEntry>,
    pub catalog_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsFindPathsRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub query: String,
    pub limit: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsGlobRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub pattern: String,
    pub limit: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsGlobResult {
    pub paths: Vec<WorkspacePath>,
    pub catalog_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsReadTextRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: WorkspaceFileTarget,
    pub range: TextReadRange,
    pub explicit_encoding: Option<TextEncoding>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsReadTextResult {
    /// The path the read used, relative to its root: the workspace root for an
    /// ordinary read, or the authorized external root when `external` is present.
    pub path: WorkspacePath,
    pub text: String,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    pub source: FileStorageSource,
    pub logical_hash: ContentHash,
    pub disk_hash: Option<ContentHash>,
    pub buffer_revision: Option<Revision>,
    pub is_dirty: bool,
    pub active_editor_session: Option<SessionId>,
    pub disk_may_be_stale: bool,
    pub total_characters: u64,
    pub total_lines: u64,
    pub returned_range: TextReadRange,
    /// Present when this read used an authorized external target. It names the
    /// grant that authorized it and the absolute path the caller referred to, so
    /// the model and the audit trail can tell an external file from a workspace
    /// file without guessing from the path.
    #[serde(default)]
    pub external: Option<FsExternalReadTarget>,
}

/// The authorized external target one read used.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsExternalReadTarget {
    /// The session grant that authorized the target, when a grant did. A path the
    /// existing policy allowed directly — an ordinary external file in FullAccess —
    /// has no grant, and the empty value is how the model and the audit trail can
    /// tell the two sources apart.
    #[serde(default)]
    pub grant_id: Option<ExternalGrantId>,
    /// The absolute path as the user sees it. `path` above is the same file
    /// relative to this grant's root.
    pub display_path: String,
}

/// The authorized external directory one listing used.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsExternalListTarget {
    #[serde(default)]
    pub grant_id: Option<ExternalGrantId>,
    /// The absolute directory path as the user sees it. Each entry's `path` is
    /// relative to this grant's root.
    pub display_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FsSearchPattern {
    Literal {
        text: String,
        /// Default false.
        #[serde(default)]
        case_sensitive: bool,
    },
    Regex {
        pattern: String,
        /// Default false.
        #[serde(default)]
        case_sensitive: bool,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FsSearchIncompleteReason {
    ScopeExcluded,
    ResultLimit,
    ScanByteBudget,
    TimeBudget,
    FileTooLarge,
    UnsupportedEncoding,
    FileChangedDuringSearch,
    FileTemporarilyUnavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum FsSearchCompletion {
    Complete,
    Partial { reasons: Vec<FsSearchIncompleteReason> },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsSearchTextRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub pattern: FsSearchPattern,
    pub root: WorkspacePath,
    pub path_glob: Option<String>,
    pub include_ignored: bool,
    pub include_generated: bool,
    pub limit: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsSearchTextMatch {
    pub path: WorkspacePath,
    pub line_index: u64,
    pub column_character_offset: u64,
    pub match_character_length: u64,
    pub excerpt: String,
    pub excerpt_start_character_offset: u64,
    pub excerpt_truncated_before: bool,
    pub excerpt_truncated_after: bool,
    pub source: FileStorageSource,
    pub logical_hash: ContentHash,
    pub buffer_revision: Option<Revision>,
    pub ignored: bool,
    pub generated: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsSearchTextResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_scopes: Vec<crate::FsSkippedScope>,
    pub catalog_revision: Revision,
    pub matches: Vec<FsSearchTextMatch>,
    pub completion: FsSearchCompletion,
    pub candidate_file_count: u64,
    /// Files whose content was opened and examined. A file that is silently
    /// excluded (binary content) is counted here *and* in
    /// [`Self::skipped_file_count`], so the two counters are independent
    /// observations and their sum may exceed the candidate count.
    pub scanned_file_count: u64,
    pub scanned_bytes: u64,
    /// Files not contributing matches: silently excluded content plus files
    /// reported as skipped with a reason.
    pub skipped_file_count: u64,
    /// Present when this search ran outside the workspace. There is no external index to
    /// read, so the match paths are relative to `external.display_path` and
    /// `catalog_revision` stays the workspace's own revision, which an external search
    /// does not change.
    #[serde(default)]
    pub external: Option<FsExternalSearchTarget>,
}

/// The authorized external directory one text search used.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsExternalSearchTarget {
    /// The session grant that authorized the search, when a grant did. A search the
    /// existing policy allowed directly has none.
    #[serde(default)]
    pub grant_id: Option<ExternalGrantId>,
    /// The absolute directory path as the user sees it. Each match's `path` is relative
    /// to this directory.
    pub display_path: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsHashRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: WorkspaceFileTarget,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsHashResult {
    pub path: WorkspacePath,
    pub hash: ContentHash,
    pub source: FileStorageSource,
    pub disk_hash: Option<ContentHash>,
    pub is_dirty: bool,
    pub active_editor_session: Option<SessionId>,
    pub disk_may_be_stale: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsInspectRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: WorkspaceFileTarget,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsInspectResult {
    pub path: WorkspacePath,
    pub byte_length: u64,
    pub content_hash: ContentHash,
    pub extension: Option<String>,
    pub media_type: String,
    pub magic_description: String,
    pub is_text: bool,
    pub text_encoding: Option<TextEncoding>,
    pub is_executable: bool,
    pub suitable_for_native_attachment: bool,
    pub source: FileStorageSource,
    pub is_dirty: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsReadBinaryRangeRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: WorkspaceFileTarget,
    pub offset: u64,
    pub length: u32,
    pub format: BinaryDisplayFormat,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsReadBinaryRangeResult {
    pub path: WorkspacePath,
    pub offset: u64,
    pub returned_bytes: u32,
    pub total_bytes: u64,
    pub format: BinaryDisplayFormat,
    pub data: String,
    pub content_hash: ContentHash,
    pub source: FileStorageSource,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsWriteFileRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: WorkspaceFileTarget,
    pub content: String,
    pub expected: FsExpectedState,
    pub encoding: Option<TextEncoding>,
    pub line_ending: Option<LineEnding>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsApplyPatchRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: WorkspaceFileTarget,
    pub unified_diff: String,
    pub expected_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsCreateDirectoryRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: WorkspaceFileTarget,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsCopyRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub source: WorkspaceFileTarget,
    pub destination: WorkspaceFileTarget,
    pub source_expected: FsExpectedState,
    pub destination_expected: FsExpectedState,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsMoveRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub source: WorkspaceFileTarget,
    pub destination: WorkspaceFileTarget,
    pub source_expected: FsExpectedState,
    pub destination_expected: FsExpectedState,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsDeleteRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: WorkspaceFileTarget,
    pub expected: FsExpectedState,
    pub recursive: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum FsChangesetOperation {
    WriteFile {
        target: WorkspaceFileTarget,
        content: String,
        expected: FsExpectedState,
        encoding: Option<TextEncoding>,
        line_ending: Option<LineEnding>,
    },
    ApplyPatch {
        target: WorkspaceFileTarget,
        unified_diff: String,
        expected_hash: ContentHash,
    },
    CreateDirectory {
        target: WorkspaceFileTarget,
    },
    Copy {
        source: WorkspaceFileTarget,
        destination: WorkspaceFileTarget,
        source_expected: FsExpectedState,
        destination_expected: FsExpectedState,
    },
    Move {
        source: WorkspaceFileTarget,
        destination: WorkspaceFileTarget,
        source_expected: FsExpectedState,
        destination_expected: FsExpectedState,
    },
    Delete {
        target: WorkspaceFileTarget,
        expected: FsExpectedState,
        recursive: bool,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsApplyChangesetRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub operations: Vec<FsChangesetOperation>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FsMutationDisposition {
    Applied,
    DeferredByEditorActivity,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsMutationResult {
    pub disposition: FsMutationDisposition,
    /// The workspace revision at the time of the mutation. A mutation of an
    /// authorized external file does not change it: that file is not indexed and
    /// has no workspace revision of its own.
    pub workspace_revision: Revision,
    pub affected_paths: Vec<WorkspacePath>,
    pub deferred_mutation_id: Option<DeferredMutationId>,
    pub requires_reread: bool,
    pub catalog_refresh_pending: bool,
    /// Present when this mutation wrote through an authorized external target, so a
    /// caller can tell an external file from a workspace file without guessing from
    /// the path. `affected_paths` then holds the path relative to the grant root.
    #[serde(default)]
    pub external: Option<FsExternalWriteTarget>,
}

/// The authorized external file one mutation wrote.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsExternalWriteTarget {
    #[serde(default)]
    pub grant_id: Option<ExternalGrantId>,
    /// The absolute path as the user sees it.
    pub display_path: String,
    /// True when this call created the file instead of replacing one.
    pub created: bool,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    /// Hash of the bytes this mutation wrote.
    pub logical_hash: ContentHash,
    /// Hash the file had before the write. Absent for a create.
    pub previous_hash: Option<ContentHash>,
    pub bytes_written: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsChangesetStepResult {
    pub index: u32,
    pub operation: String,
    pub paths: Vec<WorkspacePath>,
    /// The hash of the bytes a write or patch step wrote, the file's logical_hash after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_hash: Option<ContentHash>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsApplyChangesetResult {
    pub mutation: FsMutationResult,
    pub steps: Vec<FsChangesetStepResult>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureWorkspaceFileRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: WorkspaceFileTarget,
}

/// Renderer/model-safe export target. Arbitrary absolute paths are
/// intentionally absent: Core resolves either a Workspace path or a
/// Session-bound ExternalGrantId.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "target_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactExportTarget {
    Workspace { path: WorkspacePath },
    ExternalGrant { external_grant_id: ExternalGrantId, relative_path: WorkspacePath },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactExportRequest {
    pub artifact_id: ArtifactId,
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: ArtifactExportTarget,
    pub expected: FsExpectedState,
}
