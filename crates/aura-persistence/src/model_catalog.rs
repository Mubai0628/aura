use std::{collections::BTreeMap, str::FromStr};

use aura_domain_common::{ArtifactId, ContentHash, NormalizedIssueCode, Revision, UtcTimestamp};
use aura_model_domain::ProviderIconRef;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use crate::{PersistenceCoordinator, PersistenceError};

#[derive(Clone, Debug)]
pub struct ProviderCatalogCacheWrite {
    pub catalog_revision: Revision,
    pub catalog_content_hash: ContentHash,
    pub checked_at: UtcTimestamp,
    pub last_successful_refresh_at: UtcTimestamp,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub raw_artifact_id: ArtifactId,
    pub snapshot_json: Value,
    pub entries_json: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCatalogCacheSnapshotRecord {
    pub catalog_revision: Revision,
    pub catalog_content_hash: ContentHash,
    pub checked_at: UtcTimestamp,
    pub last_successful_refresh_at: UtcTimestamp,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub raw_artifact_id: ArtifactId,
    pub snapshot_json: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCatalogCacheStateRecord {
    pub snapshot: Option<ProviderCatalogCacheSnapshotRecord>,
    pub last_error_code: Option<NormalizedIssueCode>,
}

impl PersistenceCoordinator {
    pub fn store_provider_catalog_icon(
        &self,
        catalog_provider_id: String,
        icon: ProviderIconRef,
    ) -> Result<(), PersistenceError> {
        if catalog_provider_id.trim().is_empty()
            || icon.pixel_size != ProviderIconRef::PIXEL_SIZE
            || icon.byte_length == 0
        {
            return Err(PersistenceError::InvalidRequest(
                "provider catalog icon metadata is invalid".to_owned(),
            ));
        }
        self.dispatch(move |connection| {
            let exists = connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM artifacts AS artifact
                    JOIN artifact_owners AS owner ON owner.artifact_id=artifact.artifact_id
                    WHERE artifact.artifact_id=?1
                      AND artifact.scope='system_cache'
                      AND artifact.media_type='image/png'
                      AND artifact.byte_length=?2
                      AND artifact.content_hash_algorithm=?3
                      AND artifact.content_hash_hex=?4
                      AND owner.owner_kind='system'
                      AND owner.owner_type='provider_catalog_cache'
                      AND owner.owner_id IS NULL
                 )",
                params![
                    icon.artifact_id.to_string(),
                    i64::try_from(icon.byte_length).map_err(|_| {
                        PersistenceError::InvalidRequest(
                            "provider catalog icon length exceeds SQLite range".to_owned(),
                        )
                    })?,
                    icon.content_hash.algorithm.to_string(),
                    icon.content_hash.hex,
                ],
                |row| row.get::<_, bool>(0),
            )?;
            if !exists {
                return Err(PersistenceError::InvalidRequest(
                    "provider catalog icon artifact is not a valid PNG cache artifact".to_owned(),
                ));
            }
            connection.execute(
                "INSERT INTO provider_catalog_icons(
                    catalog_provider_id, artifact_id, content_hash_json,
                    byte_length, pixel_size, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(catalog_provider_id) DO UPDATE SET
                    artifact_id=excluded.artifact_id,
                    content_hash_json=excluded.content_hash_json,
                    byte_length=excluded.byte_length,
                    pixel_size=excluded.pixel_size,
                    updated_at=excluded.updated_at",
                params![
                    catalog_provider_id,
                    icon.artifact_id.to_string(),
                    serde_json::to_string(&icon.content_hash)?,
                    i64::try_from(icon.byte_length).map_err(|_| {
                        PersistenceError::InvalidRequest(
                            "provider catalog icon length exceeds SQLite range".to_owned(),
                        )
                    })?,
                    i64::from(icon.pixel_size),
                    UtcTimestamp::now().to_string(),
                ],
            )?;
            Ok(())
        })
    }

    pub fn provider_catalog_icons(
        &self,
    ) -> Result<BTreeMap<String, ProviderIconRef>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT icon.catalog_provider_id, icon.artifact_id,
                        icon.content_hash_json, icon.byte_length, icon.pixel_size
                 FROM provider_catalog_icons AS icon
                 JOIN artifacts AS artifact ON artifact.artifact_id=icon.artifact_id
                 JOIN artifact_owners AS owner ON owner.artifact_id=icon.artifact_id
                 WHERE artifact.scope='system_cache'
                   AND artifact.media_type='image/png'
                   AND owner.owner_kind='system'
                   AND owner.owner_type='provider_catalog_cache'
                   AND owner.owner_id IS NULL
                 ORDER BY icon.catalog_provider_id",
            )?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })?
                .map(|row| {
                    let (provider_id, artifact_id, content_hash, byte_length, pixel_size) = row?;
                    Ok((
                        provider_id,
                        ProviderIconRef {
                            artifact_id: ArtifactId::from_str(&artifact_id)?,
                            content_hash: serde_json::from_str(&content_hash)?,
                            byte_length: u64::try_from(byte_length).map_err(|_| {
                                PersistenceError::InvalidRequest(
                                    "persisted provider icon length is invalid".to_owned(),
                                )
                            })?,
                            pixel_size: u16::try_from(pixel_size).map_err(|_| {
                                PersistenceError::InvalidRequest(
                                    "persisted provider icon size is invalid".to_owned(),
                                )
                            })?,
                        },
                    ))
                })
                .collect()
        })
    }

    pub fn provider_catalog_icon_provider(
        &self,
        artifact_id: ArtifactId,
        content_hash: &ContentHash,
    ) -> Result<Option<String>, PersistenceError> {
        let content_hash = serde_json::to_string(content_hash)?;
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT catalog_provider_id FROM provider_catalog_icons
                     WHERE artifact_id=?1 AND content_hash_json=?2",
                    params![artifact_id.to_string(), content_hash],
                    |row| row.get(0),
                )
                .optional()
                .map_err(PersistenceError::from)
        })
    }

    pub fn store_provider_catalog_snapshot(
        &self,
        write: ProviderCatalogCacheWrite,
    ) -> Result<(), PersistenceError> {
        if write.entries_json.keys().any(|id| id.trim().is_empty()) {
            return Err(PersistenceError::InvalidRequest(
                "provider catalog IDs must not be blank".to_owned(),
            ));
        }
        let snapshot_json = serde_json::to_string(&write.snapshot_json)?;
        let entries = write
            .entries_json
            .into_iter()
            .map(|(provider_id, entry)| {
                serde_json::to_string(&entry).map(|json| (provider_id, json))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "INSERT INTO provider_catalog_snapshots(
                    catalog_revision, catalog_content_hash, checked_at,
                    last_successful_refresh_at, etag, last_modified,
                    raw_artifact_id, snapshot_json, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(catalog_revision) DO UPDATE SET
                    catalog_content_hash=excluded.catalog_content_hash,
                    checked_at=excluded.checked_at,
                    last_successful_refresh_at=excluded.last_successful_refresh_at,
                    etag=excluded.etag,
                    last_modified=excluded.last_modified,
                    raw_artifact_id=excluded.raw_artifact_id,
                    snapshot_json=excluded.snapshot_json",
                params![
                    write.catalog_revision.to_string(),
                    serde_json::to_string(&write.catalog_content_hash)?,
                    write.checked_at.to_string(),
                    write.last_successful_refresh_at.to_string(),
                    write.etag,
                    write.last_modified,
                    write.raw_artifact_id.to_string(),
                    snapshot_json,
                    UtcTimestamp::now().to_string(),
                ],
            )?;
            transaction.execute(
                "DELETE FROM provider_catalog_entries WHERE catalog_revision=?1",
                [write.catalog_revision.to_string()],
            )?;
            for (provider_id, entry_json) in entries {
                transaction.execute(
                    "INSERT INTO provider_catalog_entries(
                        catalog_revision, catalog_provider_id, entry_json
                     ) VALUES (?1, ?2, ?3)",
                    params![write.catalog_revision.to_string(), provider_id, entry_json],
                )?;
            }
            transaction.execute(
                "INSERT INTO provider_catalog_runtime_state(
                    catalog_key, current_revision, last_error_code, checked_at, updated_at
                 ) VALUES ('models.dev', ?1, NULL, ?2, ?3)
                 ON CONFLICT(catalog_key) DO UPDATE SET
                    current_revision=excluded.current_revision,
                    last_error_code=NULL,
                    checked_at=excluded.checked_at,
                    updated_at=excluded.updated_at",
                params![
                    write.catalog_revision.to_string(),
                    write.checked_at.to_string(),
                    UtcTimestamp::now().to_string()
                ],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn revalidate_provider_catalog_snapshot(
        &self,
        catalog_revision: Revision,
        checked_at: UtcTimestamp,
        last_successful_refresh_at: UtcTimestamp,
        etag: Option<String>,
        last_modified: Option<String>,
    ) -> Result<(), PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let changed = transaction.execute(
                "UPDATE provider_catalog_snapshots SET
                    checked_at=?1,
                    last_successful_refresh_at=?2,
                    etag=?3,
                    last_modified=?4
                 WHERE catalog_revision=?5",
                params![
                    checked_at.to_string(),
                    last_successful_refresh_at.to_string(),
                    etag,
                    last_modified,
                    catalog_revision.to_string()
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::InvalidRequest(
                    "cannot revalidate a missing provider catalog snapshot".to_owned(),
                ));
            }
            transaction.execute(
                "INSERT INTO provider_catalog_runtime_state(
                    catalog_key, current_revision, last_error_code, checked_at, updated_at
                 ) VALUES ('models.dev', ?1, NULL, ?2, ?3)
                 ON CONFLICT(catalog_key) DO UPDATE SET
                    current_revision=excluded.current_revision,
                    last_error_code=NULL,
                    checked_at=excluded.checked_at,
                    updated_at=excluded.updated_at",
                params![
                    catalog_revision.to_string(),
                    checked_at.to_string(),
                    UtcTimestamp::now().to_string()
                ],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn record_provider_catalog_failure(
        &self,
        issue_code: NormalizedIssueCode,
        checked_at: UtcTimestamp,
    ) -> Result<(), PersistenceError> {
        self.dispatch(move |connection| {
            connection.execute(
                "INSERT INTO provider_catalog_runtime_state(
                    catalog_key, current_revision, last_error_code, checked_at, updated_at
                 ) VALUES ('models.dev', NULL, ?1, ?2, ?3)
                 ON CONFLICT(catalog_key) DO UPDATE SET
                    last_error_code=excluded.last_error_code,
                    checked_at=excluded.checked_at,
                    updated_at=excluded.updated_at",
                params![
                    issue_code.to_string(),
                    checked_at.to_string(),
                    UtcTimestamp::now().to_string()
                ],
            )?;
            Ok(())
        })
    }

    pub fn load_provider_catalog_cache(
        &self,
    ) -> Result<ProviderCatalogCacheStateRecord, PersistenceError> {
        self.with_reader(|connection| {
            let state = connection
                .query_row(
                    "SELECT current_revision, last_error_code
                     FROM provider_catalog_runtime_state WHERE catalog_key='models.dev'",
                    [],
                    |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()?;
            let Some((current_revision, last_error)) = state else {
                return Ok(ProviderCatalogCacheStateRecord {
                    snapshot: None,
                    last_error_code: None,
                });
            };
            let snapshot =
                current_revision
                    .map(|revision| {
                        connection.query_row(
                        "SELECT catalog_content_hash, checked_at, last_successful_refresh_at,
                                etag, last_modified, raw_artifact_id, snapshot_json
                         FROM provider_catalog_snapshots WHERE catalog_revision=?1",
                        [&revision],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<String>>(3)?,
                                row.get::<_, Option<String>>(4)?,
                                row.get::<_, String>(5)?,
                                row.get::<_, String>(6)?,
                            ))
                        },
                    )
                    .map(|record| (revision, record))
                    })
                    .transpose()?
                    .map(
                        |(
                            revision,
                            (
                                catalog_content_hash,
                                checked_at,
                                last_successful_refresh_at,
                                etag,
                                last_modified,
                                raw_artifact_id,
                                snapshot_json,
                            ),
                        )| {
                            Ok::<ProviderCatalogCacheSnapshotRecord, PersistenceError>(
                                ProviderCatalogCacheSnapshotRecord {
                                    catalog_revision: revision.parse().map_err(|_| {
                                        PersistenceError::InvalidRequest(
                                            "persisted provider catalog revision is invalid"
                                                .to_owned(),
                                        )
                                    })?,
                                    catalog_content_hash: serde_json::from_str(
                                        &catalog_content_hash,
                                    )?,
                                    checked_at: checked_at.parse().map_err(|_| {
                                        PersistenceError::InvalidRequest(
                                            "persisted provider catalog timestamp is invalid"
                                                .to_owned(),
                                        )
                                    })?,
                                    last_successful_refresh_at:
                                        last_successful_refresh_at.parse().map_err(|_| {
                                            PersistenceError::InvalidRequest(
                                                "persisted provider catalog successful timestamp is invalid"
                                                    .to_owned(),
                                            )
                                        })?,
                                    etag,
                                    last_modified,
                                    raw_artifact_id: ArtifactId::from_str(&raw_artifact_id)?,
                                    snapshot_json: serde_json::from_str(&snapshot_json)?,
                                },
                            )
                        },
                    )
                    .transpose()?;
            let last_error_code = last_error
                .map(|value| value.parse())
                .transpose()
                .map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "persisted provider catalog issue code is invalid".to_owned(),
                    )
                })?;
            Ok(ProviderCatalogCacheStateRecord { snapshot, last_error_code })
        })
    }
}
