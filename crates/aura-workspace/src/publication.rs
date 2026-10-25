//! Facts returned by successful editor publications, including partially completed batches.
use crate::WorkspaceError;
use aura_domain_common::{FileIdentity, WorkspaceId};

/// The object and native directory entry a held editor save actually replaced.
#[derive(Clone, Debug)]
pub struct BufferPublicationReceipt {
    pub path: aura_domain_common::WorkspacePath,
    pub workspace_id: WorkspaceId,
    pub previous_identity: FileIdentity,
    pub published_identity: FileIdentity,
    pub parent_identity: FileIdentity,
    pub native_name_key: String,
}

/// Publication facts survive a later settlement or lifecycle failure.
pub struct WorkspacePublicationOutcome<T> {
    pub result: Result<T, WorkspaceError>,
    pub publications: Vec<BufferPublicationReceipt>,
}

impl<T> WorkspacePublicationOutcome<T> {
    pub(crate) fn capture(
        work: impl FnOnce(&mut Vec<BufferPublicationReceipt>) -> Result<T, WorkspaceError>,
    ) -> Self {
        let mut publications = Vec::new();
        let result = work(&mut publications);
        Self { result, publications }
    }
}
