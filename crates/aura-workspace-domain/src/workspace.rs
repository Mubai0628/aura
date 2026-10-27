use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{EditorSaveState, WorkspaceLifecycleSummary};
use aura_domain_common::{
    ContentHash, DeferredMutationId, EditorReservationId, FileIdentity, Revision, SessionId,
    WorkspaceId, WorkspacePath,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState {
    Online,
    Offline,
    Archived,
    ReconciliationRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogFreshness {
    Unavailable,
    Rebuilding,
    Fresh,
    Stale,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogIndexMode {
    FullText,
    MetadataOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceWatcherState {
    Initializing,
    Watching,
    Coalescing,
    RescanRequired,
    Rescanning,
    PollingFallback,
    Failed,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSummary {
    pub workspace_id: WorkspaceId,
    pub display_name: String,
    pub selected_root: String,
    pub canonical_root: String,
    pub root_identity: Option<FileIdentity>,
    pub state: WorkspaceState,
    pub revision: Revision,
    pub catalog_freshness: CatalogFreshness,
    pub authorization_epoch: Revision,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPartitionSummary {
    pub partition_path: WorkspacePath,
    pub file_count: u64,
    pub indexable_text_bytes: u64,
    pub index_mode: CatalogIndexMode,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceCatalogStatus {
    pub freshness: CatalogFreshness,
    pub catalog_revision: Revision,
    pub indexed_at: Option<String>,
    pub watcher_state: WorkspaceWatcherState,
    pub partitions: Vec<CatalogPartitionSummary>,
    /// The index build, while it runs or when it did not finish the whole tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexing: Option<WorkspaceIndexingStatus>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceIndexingStatus {
    pub state: WorkspaceIndexingState,
    /// Entries the running or last walk listed.
    pub listed_entries: u64,
    /// Folders recorded but not entered: past the entry cap, or with memory low.
    pub unindexed_directories: u64,
    /// Why the build did not enter every folder, when it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incomplete_reason: Option<WorkspaceIndexIncompleteReason>,
    /// A walk of the whole folder runs now (a check, or one the user asked for); the index
    /// answers meanwhile.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub walking: bool,
    /// Why the build failed, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

/// Why an index build did not enter every folder.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceIndexIncompleteReason {
    /// Free memory ran low; the folders are indexed again once it comes back.
    MemoryLow,
    /// The workspace holds more entries than an index keeps.
    EntryLimit,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceIndexingState {
    Building,
    Incomplete,
    Failed,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalFileStatus {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approval_follow_up: Vec<crate::EditorApprovalFollowUp>,
    pub session_id: SessionId,
    pub path: WorkspacePath,
    pub logical_hash: ContentHash,
    pub disk_hash: Option<ContentHash>,
    pub buffer_revision: Revision,
    pub dirty: bool,
    pub active_editing: bool,
    pub ime_composition_active: bool,
    pub reservation_id: Option<EditorReservationId>,
    pub reservation_epoch: Revision,
    pub core_acknowledged_revision: Revision,
    pub save_state: EditorSaveState,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditorReservationSummary {
    pub session_id: SessionId,
    pub path: WorkspacePath,
    pub reservation_id: EditorReservationId,
    pub reservation_epoch: Revision,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeferredMutationSummary {
    pub deferred_mutation_id: DeferredMutationId,
    pub session_id: SessionId,
    pub path: WorkspacePath,
    pub reason: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileStatus {
    pub workspace_id: WorkspaceId,
    pub workspace_revision: Revision,
    pub catalog: WorkspaceCatalogStatus,
    pub logical_files: Vec<LogicalFileStatus>,
    pub editor_reservations: Vec<EditorReservationSummary>,
    pub deferred_mutations: Vec<DeferredMutationSummary>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFilesProjection {
    pub workspaces: Vec<WorkspaceFileStatus>,
    pub lifecycle_operations: Vec<WorkspaceLifecycleSummary>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceCreateCommand {
    pub selected_root: String,
    pub display_name: String,
}

/// Main owns the native picker. Renderer cannot supply paths or dialog options.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSelectDirectoryRequest {}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceDirectorySelection {
    Cancelled {},
    Selected { selected_root: String, suggested_display_name: String },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceCreateDisposition {
    Created,
    RestoredArchived,
    Existing,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceCreateResult {
    pub disposition: WorkspaceCreateDisposition,
    pub workspace: WorkspaceSummary,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelocateConfirmation {
    RequireSameObject,
    UserConfirmedSameLogicalProject,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRelocateCommand {
    pub workspace_id: WorkspaceId,
    pub selected_root: String,
    pub confirmation: RelocateConfirmation,
    pub expected_workspace_revision: Revision,
}

macro_rules! workspace_id_command {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub workspace_id: WorkspaceId,
        }
    };
}

workspace_id_command!(WorkspaceRestoreCommand);
workspace_id_command!(WorkspaceDeleteCommand);

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceArchiveCommand {
    pub workspace_id: WorkspaceId,
    pub expected_workspace_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDeleteResult {
    pub ended_sessions: Vec<SessionId>,
    pub artifacts_queued_for_gc: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathSearchCommand {
    pub workspace_id: WorkspaceId,
    pub query: String,
    pub limit: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathSearchMatch {
    pub path: WorkspacePath,
    pub score: i64,
    pub kind: crate::CatalogEntryKind,
    pub ignored: bool,
    pub generated: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathSearchResult {
    pub matches: Vec<PathSearchMatch>,
    pub catalog_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BufferOpenCommand {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub path: WorkspacePath,
    pub writable: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BufferEditCommand {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub path: WorkspacePath,
    pub reservation_id: EditorReservationId,
    pub reservation_epoch: Revision,
    pub buffer_revision: Revision,
    pub base_core_revision: Revision,
    pub content: String,
    pub active_editing: bool,
    pub ime_composition_active: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BufferEditResult {
    pub status: LogicalFileStatus,
    pub deferred_intents_superseded: u64,
    pub requires_model_reread: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BufferSaveCommand {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub path: WorkspacePath,
    pub reservation_id: EditorReservationId,
    pub reservation_epoch: Revision,
    pub expected_buffer_revision: Revision,
}
