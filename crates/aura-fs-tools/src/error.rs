use std::path::PathBuf;

use aura_domain_common::{ContentHash, Revision, WorkspacePath};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FsToolError {
    #[error(transparent)]
    Workspace(#[from] aura_workspace::WorkspaceError),
    #[error(transparent)]
    Persistence(#[from] aura_persistence::PersistenceError),
    #[error("scope_unavailable: P03 only executes workspace file targets")]
    ScopeUnavailable,
    #[error("invalid fs request: {0}")]
    InvalidRequest(String),
    #[error("file is too large for a full text read; provide a lines or characters range")]
    LargeFileRangeRequired,
    #[error("the requested text range exceeds the configured small-range limit")]
    TextRangeTooLarge,
    #[error("binary range exceeds the 64 KiB hard limit")]
    BinaryRangeTooLarge,
    #[error("expected file to be absent: {path}")]
    ExpectedAbsent { path: WorkspacePath },
    #[error("expected directory was not found: {path}")]
    ExpectedDirectory { path: WorkspacePath },
    #[error("expected hash mismatch for {path}: expected {expected}, actual {actual}")]
    ExpectedHash { path: WorkspacePath, expected: ContentHash, actual: ContentHash },
    #[error("file is actively edited; mutation was deferred and must be re-issued after reread")]
    DeferredByEditorActivity,
    /// No journal or filesystem effect has started. The async application owner may retry the
    /// original request within its original deadline, after releasing its blocking carrier.
    #[error("the complete file resource set is held by a live execution: {0:?}")]
    ResourceBusy(Box<aura_workspace_domain::AdmissionRefusal>),
    #[error("unified diff is invalid: {0}")]
    InvalidPatch(String),
    #[error("unified diff header does not match target {path}")]
    PatchTargetMismatch { path: WorkspacePath },
    #[error(
        "changeset failed at step {failed_step}; applied={applied_steps}, compensated={compensated_steps}: {cause}{journal_fault}"
    )]
    ChangesetFailed {
        failed_step: usize,
        applied_steps: usize,
        compensated_steps: usize,
        cause: String,
        /// The journal write failures this operation also suffered, if any.
        ///
        /// Kept as a **separate, additive** fact rather than replacing the message: the confirmed
        /// rule is that a journal failure must never mask the primary cause, so the caller always
        /// sees the real file failure *and* is told that the record of it is incomplete. It is an
        /// empty string when every journal write succeeded, which is the ordinary case.
        journal_fault: String,
    },
    #[error("changeset compensation failed; workspace reconciliation is required: {0}")]
    ReconciliationRequired(String),
    #[error("workspace mutation completed but revision {revision} could not be projected")]
    ProjectionFailed { revision: Revision },
    #[error("failed to {action} {path}: {source}")]
    Io { action: &'static str, path: PathBuf, source: std::io::Error },
    #[error(
        "the authorized external file {path} is no longer the file that was verified; re-derive \
         the grant before retrying"
    )]
    ExternalFileReplaced { path: PathBuf },
    #[error("external file {path} is not in the state this call required: {reason}")]
    ExternalExpectedState { path: PathBuf, reason: String },
    #[error(
        "publishing external file {path} did not commit and the platform restored the previous \
         content: {reason}"
    )]
    ExternalPublishRolledBack { path: PathBuf, reason: String },
    #[error("search was cancelled")]
    Cancelled,
    #[error("search failed: {0}")]
    Internal(String),
}

/// How far an external publication got, as the file layer really observed it.
///
/// This is a report about the destination, not a classification of an error kind. The
/// platform says where each failure happened, the file layer turns that into one of these,
/// and the caller persists the fact. Guessing from error variants is what let a publication
/// that really landed be recorded as "no effect".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalPublishStage {
    /// The destination still holds exactly what it held before the call, so no attempt reached
    /// it. Nothing was written.
    NeverPublished,
    /// The new content really was published and then removed again, with the previous content
    /// put back in its place.
    ///
    /// Distinct from [`Self::NeverPublished`] because the object *was* written: a compensation
    /// that succeeded is evidence that the write reached the destination, which is a different
    /// fact from a call that was refused before anything moved. Collapsing the two loses the
    /// only record that a publication was attempted and undone.
    PublishedThenRolledBack,
    /// The platform had already replaced the destination with the new content, and the
    /// publication could not then be finalized.
    ///
    /// Distinct from the two above because the new content is on disk: a failure to confirm a
    /// publication is a failure to *confirm*, never evidence that nothing happened.
    PublishedUnconfirmed,
    /// The old and the new content were both moved aside and one of the two moves could not
    /// be undone: the destination is not known to hold either version. A human — or a
    /// separate, identity-bound check of the real object — has to settle it.
    PublishedUnreconciled,
    /// The publication never started: the expected state, the content check, the encoding or
    /// an identity check refused the call before the destination was touched.
    Refused,
}

/// One external write failure, with the stage the publication really reached.
///
/// The file layer's external write entry points return this instead of a bare
/// [`FsToolError`] because the stage is not recoverable from the error afterwards: the
/// caller has to persist "was anything published?" as a fact about the object, and the only
/// place that knows the answer is the code that ran the publication.
#[derive(Debug)]
pub struct ExternalWriteError {
    /// How far the publication got.
    pub stage: ExternalPublishStage,
    /// What went wrong, as the ordinary file-tool error the rest of the system reports.
    ///
    /// Boxed so a publication result stays a small value. The reason itself is only read
    /// after a failure, and several of its variants carry paths and messages.
    pub error: Box<FsToolError>,
    /// The hash the file layer really observed on the destination immediately before it
    /// published, when it got far enough to look. `None` means the call was refused before
    /// any observation was possible — not that the file was empty.
    pub observed_previous_hash: Option<ContentHash>,
}

impl ExternalWriteError {
    /// A refusal that never reached the destination.
    #[must_use]
    pub fn refused(error: FsToolError) -> Self {
        Self {
            stage: ExternalPublishStage::Refused,
            error: Box::new(error),
            observed_previous_hash: None,
        }
    }

    /// The underlying file-tool error, so callers that only need the reason can match on it.
    #[must_use]
    pub fn kind(&self) -> &FsToolError {
        &self.error
    }
}

impl std::fmt::Display for ExternalWriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.stage, self.error)
    }
}

impl std::error::Error for ExternalWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.error.as_ref())
    }
}

impl From<ExternalWriteError> for FsToolError {
    fn from(value: ExternalWriteError) -> Self {
        *value.error
    }
}

pub(crate) fn io_error(
    action: &'static str,
    path: impl Into<PathBuf>,
    source: std::io::Error,
) -> FsToolError {
    FsToolError::Io { action, path: path.into(), source }
}
