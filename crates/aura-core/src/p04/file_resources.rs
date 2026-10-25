//! Resource admission and additive journal faults for the actual external publication owner.
use std::sync::Arc;

use aura_domain_common::{CorrelationId, SecurityOperationId};
use aura_runtime_contract::IpcErrorEnvelope;
use aura_windows_platform::{NativeFileIdentity, VerifiedFile};
use aura_workspace::{external_object, external_slot};
use aura_workspace_domain::{
    CommitRequest, EligibilityOwner, ExecutionId, ResourceAccess, ResourceCommit, ResourceOwner,
    ResourceRequest,
};

/// The typed code for "another execution holds this external target **right now**".
///
/// It is deliberately distinct from the non-retryable `file.external_resource_held`: this one means
/// no byte was published, the holder is a live execution that will finish, and the object may still
/// be exactly what the caller decided about — so the caller's own asynchronous owner may wait within
/// its original budget and re-enter. `p03`'s dispatcher recognises exactly this code the same way it
/// recognises `fs.resource_busy` for the in-workspace path.
pub(super) const EXTERNAL_RESOURCE_BUSY_CODE: &str = "file.external_resource_busy";

/// The synchronous publication keeps this guard until the actual effect and its settlement end.
pub(super) struct ExternalAdmission {
    owner: Arc<EligibilityOwner>,
    commit: Option<ResourceCommit>,
}

#[derive(Debug)]
pub(super) enum ExternalAdmissionError {
    Facts(aura_workspace::WorkspaceError),
    Refused(aura_workspace_domain::AdmissionRefusal),
}

impl ExternalAdmission {
    pub(super) fn requests(&self) -> &[ResourceRequest] {
        self.commit.as_ref().map_or(&[], ResourceCommit::requests)
    }

    pub(super) fn acquire(
        owner: Arc<EligibilityOwner>,
        parent: &VerifiedFile,
        leaf: &str,
        target: Option<NativeFileIdentity>,
        label: ResourceOwner,
    ) -> Result<Self, ExternalAdmissionError> {
        let session = label.session_id;
        // **The entry carries the parent directory's real ancestor chain**, derived from the OS
        // directory tree above the handle the grant validator already verified. The pre-fix shape
        // used `ResourceLocation::at_anchor(parent_identity)`, which asserts the parent is itself the
        // outermost directory. That is only true one level below a volume root: for an ordinary
        // granted folder several levels deep the anchor was a directory that really sits *inside*
        // some higher directory's chain, so a scope held over that higher directory could never be
        // seen to contain the external target and the deeper write escaped coordination with it.
        //
        // The derivation is the shared one (`external_slot` / `external_object`), so this entry is
        // comparable with every other entry's view of the same physical object rather than being a
        // second, local path-authorisation cache. The directory itself is still not added as a Write
        // target: that would serialise unrelated sibling exports in one folder.
        let slot =
            external_slot(parent, leaf, label.clone()).map_err(ExternalAdmissionError::Facts)?;
        let mut resources = vec![ResourceRequest::new(slot, ResourceAccess::Write)];
        if let Some(target) = target {
            resources.push(ResourceRequest::new(
                external_object(parent, super::domain_identity(&target), label)
                    .map_err(ExternalAdmissionError::Facts)?,
                ResourceAccess::Write,
            ));
        }
        let commit = owner
            .commit(&CommitRequest::per_resource(resources), session, ExecutionId::next(), None)
            .map_err(ExternalAdmissionError::Refused)?;
        Ok(Self { owner, commit: Some(commit) })
    }
}

impl Drop for ExternalAdmission {
    fn drop(&mut self) {
        if let Some(commit) = self.commit.take() {
            self.owner.release_commit(commit);
        }
    }
}

pub(super) fn with_journal_failure<T>(
    mut primary: IpcErrorEnvelope,
    record: Result<T, aura_persistence::PersistenceError>,
) -> IpcErrorEnvelope {
    if let Err(secondary) = record {
        let detail = secondary.to_string();
        primary
            .message
            .push_str(&format!("; recording this publication failure also failed: {detail}"));
        primary.details.insert("journal_failure".into(), serde_json::json!(detail));
    }
    primary
}

/// The bytes have already been published. A failed durable receipt must be queried and reconciled,
/// never represented as a safely retryable write failure.
pub(super) fn unrecorded_publication(
    secondary: aura_persistence::PersistenceError,
    operation_id: SecurityOperationId,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    let mut error = super::envelope(
        "command.outcome_unknown",
        aura_domain_common::ErrorCategory::Unavailable,
        format!("File content was published but its durable receipt failed: {secondary}"),
        false,
        correlation_id,
    );
    error.details.insert("security_operation_id".into(), serde_json::json!(operation_id));
    error.details.insert("journal_failure".into(), serde_json::json!(secondary.to_string()));
    error
}
