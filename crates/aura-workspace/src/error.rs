use std::path::PathBuf;

use aura_domain_common::{
    EditorReservationId, RendererInstanceId, Revision, SessionId, WorkspacePath,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Persistence(#[from] aura_persistence::PersistenceError),
    #[error("failed to {action} {path}: {source}")]
    Io { action: &'static str, path: PathBuf, source: std::io::Error },
    #[error("workspace path resolution failed for {path}: {source}")]
    PathResolutionFailed { path: PathBuf, source: std::io::Error },
    #[error("workspace root must be an existing directory")]
    InvalidRoot,
    #[error("workspace is offline")]
    Offline,
    #[error("workspace is archived")]
    Archived,
    #[error("workspace is busy: {reason}")]
    Busy { reason: String },
    #[error("relocation changed file identity and requires explicit same-project confirmation")]
    RelocateConfirmationRequired,
    #[error("workspace root identity changed while the workspace was active")]
    RootIdentityChanged,
    #[error("workspace path resolves outside the final workspace root: {path}")]
    ScopeUnavailable { path: WorkspacePath },
    #[error("workspace path does not exist: {path}")]
    NotFound { path: WorkspacePath },
    #[error("workspace path is not the required node kind: {path}")]
    WrongKind { path: WorkspacePath },
    #[error("editor reservation for {path} belongs to session {owner}")]
    EditorReserved { path: WorkspacePath, owner: SessionId },
    #[error("buffer revision conflict: expected {expected}, current {actual}")]
    BufferRevisionConflict { expected: Revision, actual: Revision },
    #[error("file version changed on disk: {path}")]
    FileVersionConflict { path: WorkspacePath },
    /// Another execution holds this file's resource **right now**, and nothing has been written.
    ///
    /// This is deliberately not [`Self::FileVersionConflict`]. The two look similar from a caller's
    /// point of view and mean opposite things:
    ///
    /// * a **version conflict** is a statement about the *object* — the bytes or the identity are no
    ///   longer the ones the caller decided about — and waiting cannot change it. The caller must
    ///   re-read and decide again.
    /// * a **resource short commit** is a statement about *who is in the resource at this instant*.
    ///   The file on disk may still be exactly what the caller expects; the resource is simply taken
    ///   by another execution that will finish. Waiting is therefore meaningful, and the caller's own
    ///   asynchronous owner may wait within its original budget and try the unchanged request again.
    ///
    /// Keeping them apart is what lets the save path wait for a live short commit without ever
    /// waiting on a genuinely stale version, and without renaming one error into a pretence of
    /// waiting. The refusal is carried whole so a caller can report who holds it.
    #[error("file resource is held by another execution: {path}")]
    FileResourceBusy { path: WorkspacePath, refusal: Box<aura_workspace_domain::AdmissionRefusal> },
    #[error("logical buffer was not found")]
    BufferNotFound,
    #[error("logical buffer is read-only")]
    BufferReadOnly,
    #[error("workspace lifecycle transition is in progress")]
    LifecycleTransitionInProgress,
    #[error("workspace lifecycle operation was not found")]
    LifecycleNotFound,
    #[error("workspace lifecycle revision conflict: expected {expected}, current {actual}")]
    LifecycleRevisionConflict { expected: Revision, actual: Revision },
    #[error("workspace revision conflict: expected {expected}, current {actual}")]
    WorkspaceRevisionConflict { expected: Revision, actual: Revision },
    #[error("workspace lifecycle renderer mismatch: expected {expected}, received {actual}")]
    LifecycleRendererMismatch { expected: RendererInstanceId, actual: RendererInstanceId },
    #[error("workspace lifecycle Prepare ACK timed out")]
    EditorPrepareAckTimeout,
    #[error("workspace editor state could not be confirmed")]
    EditorStateUnconfirmed,
    #[error("workspace lifecycle editor blockers remain")]
    LifecycleEditorBlockersRemain,
    #[error("editor reservation {reservation_id} was revoked at epoch {current_epoch}")]
    ReservationRevoked { reservation_id: EditorReservationId, current_epoch: Revision },
    #[error("editor reservation {reservation_id} was not found")]
    ReservationNotFound { reservation_id: EditorReservationId },
    #[error("discard confirmation no longer matches the current dirty Buffer set")]
    DiscardConfirmationStale,
    #[error("discard confirmation is required")]
    DiscardConfirmationRequired,
    #[error("discard confirmation expired")]
    DiscardConfirmationExpired,
    #[error("discard confirmation is invalid")]
    DiscardConfirmationInvalid,
    #[error("workspace lifecycle target changed after Preparing started")]
    LifecycleTargetChanged,
    #[error("failed to obtain cryptographically secure randomness: {0}")]
    Randomness(String),
    #[error("text encoding is ambiguous; explicit encoding is required")]
    EncodingAmbiguous { candidates: Vec<String>, preview: String },
    #[error("catalog is unavailable or stale")]
    CatalogUnavailable,
    /// The index is still being built after the bounded wait a query gives it.
    #[error("索引还在建立（已处理 {listed} 项），请稍后再试；文件夹可以逐层展开查看")]
    IndexBuilding { listed: u64 },
    /// Another update committed while this one was being planned, so the plan was
    /// discarded rather than written over newer facts. The changes are still valid;
    /// the caller decides whether to describe them again.
    #[error("catalog plan was superseded by a newer revision after {replans} attempts")]
    CatalogPlanSuperseded { replans: usize },
    #[error("workspace lock was poisoned: {0}")]
    LockPoisoned(&'static str),
    #[error("watcher failed: {0}")]
    Watcher(String),
    #[error("workspace revision cannot advance beyond u64::MAX")]
    RevisionExhausted,
}

pub(crate) fn io_error(
    action: &'static str,
    path: impl Into<PathBuf>,
    source: std::io::Error,
) -> WorkspaceError {
    WorkspaceError::Io { action, path: path.into(), source }
}
