use std::path::PathBuf;

use aura_domain_common::{ContentHashError, IdParseError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("failed to {action} {path}: {source}")]
    Io { action: &'static str, path: PathBuf, source: std::io::Error },
    #[error("failed to encode or decode persisted JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("persisted ID is invalid: {0}")]
    Id(#[from] IdParseError),
    #[error("persisted content hash is invalid: {0}")]
    ContentHash(#[from] ContentHashError),
    #[error("persistence writer is unavailable")]
    WriterUnavailable,
    #[error("persistence writer response channel closed")]
    WriterResponseClosed,
    /// Admission refused this call: its work class was at capacity. The work was
    /// **not** enqueued, so nothing has run and nothing has changed.
    #[error("persistence writer {class} queue is at capacity; the request was not admitted")]
    WriterBusy { class: &'static str },
    /// The deadline expired after the work was enqueued. The answer is unknown: the
    /// write may still run or may already have committed, so a caller must not report
    /// it as "not executed". The original operation identity is what resolves it.
    #[error("persistence writer did not answer the {class} request inside its budget")]
    WriterUnresponsive { class: &'static str },
    /// No read connection was free inside the caller's budget. This is a real
    /// unavailable answer, never an empty result and never "the row is absent".
    #[error("no persistence read connection was available after {waited_ms} ms")]
    ReadUnavailable { waited_ms: u64 },
    /// The read had a connection and did not finish inside the caller's **total** budget,
    /// so its SQLite work was interrupted at the deadline. The call is over — no result
    /// can still arrive and no work outlives it — which is what separates this answer from
    /// [`PersistenceError::WriterUnresponsive`], where a write may still commit.
    #[error(
        "persistence read was interrupted at its {budget_ms} ms budget ({elapsed_ms} ms elapsed)"
    )]
    ReadDeadlineExceeded { budget_ms: u64, elapsed_ms: u64 },
    #[error("persistence read connection lock was poisoned")]
    ReadLockPoisoned,
    #[error("persistence subscription lock was poisoned")]
    SubscriptionLockPoisoned,
    #[error("invalid persistence request: {0}")]
    InvalidRequest(String),
    #[error("workspace was not found")]
    WorkspaceNotFound,
    #[error("workspace root is already registered")]
    WorkspaceRootConflict,
    #[error("workspace operation is blocked by active runtime state")]
    WorkspaceBusy,
    #[error("workspace mutation ownership belongs to another session")]
    MutationOwnedByOtherSession,
    #[error("artifact was not found")]
    ArtifactNotFound,
    #[error("artifact access is denied")]
    ArtifactAccessDenied,
    #[error("quarantined artifacts require an explicit quarantine read capability")]
    QuarantineAccessRequired,
    #[error("artifact range exceeds the 64 KiB P02 read limit")]
    ArtifactRangeTooLarge,
    #[error("artifact content failed integrity verification")]
    ArtifactIntegrity,
    #[error("session permission state was not found")]
    PermissionStateNotFound,
    #[error("session model binding was not found")]
    SessionModelBindingNotFound,
    #[error("session model binding revision is stale")]
    SessionModelBindingRevisionConflict,
    #[error("credential record was not found")]
    CredentialRecordNotFound,
    #[error("credential slot already exists for this endpoint and slot name")]
    CredentialSlotConflict,
    #[error("credential row revision is stale")]
    CredentialRowRevisionConflict,
    #[error("credential mutation lease is already held")]
    CredentialMutationLeaseConflict,
    #[error("credential record state does not permit this operation")]
    CredentialStateConflict,
    #[error("credential mutation journal was not found")]
    CredentialJournalNotFound,
    #[error("credential mutation journal state transition is invalid or stale")]
    CredentialJournalStateConflict,
    #[error("mutation journal was not found")]
    MutationJournalNotFound,
    #[error("mutation journal kind does not match the requested recovery handler")]
    MutationJournalKindConflict,
    #[error("mutation journal state transition is invalid or stale")]
    MutationJournalStateConflict,
    #[error("mutation lease is already held by another journal or runtime")]
    MutationLeaseConflict,
    #[error("mutation recovery handler is already registered")]
    MutationRecoveryHandlerConflict,
    #[error("no mutation recovery handler is registered for journal kind {journal_kind}")]
    MutationRecoveryHandlerMissing { journal_kind: String },
    #[error("credential is unavailable for model transport")]
    CredentialUnavailable,
    #[error("credential binding identity does not match the authorized identity")]
    CredentialBindingMismatch,
    #[error("model discovery run was not found")]
    ModelDiscoveryRunNotFound,
    #[error("model discovery run revision or state is stale")]
    ModelDiscoveryRunStateConflict,
    #[error("model availability test was not found")]
    ModelAvailabilityTestNotFound,
    #[error("model availability test revision or state is stale")]
    ModelAvailabilityTestStateConflict,
    #[error("session permission revision is stale")]
    PermissionRevisionConflict,
    #[error("permission snapshot was not found")]
    PermissionSnapshotNotFound,
    #[error("permission grant was not found")]
    PermissionGrantNotFound,
    #[error("external grant was not found")]
    ExternalGrantNotFound,
    #[error("external grant belongs to a different session")]
    ExternalGrantSessionMismatch,
    #[error("external grant state or revalidation revision is stale")]
    ExternalGrantStateConflict,
    #[error("approval request was not found")]
    ApprovalNotFound,
    #[error("approval request revision is stale")]
    ApprovalStale,
    #[error("approval request already has a terminal state")]
    ApprovalAlreadyResolved,
    #[error("approval decision is not allowed for this request")]
    ApprovalDecisionNotAllowed,
    #[error("trust record was not found")]
    TrustRecordNotFound,
    #[error("sandbox execution was not found")]
    SandboxExecutionNotFound,
    #[error("sandbox ACL journal entry was not found")]
    SandboxJournalNotFound,
    #[error("sandbox state transition conflicts with the persisted state")]
    SandboxStateConflict,
    #[error("host preparation state revision is stale")]
    HostPreparationRevisionConflict,
    #[error("host preparation operation was not found")]
    HostPreparationOperationNotFound,
    #[error("host preparation operation conflicts with the persisted state")]
    HostPreparationStateConflict,
    #[error("network grant was not found")]
    NetworkGrantNotFound,
    #[error("network audit record was not found")]
    NetworkAuditNotFound,
    #[error("network audit already has a terminal state")]
    NetworkAuditAlreadyFinished,
    #[error("quarantine record was not found")]
    QuarantineNotFound,
    #[error("quarantine release requires an approved matching approval")]
    QuarantineReleaseNotApproved,
    #[error("Tool Registry Snapshot or Search Corpus failed integrity verification")]
    ToolRegistryIntegrity,
    #[error("Tool Search Cursor binding failed integrity verification")]
    ToolSearchCursorIntegrity,
    #[error("Tool Activity Cursor binding failed integrity verification")]
    ToolActivityCursorIntegrity,
    #[error("Tool Activity Cursor is unavailable")]
    ToolActivityCursorUnavailable,
    #[error("Tool Activity Cursor does not match the authenticated visibility context")]
    ToolActivityCursorMismatch,
    #[error("Tool Activity Cursor source history was explicitly removed or isolated")]
    ToolActivityCursorExpired,
    #[error("Tool authority entity was not found")]
    ToolEntityNotFound,
    #[error("Tool authority entity revision is stale")]
    ToolEntityRevisionConflict,
    #[error("Tool authority entity state conflicts with the requested mutation")]
    ToolEntityStateConflict,
    #[error("Tool Package key is already held by another live Package identity")]
    ToolPackageKeyConflict,
    #[error("Tool Package version already exists with different immutable content")]
    ToolPackageVersionContentConflict,
    #[error("Tool Package publish evidence is missing or inconsistent")]
    ToolPackagePublishEvidenceConflict,
    #[error("Tool Package payload is unavailable or failed immutable-store verification")]
    ToolPackagePayloadIntegrity,
    #[error("Tool Package mutation requires explicit reconciliation")]
    ToolPackageMutationRecoveryRequired,
    #[error("Tool Package deletion approval is missing, stale, or not user-authenticated")]
    ToolPackageDeleteApprovalInvalid,
    #[error("Tool Package deletion impact changed after the reviewed Preview")]
    ToolPackageDeleteImpactChanged,
    #[error("Tool Package deletion evidence is missing or inconsistent")]
    ToolPackageDeleteEvidenceConflict,
    #[error("Tool long operation has crossed its irreversible boundary or is terminal")]
    ToolLongOperationNotCancellable,
    #[error("Tool authority entity failed identity or canonical integrity validation")]
    ToolEntityIntegrity,
    #[error("Tool execution history requires explicit recovery before Core can become Ready")]
    ToolExecutionRecoveryRequired,
    #[error("authorized Tool Sandbox dispatch binding was not found")]
    ToolSandboxDispatchBindingNotFound,
    #[error("authorized Tool Sandbox dispatch binding does not match the frozen execution")]
    ToolSandboxDispatchBindingMismatch,
    #[error("authorized Tool Sandbox dispatch binding was already consumed or is no longer usable")]
    ToolSandboxDispatchBindingUnavailable,
    #[error("authorized Tool Conformance Sandbox dispatch binding was not found")]
    ToolConformanceSandboxDispatchBindingNotFound,
    #[error("authorized Tool Conformance Sandbox dispatch binding does not match the frozen run")]
    ToolConformanceSandboxDispatchBindingMismatch,
    #[error(
        "authorized Tool Conformance Sandbox dispatch binding was already consumed or is no longer usable"
    )]
    ToolConformanceSandboxDispatchBindingUnavailable,
    #[error("authorized Built-in Core handler dispatch binding was not found")]
    BuiltinCoreHandlerDispatchBindingNotFound,
    #[error(
        "authorized Built-in Core handler dispatch binding does not match the frozen execution"
    )]
    BuiltinCoreHandlerDispatchBindingMismatch,
    #[error(
        "authorized Built-in Core handler dispatch binding was already consumed or is no longer usable"
    )]
    BuiltinCoreHandlerDispatchBindingUnavailable,
    #[error("Tool mutation CommandInvocationId was reused with a different request")]
    ToolMutationCommandIdempotencyConflict,
    #[error("Tool mutation command journal state is invalid or stale")]
    ToolMutationCommandJournalStateConflict,
    #[error("Tool mutation command journal failed canonical integrity verification")]
    ToolMutationCommandJournalIntegrity,
    #[error(
        "historical Context Projection Snapshot v1 is read-only and cannot drive a new ModelStep"
    )]
    HistoricalContextProjectionSnapshot,
    #[error("Context Projection Snapshot failed version or integrity verification")]
    ContextProjectionIntegrity,
}
