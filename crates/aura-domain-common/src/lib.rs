#![forbid(unsafe_code)]

mod canonical_json;
mod content_hash;
mod credential;
mod display_path;
mod error;
mod file;
mod file_name;
mod id;
mod nonce;
mod revision;
mod timestamp;
mod version;

pub use canonical_json::{CanonicalJsonError, canonical_json_v1, canonical_json_v1_sha256};
pub use content_hash::{ContentHash, ContentHashAlgorithm, ContentHashError};
pub use credential::{
    CredentialRef, CredentialRefParseError, CredentialRevision, CredentialRevisionError,
};
pub use display_path::display_path;
pub use error::{
    ErrorCategory, ErrorCode, ErrorCodeError, ErrorEnvelope, NormalizedIssueCode,
    NormalizedIssueCodeError,
};
pub use file::{FileIdentity, WorkspacePath, WorkspacePathError};
pub use file_name::FileNameComparison;
pub use id::{
    AgentRunId, ApprovalDecisionId, ApprovalRequestId, ArtifactId, AttachmentGrantId,
    BrowserShareId, COMMON_ID_PREFIXES, COMMON_ID_TYPES, CancellationId, CausationId, CommandId,
    CommandInvocationId, ContextProjectionSnapshotId, CorrelationId, CredentialId,
    DeferredMutationId, DeleteConfirmationId, EditorReservationId, ElevationLaunchTicketId,
    EndpointId, EventId, ExternalGrantId, FileReferenceId, HostPreparationOperationId,
    IdParseError, IpcChannelId, MemoryId, ModelAvailabilityTestId, ModelContentBlockId,
    ModelDiscoveryRunId, ModelExecutionSnapshotId, ModelFailureId, ModelProfileId, ModelStepId,
    MutationJournalId, NetworkGrantId, NetworkRequestId, PermissionGrantId, PermissionSnapshotId,
    PrincipalId, PromptBundleSnapshotId, ProviderId, QuarantineRecordId, ReleaseBundleId,
    RendererInstanceId, RuntimeInstanceId, RuntimeLeaseId, SandboxAclJournalId, SandboxExecutionId,
    SecurityOperationId, SessionId, SkillInstanceId, StreamId, TaskLineageId, TransportAttemptId,
    TrustRecordId, TurnTaskId, WorkspaceId, WorkspaceLifecycleOperationId,
};
pub use id::{
    CompletionReviewId, HookInvocationId, NaturalCompletionRecordId, NormalCompletionRecordId,
    SourceReadReceiptId, TaskLineageRelationId, TodoItemId, UserQueryId,
};
pub use nonce::SecretNonce;
pub use revision::{ExpectedRevision, Revision, RevisionParseError, SessionSequence};
pub use timestamp::{TimestampParseError, UtcTimestamp};
pub use version::{ContractCompatibilityError, ContractVersion};

pub mod jcs;
pub mod nullable;
mod tool_id;
mod wire_version;
pub use tool_id::{
    TOOL_ID_PREFIXES, TOOL_ID_TYPES, ToolBatchId, ToolCallId, ToolConformanceRunId, ToolDraftId,
    ToolExecutionAttemptId, ToolId, ToolInstanceId, ToolPackageId, ToolRegistrySnapshotId,
};
pub use wire_version::ToolWireVersionV1;

mod semver;
pub use semver::{CanonicalSemVer, CanonicalSemVerError};
