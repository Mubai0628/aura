use std::{collections::BTreeMap, sync::Arc};

use aura_artifact_contract::{
    ArtifactAccess, ArtifactOwnerRef, ArtifactScope, ArtifactSystemOwner,
};
use aura_domain_common::{NormalizedIssueCode, UtcTimestamp};
use aura_model_domain::ProviderCatalogSnapshot;
use aura_persistence::{
    ArtifactMetadataRequest, ArtifactWriteRequest, PersistenceCoordinator,
    ProviderCatalogCacheWrite,
};

use crate::{CatalogCache, CatalogCacheRecord, CatalogError};

pub struct PersistenceCatalogCache {
    persistence: Arc<PersistenceCoordinator>,
}

impl PersistenceCatalogCache {
    #[must_use]
    pub fn new(persistence: Arc<PersistenceCoordinator>) -> Self {
        Self { persistence }
    }
}

impl CatalogCache for PersistenceCatalogCache {
    fn load(&self) -> Result<CatalogCacheRecord, CatalogError> {
        let state = self.persistence.load_provider_catalog_cache().map_err(cache_error)?;
        let snapshot = state
            .snapshot
            .map(|record| {
                let snapshot: ProviderCatalogSnapshot =
                    serde_json::from_value(record.snapshot_json).map_err(CatalogError::Decode)?;
                if snapshot.catalog_revision != record.catalog_revision
                    || snapshot.catalog_content_hash != record.catalog_content_hash
                    || snapshot.raw_artifact_id != record.raw_artifact_id
                    || snapshot.checked_at != record.checked_at
                    || snapshot.last_successful_refresh_at != record.last_successful_refresh_at
                    || snapshot.etag != record.etag
                    || snapshot.last_modified != record.last_modified
                {
                    return Err(CatalogError::Cache(
                        "persisted provider catalog envelope does not match its snapshot"
                            .to_owned(),
                    ));
                }
                let artifact = self
                    .persistence
                    .artifact_record(ArtifactMetadataRequest {
                        artifact_id: snapshot.raw_artifact_id,
                        session_id: None,
                        required_access: ArtifactAccess::USER_READ,
                        allow_quarantine: false,
                    })
                    .map_err(cache_error)?;
                if artifact.scope != ArtifactScope::SystemCache
                    || artifact.content_hash != snapshot.catalog_content_hash
                {
                    return Err(CatalogError::Cache(
                        "persisted provider catalog artifact does not match its revision"
                            .to_owned(),
                    ));
                }
                Ok(snapshot)
            })
            .transpose()?;
        Ok(CatalogCacheRecord { snapshot, last_error_code: state.last_error_code })
    }

    fn persist_success(
        &self,
        raw: &[u8],
        mut snapshot: ProviderCatalogSnapshot,
    ) -> Result<ProviderCatalogSnapshot, CatalogError> {
        if snapshot.catalog_content_hash != aura_domain_common::ContentHash::blake3(raw) {
            return Err(CatalogError::Cache(
                "provider catalog revision does not match the raw response".to_owned(),
            ));
        }
        let artifact = self
            .persistence
            .write_artifact(ArtifactWriteRequest {
                scope: ArtifactScope::SystemCache,
                media_type: "application/json".to_owned(),
                access: ArtifactAccess::USER_READ,
                owner: ArtifactOwnerRef::System {
                    owner: ArtifactSystemOwner::ProviderCatalogCache,
                },
                bytes: raw.to_vec(),
            })
            .map_err(cache_error)?;
        snapshot.raw_artifact_id = artifact.artifact_id;
        let snapshot_json = serde_json::to_value(&snapshot).map_err(CatalogError::Decode)?;
        let entries_json = snapshot
            .providers
            .iter()
            .map(|(provider_id, entry)| {
                serde_json::to_value(entry)
                    .map(|value| (provider_id.clone(), value))
                    .map_err(CatalogError::Decode)
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        if let Err(error) =
            self.persistence.store_provider_catalog_snapshot(ProviderCatalogCacheWrite {
                catalog_revision: snapshot.catalog_revision,
                catalog_content_hash: snapshot.catalog_content_hash.clone(),
                checked_at: snapshot.checked_at,
                last_successful_refresh_at: snapshot.last_successful_refresh_at,
                etag: snapshot.etag.clone(),
                last_modified: snapshot.last_modified.clone(),
                raw_artifact_id: snapshot.raw_artifact_id,
                snapshot_json,
                entries_json,
            })
        {
            let _ = self.persistence.enqueue_artifact_gc_if_unreferenced(
                snapshot.raw_artifact_id,
                "provider_catalog_snapshot_commit_failed",
            );
            return Err(cache_error(error));
        }
        Ok(snapshot)
    }

    fn persist_revalidated(&self, snapshot: ProviderCatalogSnapshot) -> Result<(), CatalogError> {
        self.persistence
            .revalidate_provider_catalog_snapshot(
                snapshot.catalog_revision,
                snapshot.checked_at,
                snapshot.last_successful_refresh_at,
                snapshot.etag,
                snapshot.last_modified,
            )
            .map_err(cache_error)
    }

    fn persist_failure(&self, issue_code: &NormalizedIssueCode) -> Result<(), CatalogError> {
        self.persistence
            .record_provider_catalog_failure(issue_code.clone(), UtcTimestamp::now())
            .map_err(cache_error)
    }
}

fn cache_error(error: aura_persistence::PersistenceError) -> CatalogError {
    CatalogError::Cache(error.to_string())
}
