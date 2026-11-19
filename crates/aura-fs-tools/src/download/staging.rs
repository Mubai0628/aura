use super::{DownloadTarget, lines, target::invalid};
use crate::{FilePublicationReceipt, FsToolError, FsToolService};
use aura_domain_common::{CorrelationId, SessionId, WorkspaceId, WorkspacePath};
use aura_workspace_domain::{
    CommitRequest, EligibilityOwner, ExecutionId, ResourceAccess, ResourceCommit, ResourceOwner,
    ResourceRequest,
};
use std::sync::Arc;

pub struct DownloadStaging {
    staging: Option<aura_windows_platform::StagedFilePublication>,
    target: DownloadTarget,
    hash: blake3::Hasher,
    owner: Arc<EligibilityOwner>,
    commit: Option<ResourceCommit>,
    folders: super::folders::DownloadFolders,
}
pub struct DownloadPublication {
    pub detected_media_type: Option<&'static str>,
    pub bytes_written: u64,
    pub hash: aura_domain_common::ContentHash,
    pub overwritten: bool,
    pub max_chunk_bytes: usize,
    pub receipts: Vec<FilePublicationReceipt>,
}

impl FsToolService {
    fn download_resources(
        &self,
        target: &DownloadTarget,
        workspace: WorkspaceId,
        session: SessionId,
    ) -> Result<Vec<ResourceRequest>, FsToolError> {
        target.verify()?;
        let mut resources = Vec::new();
        if let Some(path) = &target.workspace_path {
            let facts = self.resource_facts(workspace, session)?;
            resources.push(ResourceRequest::new(facts.slot(path)?, ResourceAccess::Write));
            if let Some(object) = facts.object(path)? {
                resources.push(ResourceRequest::new(object, ResourceAccess::Write));
            }
        } else {
            let parent = target.parent()?;
            let leaf = target.path.file_name().ok_or_else(lines::no_leaf)?.to_string_lossy();
            let owner = ResourceOwner {
                workspace_id: workspace,
                session_id: session,
                path: WorkspacePath::parse(".").map_err(invalid)?,
            };
            resources.push(ResourceRequest::new(
                aura_workspace::external_slot(&parent, &leaf, owner.clone())?,
                ResourceAccess::Write,
            ));
            if let Some(identity) = &target.identity {
                resources.push(ResourceRequest::new(
                    aura_workspace::external_object(&parent, identity.clone(), owner)?,
                    ResourceAccess::Write,
                ));
            }
        }
        Ok(resources)
    }
    pub fn preflight_download(
        &self,
        target: &DownloadTarget,
        workspace: WorkspaceId,
        session: SessionId,
    ) -> Result<(), FsToolError> {
        let resources = self.download_resources(target, workspace, session)?;
        let owner = self.eligibility_owner();
        let commit = owner
            .commit(&CommitRequest::per_resource(resources), session, ExecutionId::next(), None)
            .map_err(|e| FsToolError::ResourceBusy(Box::new(e)))?;
        owner.release_commit(commit);
        Ok(())
    }
    pub fn begin_download(
        &self,
        target: DownloadTarget,
        workspace: WorkspaceId,
        session: SessionId,
    ) -> Result<DownloadStaging, FsToolError> {
        let resources = self.download_resources(&target, workspace, session)?;
        let owner = self.eligibility_owner();
        let commit = owner
            .commit(&CommitRequest::per_resource(resources), session, ExecutionId::next(), None)
            .map_err(|e| FsToolError::ResourceBusy(Box::new(e)))?;
        let mut result = DownloadStaging {
            staging: None,
            target,
            hash: blake3::Hasher::new(),
            owner,
            commit: Some(commit),
            folders: super::folders::DownloadFolders::default(),
        };
        let anchor = result.target.parent()?;
        let parent = result.folders.make(&mut result.target, anchor)?;
        let leaf = result.target.path.file_name().ok_or_else(lines::no_leaf)?;
        let expected = match (&result.target.identity, &result.target.blake3) {
            (Some(identity), Some(hash)) => {
                Some((aura_workspace::native_identity(identity).map_err(invalid)?, hash.clone()))
            }
            (None, None) => None,
            _ => return Err(lines::inconsistent()),
        };
        result.staging = Some(
            aura_windows_platform::StagedFilePublication::new(&parent, leaf, expected)
                .map_err(|error| lines::staging_failed(&parent.info().final_path, &error))?,
        );
        Ok(result)
    }
    pub fn finish_download(
        &self,
        mut download: DownloadStaging,
        workspace: WorkspaceId,
        correlation: CorrelationId,
    ) -> Result<DownloadPublication, FsToolError> {
        let stage = download.staging.take().ok_or_else(lines::staging_finished)?;
        let bytes_written = stage.bytes_written();
        let max_chunk_bytes = stage.max_chunk_bytes();
        let hash = aura_domain_common::ContentHash::new(
            aura_domain_common::ContentHashAlgorithm::Blake3,
            download.hash.finalize().to_hex().to_string(),
        )
        .map_err(invalid)?;
        let (published, receipts) = crate::with_file_publications(|| {
            let mut published =
                stage.publish().map_err(|e| publication_error(&download.target, e))?;
            let media_type = super::media::detect(published.file_mut(), bytes_written);
            let parent = download.target.parent()?;
            let info = published.commit().map_err(|e| publication_error(&download.target, e))?;
            let previous = download
                .target
                .identity
                .as_ref()
                .map(aura_workspace::native_identity)
                .transpose()
                .map_err(invalid)?;
            crate::publication_receipt::published(
                &parent,
                download.target.path.file_name().ok_or_else(lines::no_leaf)?,
                previous.as_ref(),
                &info,
            );
            Ok::<_, FsToolError>(media_type)
        });
        let detected_media_type = published?;
        download.folders.keep();
        if let Some(path) = &download.target.workspace_path {
            let catalog = self.catalog(workspace)?;
            let resolver = self.resolver(workspace)?;
            let update = aura_workspace::CatalogUpdate::new(
                vec![(path.clone(), aura_workspace::CatalogChange::Changed)],
                "http.download",
            );
            if catalog
                .apply_changes(
                    &resolver,
                    update,
                    &aura_workspace::CatalogOptions::default(),
                    correlation,
                )
                .is_err()
            {
                self.persistence.mark_catalog_stale(workspace)?;
            }
            self.persistence.complete_workspace_file_mutation(
                workspace,
                correlation,
                "http.download",
            )?;
            self.workspace.notify_projection_change()?;
        }
        Ok(DownloadPublication {
            detected_media_type,
            bytes_written,
            hash,
            overwritten: download.target.identity.is_some(),
            max_chunk_bytes,
            receipts,
        })
    }
}
impl DownloadStaging {
    pub fn append(&mut self, bytes: &[u8]) -> Result<(), FsToolError> {
        self.staging
            .as_mut()
            .ok_or_else(lines::staging_finished)?
            .append(bytes)
            .map_err(|error| lines::staging_write_failed(&error))?;
        self.hash.update(bytes);
        Ok(())
    }
}
impl Drop for DownloadStaging {
    fn drop(&mut self) {
        self.staging.take();
        // After the staged file is gone, so the folders it was in are empty again.
        self.folders.take_back();
        if let Some(commit) = self.commit.take() {
            self.owner.release_commit(commit);
        }
    }
}

fn publication_error(
    target: &DownloadTarget,
    error: aura_windows_platform::HandlePublishError,
) -> FsToolError {
    match error.location() {
        aura_windows_platform::PublishLocation::NeverPublished => invalid("http.never_published"),
        aura_windows_platform::PublishLocation::RolledBack => {
            FsToolError::ExternalPublishRolledBack {
                path: target.path.clone(),
                reason: "download publication restored the previous content".into(),
            }
        }
        aura_windows_platform::PublishLocation::Unconfirmable => {
            FsToolError::ReconciliationRequired("http.publication_unconfirmed".into())
        }
    }
}
