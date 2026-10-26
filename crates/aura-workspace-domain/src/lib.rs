#![forbid(unsafe_code)]
mod agent_delete;
mod agent_mutation;
mod agent_path;
mod agent_tools;
mod agent_write;
mod agents_init;
mod read_scope;
pub use agent_delete::{AgentFileDeleteInput, AgentFileDeleteOutcome, AgentFileDeleteResult};
pub use agent_mutation::AgentFileMutationResult;
pub use agent_path::{AgentPathError, AgentWorkspacePath, resolve_agent_path};
pub use agent_tools::*;
pub use agents_init::*;
pub use read_scope::{FsSkippedScope, FsSkippedScopeReason};
mod git;
pub use git::*;

mod workspace;
pub use workspace::{
    BufferEditCommand, BufferEditResult, BufferOpenCommand, BufferSaveCommand, CatalogFreshness,
    CatalogIndexMode, CatalogPartitionSummary, DeferredMutationSummary, EditorReservationSummary,
    LogicalFileStatus, PathSearchCommand, PathSearchMatch, PathSearchResult, RelocateConfirmation,
    WorkspaceArchiveCommand, WorkspaceCatalogStatus, WorkspaceCreateCommand,
    WorkspaceCreateDisposition, WorkspaceCreateResult, WorkspaceDeleteCommand,
    WorkspaceDeleteResult, WorkspaceDirectorySelection, WorkspaceFileStatus,
    WorkspaceFilesProjection, WorkspaceIndexingState, WorkspaceIndexingStatus,
    WorkspaceRelocateCommand, WorkspaceRestoreCommand, WorkspaceSelectDirectoryRequest,
    WorkspaceState, WorkspaceSummary, WorkspaceWatcherState,
};
mod editor_approval;
mod lifecycle;
pub use editor_approval::EditorApprovalFollowUp;
pub use lifecycle::{
    BeginWorkspaceLifecycleInput, BeginWorkspaceLifecycleResult, DiscardBufferBinding,
    DiscardConfirmation, EditorBufferLifecycleState, EditorLifecycleTransitionAck, EditorSaveState,
    EditorStateRef, IssueDiscardConfirmationInput, PrepareEditorLifecycleTransition,
    RendererLifecycleUnavailableInput, ResolveWorkspaceLifecycleInput, WorkspaceLifecycleBlocker,
    WorkspaceLifecycleOperation, WorkspaceLifecycleOperationKind, WorkspaceLifecycleOperationQuery,
    WorkspaceLifecyclePreflight, WorkspaceLifecyclePrepareDeliveredInput,
    WorkspaceLifecycleResolution, WorkspaceLifecycleState, WorkspaceLifecycleSummary,
};
mod storage;
pub use storage::{
    SessionCreateRequest, SessionDeleteResult, SessionLifecycleRequest, SessionRenameRequest,
    SessionSummary, WorkspaceSessionProjection,
};
mod fs;
pub use fs::{
    ArtifactExportRequest, ArtifactExportTarget, CaptureWorkspaceFileRequest,
    FsApplyChangesetRequest, FsApplyChangesetResult, FsApplyPatchRequest, FsChangesetOperation,
    FsChangesetStepResult, FsCopyRequest, FsCreateDirectoryRequest, FsDeleteRequest,
    FsExpectedState, FsExternalListTarget, FsExternalReadTarget, FsExternalSearchTarget,
    FsExternalWriteTarget, FsFindPathsRequest, FsGlobRequest, FsGlobResult, FsHashRequest,
    FsHashResult, FsInspectRequest, FsInspectResult, FsListEntry, FsListRequest, FsListResult,
    FsMoveRequest, FsMutationDisposition, FsMutationResult, FsNodeKind, FsReadBinaryRangeRequest,
    FsReadBinaryRangeResult, FsReadTextRequest, FsReadTextResult, FsSearchCompletion,
    FsSearchIncompleteReason, FsSearchPattern, FsSearchTextMatch, FsSearchTextRequest,
    FsSearchTextResult, FsStatRequest, FsStatResult, FsWriteFileRequest,
};
mod file;
pub use file::{
    BinaryDisplayFormat, CatalogEntryKind, FileStorageSource, FileTargetRef, LargeFileReadMode,
    LineEnding, LogicalFileSnapshot, LogicalTextReadSnapshot, TextEncoding, TextReadRange,
    WorkspaceFileTarget,
};
mod resource;
pub use resource::{
    ConflictReason, ContainmentPath, FileResource, ResourceAccess, ResourceConflict,
    ResourceIdentity, ResourceLocation, ResourceOwner, ResourceRequest, ResourceScope,
    SlotCaseRule, SlotParent, conflict_between,
};
mod eligibility;
pub use eligibility::{
    Admission, AdmissionRefusal, AdmissionWait, CommitIntent, CommitRequest, EditClaimId,
    EditEligibility, EligibilityOwner, EligibilitySnapshot, ExecutionId, IdentityMigration,
    ResourceCommit, WorkspaceRelease, fault, owner_of,
};
