use std::{
    fs::{self, OpenOptions},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    str::FromStr,
};

use aura_artifact_contract::{
    ArtifactAccess, ArtifactAccessLinkV1, ArtifactAccessSubjectV1, ArtifactOperationOwnerRef,
    ArtifactOwnerRef, ArtifactRecord, ArtifactScope, ArtifactSystemOwner, ToolArtifactOwnerRef,
};
use aura_domain_common::{
    ArtifactId, ContentHash, ContentHashAlgorithm, ModelAvailabilityTestId, ModelDiscoveryRunId,
    SessionId, UtcTimestamp,
};
use rusqlite::{OptionalExtension, Transaction, params};
use ulid::Ulid;

use crate::{PersistenceCoordinator, PersistenceError};

pub const ARTIFACT_INLINE_LIMIT: usize = 16 * 1024;
pub const ARTIFACT_CHUNK_SIZE: usize = 1024 * 1024;
pub const ARTIFACT_RANGE_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct ArtifactWriteRequest {
    pub scope: ArtifactScope,
    pub media_type: String,
    pub access: ArtifactAccess,
    pub owner: ArtifactOwnerRef,
    pub bytes: Vec<u8>,
}

/// Core-owned Artifact identity and bytes prepared before a larger SQLite
/// transaction. The row and any object references are inserted only when the
/// owning transaction commits.
#[derive(Clone, Debug)]
pub struct ArtifactWriteDraftV1 {
    pub artifact_id: ArtifactId,
    pub created_at: UtcTimestamp,
    pub request: ArtifactWriteRequest,
}

impl ArtifactWriteDraftV1 {
    pub fn new(
        request: ArtifactWriteRequest,
        created_at: UtcTimestamp,
    ) -> Result<Self, PersistenceError> {
        validate_write_request(&request)?;
        u64::try_from(request.bytes.len()).map_err(|_| {
            PersistenceError::InvalidRequest("artifact exceeds u64 length".to_owned())
        })?;
        Ok(Self { artifact_id: ArtifactId::new(), created_at, request })
    }

    pub fn record(&self) -> Result<ArtifactRecord, PersistenceError> {
        validate_write_request(&self.request)?;
        let byte_length = u64::try_from(self.request.bytes.len()).map_err(|_| {
            PersistenceError::InvalidRequest("artifact exceeds u64 length".to_owned())
        })?;
        Ok(ArtifactRecord {
            artifact_id: self.artifact_id,
            owner: self.request.owner.clone(),
            scope: self.request.scope,
            media_type: self.request.media_type.clone(),
            access: self.request.access,
            byte_length,
            content_hash: ContentHash::blake3(&self.request.bytes),
            inline: self.request.bytes.len() <= ARTIFACT_INLINE_LIMIT,
            created_at: self.created_at,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ArtifactReadRequest {
    pub artifact_id: ArtifactId,
    pub session_id: Option<SessionId>,
    pub required_access: ArtifactAccess,
    pub offset: u64,
    pub length: usize,
    pub allow_quarantine: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ArtifactMetadataRequest {
    pub artifact_id: ArtifactId,
    pub session_id: Option<SessionId>,
    pub required_access: ArtifactAccess,
    pub allow_quarantine: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ArtifactGcReport {
    pub artifacts_removed: usize,
    pub objects_removed: usize,
    pub orphan_files_remaining: usize,
}

#[derive(Clone, Debug)]
struct PreparedChunk {
    chunk_index: usize,
    raw_offset: usize,
    raw_length: usize,
    stored_length: usize,
    compression: &'static str,
    raw_hash: String,
    object_hash: String,
    relative_path: String,
}

#[derive(Clone, Debug)]
struct ArtifactRow {
    owner: ArtifactOwnerRef,
    scope: ArtifactScope,
    access: ArtifactAccess,
    byte_length: u64,
    content_hash: ContentHash,
    inline_bytes: Option<Vec<u8>>,
    link_access: Option<ArtifactAccess>,
}

impl PersistenceCoordinator {
    pub fn artifact_record(
        &self,
        request: ArtifactMetadataRequest,
    ) -> Result<ArtifactRecord, PersistenceError> {
        if request.required_access == ArtifactAccess::NONE {
            return Err(PersistenceError::InvalidRequest(
                "artifact metadata requires an explicit access capability".to_owned(),
            ));
        }
        self.with_reader(|connection| {
            let session_id = request.session_id.map(|id| id.to_string());
            let row = connection
                .query_row(
                    "SELECT a.scope, a.media_type, a.access_flags, a.byte_length,
                            a.content_hash_algorithm, a.content_hash_hex,
                            a.inline_bytes IS NOT NULL, a.created_at, l.access_flags,
                            o.owner_kind, o.owner_type, o.owner_id
                     FROM artifacts a
                     JOIN artifact_owners o ON o.artifact_id=a.artifact_id
                     LEFT JOIN artifact_access_links l
                       ON l.artifact_id=a.artifact_id AND l.session_id=?2
                     WHERE a.artifact_id=?1",
                    params![request.artifact_id.to_string(), session_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, bool>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, Option<i64>>(8)?,
                            row.get::<_, String>(9)?,
                            row.get::<_, String>(10)?,
                            row.get::<_, Option<String>>(11)?,
                        ))
                    },
                )
                .optional()?
                .ok_or(PersistenceError::ArtifactNotFound)?;
            let scope = parse_scope(&row.0)?;
            let access = parse_access(row.2)?;
            let byte_length = u64::try_from(row.3).map_err(|_| {
                PersistenceError::InvalidRequest("negative artifact byte length".to_owned())
            })?;
            let content_hash = parse_content_hash(&row.4, row.5)?;
            let owner = parse_owner(&row.9, &row.10, row.11.as_deref())?;
            let authorization = ArtifactRow {
                owner: owner.clone(),
                scope,
                access,
                byte_length,
                content_hash: content_hash.clone(),
                inline_bytes: None,
                link_access: row.8.map(parse_access).transpose()?,
            };
            authorize_artifact(
                &authorization,
                &ArtifactReadRequest {
                    artifact_id: request.artifact_id,
                    session_id: request.session_id,
                    required_access: request.required_access,
                    offset: 0,
                    length: 0,
                    allow_quarantine: request.allow_quarantine,
                },
            )?;
            Ok(ArtifactRecord {
                artifact_id: request.artifact_id,
                owner,
                scope,
                media_type: row.1,
                access,
                byte_length,
                content_hash,
                inline: row.6,
                created_at: row.7.parse().map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "persisted artifact created_at is invalid".to_owned(),
                    )
                })?,
            })
        })
    }

    pub fn write_artifact(
        &self,
        request: ArtifactWriteRequest,
    ) -> Result<ArtifactRecord, PersistenceError> {
        let artifact_id = ArtifactId::new();
        let artifact_root = self.artifact_root().to_path_buf();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let record =
                insert_artifact_in_transaction(&transaction, &artifact_root, artifact_id, request)?;
            transaction.commit()?;
            Ok(record)
        })
    }

    pub fn read_artifact_range(
        &self,
        request: ArtifactReadRequest,
    ) -> Result<Vec<u8>, PersistenceError> {
        if request.required_access == ArtifactAccess::NONE {
            return Err(PersistenceError::InvalidRequest(
                "artifact reads require an explicit access capability".to_owned(),
            ));
        }
        if request.length > ARTIFACT_RANGE_LIMIT {
            return Err(PersistenceError::ArtifactRangeTooLarge);
        }
        let artifact_root = self.artifact_root().to_path_buf();
        self.with_reader(|connection| {
            let session_id = request.session_id.map(|id| id.to_string());
            let row = connection
                .query_row(
                    "SELECT a.scope, a.access_flags, a.byte_length,
                            a.content_hash_algorithm, a.content_hash_hex, a.inline_bytes,
                            l.access_flags, o.owner_kind, o.owner_type, o.owner_id
                     FROM artifacts a
                     JOIN artifact_owners o ON o.artifact_id=a.artifact_id
                     LEFT JOIN artifact_access_links l
                       ON l.artifact_id=a.artifact_id AND l.session_id=?2
                     WHERE a.artifact_id=?1",
                    params![request.artifact_id.to_string(), session_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<Vec<u8>>>(5)?,
                            row.get::<_, Option<i64>>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, Option<String>>(9)?,
                        ))
                    },
                )
                .optional()?
                .ok_or(PersistenceError::ArtifactNotFound)?;
            let artifact = ArtifactRow {
                owner: parse_owner(&row.7, &row.8, row.9.as_deref())?,
                scope: parse_scope(&row.0)?,
                access: parse_access(row.1)?,
                byte_length: u64::try_from(row.2).map_err(|_| {
                    PersistenceError::InvalidRequest("negative artifact byte length".to_owned())
                })?,
                content_hash: parse_content_hash(&row.3, row.4)?,
                inline_bytes: row.5,
                link_access: row.6.map(parse_access).transpose()?,
            };
            authorize_artifact(&artifact, &request)?;
            let end = request
                .offset
                .saturating_add(u64::try_from(request.length).unwrap_or(u64::MAX))
                .min(artifact.byte_length);
            if request.offset >= end {
                return Ok(Vec::new());
            }
            if let Some(inline) = artifact.inline_bytes {
                if u64::try_from(inline.len()).ok() != Some(artifact.byte_length)
                    || ContentHash::blake3(&inline) != artifact.content_hash
                {
                    return Err(PersistenceError::ArtifactIntegrity);
                }
                let start = usize::try_from(request.offset).map_err(|_| {
                    PersistenceError::InvalidRequest("artifact offset exceeds usize".to_owned())
                })?;
                let end = usize::try_from(end).map_err(|_| {
                    PersistenceError::InvalidRequest("artifact end exceeds usize".to_owned())
                })?;
                return Ok(inline[start..end].to_vec());
            }
            read_chunk_range(connection, &artifact_root, request.artifact_id, request.offset, end)
        })
    }

    pub(crate) fn read_internal_artifact_range(
        &self,
        artifact_id: ArtifactId,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, PersistenceError> {
        if length > ARTIFACT_RANGE_LIMIT {
            return Err(PersistenceError::ArtifactRangeTooLarge);
        }
        let artifact_root = self.artifact_root().to_path_buf();
        self.with_reader(|connection| {
            let row = connection
                .query_row(
                    "SELECT byte_length, content_hash_algorithm, content_hash_hex, inline_bytes
                     FROM artifacts WHERE artifact_id=?1",
                    [artifact_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<Vec<u8>>>(3)?,
                        ))
                    },
                )
                .optional()?
                .ok_or(PersistenceError::ArtifactNotFound)?;
            let byte_length = u64::try_from(row.0).map_err(|_| {
                PersistenceError::InvalidRequest("negative artifact byte length".to_owned())
            })?;
            let content_hash = parse_content_hash(&row.1, row.2)?;
            let end =
                offset.saturating_add(u64::try_from(length).unwrap_or(u64::MAX)).min(byte_length);
            if offset >= end {
                return Ok(Vec::new());
            }
            if let Some(inline) = row.3 {
                if u64::try_from(inline.len()).ok() != Some(byte_length)
                    || ContentHash::blake3(&inline) != content_hash
                {
                    return Err(PersistenceError::ArtifactIntegrity);
                }
                let start = usize::try_from(offset).map_err(|_| {
                    PersistenceError::InvalidRequest("artifact offset exceeds usize".to_owned())
                })?;
                let end = usize::try_from(end).map_err(|_| {
                    PersistenceError::InvalidRequest("artifact end exceeds usize".to_owned())
                })?;
                return Ok(inline[start..end].to_vec());
            }
            read_chunk_range(connection, &artifact_root, artifact_id, offset, end)
        })
    }

    pub(crate) fn read_internal_artifact_all(
        &self,
        artifact_id: ArtifactId,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, PersistenceError> {
        let byte_length = self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT byte_length FROM artifacts WHERE artifact_id=?1",
                    [artifact_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(PersistenceError::from)
        })?;
        let byte_length =
            u64::try_from(byte_length).map_err(|_| PersistenceError::ArtifactIntegrity)?;
        if byte_length > maximum_bytes {
            return Err(PersistenceError::ArtifactIntegrity);
        }
        let capacity =
            usize::try_from(byte_length).map_err(|_| PersistenceError::ArtifactIntegrity)?;
        let mut bytes = Vec::with_capacity(capacity);
        let mut offset = 0_u64;
        while offset < byte_length {
            let remaining = byte_length - offset;
            let length = usize::try_from(remaining.min(ARTIFACT_RANGE_LIMIT as u64))
                .map_err(|_| PersistenceError::ArtifactIntegrity)?;
            let chunk = self.read_internal_artifact_range(artifact_id, offset, length)?;
            if chunk.len() != length {
                return Err(PersistenceError::ArtifactIntegrity);
            }
            bytes.extend_from_slice(&chunk);
            offset = offset
                .checked_add(
                    u64::try_from(length).map_err(|_| PersistenceError::ArtifactIntegrity)?,
                )
                .ok_or(PersistenceError::ArtifactIntegrity)?;
        }
        Ok(bytes)
    }

    /// Reads only a user-controlled Session attachment that explicitly grants
    /// ATTACH access. This is the narrow Core boundary used by Tool Draft
    /// archive import; it does not expose generic internal Artifact reads.
    pub fn read_user_attachable_artifact_all(
        &self,
        artifact_id: ArtifactId,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, PersistenceError> {
        self.read_user_attachable_artifact_exact(artifact_id, None, None, maximum_bytes)
    }

    /// Reads one exact user-controlled Session attachment after checking its
    /// immutable metadata. `expected_media_type` is used for typed transfer
    /// envelopes; child content Artifacts intentionally retain their authored
    /// media types and pass `None`.
    pub fn read_user_attachable_artifact_exact(
        &self,
        artifact_id: ArtifactId,
        expected_byte_length: Option<u64>,
        expected_media_type: Option<&str>,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, PersistenceError> {
        let expected_media_type = expected_media_type.map(str::to_owned);
        let authorized_length = self.with_reader(|connection| {
            let row = connection
                .query_row(
                    "SELECT a.scope, a.media_type, a.access_flags, a.byte_length,
                            o.owner_kind, o.owner_type, o.owner_id
                     FROM artifacts a
                     JOIN artifact_owners o ON o.artifact_id=a.artifact_id
                     WHERE a.artifact_id=?1",
                    [artifact_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, Option<String>>(6)?,
                        ))
                    },
                )
                .optional()?
                .ok_or(PersistenceError::ArtifactNotFound)?;
            let scope = parse_scope(&row.0)?;
            let access = parse_access(row.2)?;
            let owner = parse_owner(&row.4, &row.5, row.6.as_deref())?;
            let byte_length =
                u64::try_from(row.3).map_err(|_| PersistenceError::ArtifactIntegrity)?;
            if scope != ArtifactScope::Session
                || !matches!(owner, ArtifactOwnerRef::Session { .. })
                || !access.contains(ArtifactAccess::ATTACH)
            {
                return Err(PersistenceError::ArtifactAccessDenied);
            }
            if byte_length > maximum_bytes
                || expected_byte_length.is_some_and(|expected| expected != byte_length)
                || expected_media_type.as_ref().is_some_and(|expected| expected != &row.1)
            {
                return Err(PersistenceError::ArtifactIntegrity);
            }
            Ok(byte_length)
        })?;
        self.read_internal_artifact_all(artifact_id, authorized_length)
    }

    pub fn release_session_artifact(
        &self,
        artifact_id: ArtifactId,
        session_id: SessionId,
    ) -> Result<bool, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let direct_owner = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM artifact_owners
                    WHERE artifact_id=?1 AND owner_kind='session' AND owner_id=?2
                 )",
                params![artifact_id.to_string(), session_id.to_string()],
                |row| row.get::<_, bool>(0),
            )?;
            let released = if direct_owner {
                transaction.execute(
                    "INSERT OR IGNORE INTO artifact_gc_queue(
                        artifact_id, enqueued_at, reason
                     ) VALUES (?1, ?2, 'direct_session_owner_released')",
                    params![artifact_id.to_string(), UtcTimestamp::now().to_string()],
                )?;
                true
            } else {
                transaction.execute(
                    "DELETE FROM artifact_access_links WHERE artifact_id=?1 AND session_id=?2",
                    params![artifact_id.to_string(), session_id.to_string()],
                )? == 1
            };
            if !released {
                let exists = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM artifacts WHERE artifact_id=?1)",
                    [artifact_id.to_string()],
                    |row| row.get::<_, bool>(0),
                )?;
                if !exists {
                    return Err(PersistenceError::ArtifactNotFound);
                }
            }
            transaction.commit()?;
            Ok(released)
        })
    }

    pub fn enqueue_artifact_gc_if_unreferenced(
        &self,
        artifact_id: ArtifactId,
        reason: &'static str,
    ) -> Result<bool, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let exists = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM artifacts WHERE artifact_id=?1)",
                [artifact_id.to_string()],
                |row| row.get::<_, bool>(0),
            )?;
            if !exists {
                return Err(PersistenceError::ArtifactNotFound);
            }
            let session_links = transaction.query_row(
                "SELECT COUNT(*) FROM artifact_access_links WHERE artifact_id=?1",
                [artifact_id.to_string()],
                |row| row.get::<_, i64>(0),
            )?;
            let quarantine_links = transaction.query_row(
                "SELECT COUNT(*) FROM quarantine_records
                 WHERE artifact_id=?1 AND state!='discarded'",
                [artifact_id.to_string()],
                |row| row.get::<_, i64>(0),
            )?;
            let queued = session_links == 0 && quarantine_links == 0;
            if queued {
                transaction.execute(
                    "INSERT OR IGNORE INTO artifact_gc_queue(artifact_id, enqueued_at, reason)
                     VALUES (?1, ?2, ?3)",
                    params![artifact_id.to_string(), UtcTimestamp::now().to_string(), reason],
                )?;
            }
            transaction.commit()?;
            Ok(queued)
        })
    }

    pub fn run_artifact_gc(&self, limit: usize) -> Result<ArtifactGcReport, PersistenceError> {
        let limit = limit.clamp(1, 100);
        let artifact_root = self.artifact_root().to_path_buf();
        let (artifacts_removed, objects) = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let artifact_ids = {
                let mut statement = transaction.prepare(
                    "SELECT artifact_id FROM artifact_gc_queue ORDER BY enqueued_at LIMIT ?1",
                )?;
                statement
                    .query_map([i64::try_from(limit).unwrap_or(100)], |row| {
                        row.get::<_, String>(0)
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let mut unreferenced = Vec::<PathBuf>::new();
            for artifact_id in &artifact_ids {
                let object_hashes = {
                    let mut statement = transaction
                        .prepare("SELECT object_hash FROM artifact_chunks WHERE artifact_id=?1")?;
                    statement
                        .query_map([artifact_id], |row| row.get::<_, String>(0))?
                        .collect::<Result<Vec<_>, _>>()?
                };
                transaction.execute("DELETE FROM artifacts WHERE artifact_id=?1", [artifact_id])?;
                for object_hash in object_hashes {
                    transaction.execute(
                        "UPDATE artifact_object_refs
                         SET ref_count=ref_count-1 WHERE object_hash=?1",
                        [&object_hash],
                    )?;
                    let remaining = transaction
                        .query_row(
                            "SELECT ref_count FROM artifact_object_refs WHERE object_hash=?1",
                            [&object_hash],
                            |row| row.get::<_, i64>(0),
                        )
                        .optional()?;
                    if remaining == Some(0) {
                        let relative = transaction.query_row(
                            "SELECT relative_path FROM artifact_object_refs WHERE object_hash=?1",
                            [&object_hash],
                            |row| row.get::<_, String>(0),
                        )?;
                        let object_path =
                            resolve_object_path(&artifact_root, &relative, &object_hash)?;
                        transaction.execute(
                            "DELETE FROM artifact_object_refs WHERE object_hash=?1",
                            [&object_hash],
                        )?;
                        unreferenced.push(object_path);
                    }
                }
            }
            transaction.commit()?;
            Ok((artifact_ids.len(), unreferenced))
        })?;
        let mut objects_removed = 0;
        let mut orphan_files_remaining = 0;
        for path in objects {
            match fs::remove_file(&path) {
                Ok(()) => objects_removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => orphan_files_remaining += 1,
            }
        }
        Ok(ArtifactGcReport { artifacts_removed, objects_removed, orphan_files_remaining })
    }

    pub fn artifact_count(&self) -> Result<u64, PersistenceError> {
        self.with_reader(|connection| {
            let count = connection
                .query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get::<_, i64>(0))?;
            u64::try_from(count)
                .map_err(|_| PersistenceError::InvalidRequest("negative artifact count".to_owned()))
        })
    }

    pub fn artifact_object_count(&self) -> Result<u64, PersistenceError> {
        self.with_reader(|connection| {
            let count =
                connection.query_row("SELECT COUNT(*) FROM artifact_object_refs", [], |row| {
                    row.get::<_, i64>(0)
                })?;
            u64::try_from(count).map_err(|_| {
                PersistenceError::InvalidRequest("negative artifact object count".to_owned())
            })
        })
    }
}

pub(crate) fn insert_artifact_in_transaction(
    transaction: &Transaction<'_>,
    artifact_root: &Path,
    artifact_id: ArtifactId,
    request: ArtifactWriteRequest,
) -> Result<ArtifactRecord, PersistenceError> {
    let created_at = UtcTimestamp::now();
    insert_artifact_at_in_transaction(transaction, artifact_root, artifact_id, request, created_at)
}

pub(crate) fn insert_artifact_write_draft_in_transaction(
    transaction: &Transaction<'_>,
    artifact_root: &Path,
    draft: ArtifactWriteDraftV1,
) -> Result<ArtifactRecord, PersistenceError> {
    draft.record()?;
    insert_artifact_at_in_transaction(
        transaction,
        artifact_root,
        draft.artifact_id,
        draft.request,
        draft.created_at,
    )
}

fn insert_artifact_at_in_transaction(
    transaction: &Transaction<'_>,
    artifact_root: &Path,
    artifact_id: ArtifactId,
    request: ArtifactWriteRequest,
    created_at: UtcTimestamp,
) -> Result<ArtifactRecord, PersistenceError> {
    validate_write_request(&request)?;
    let byte_length = u64::try_from(request.bytes.len())
        .map_err(|_| PersistenceError::InvalidRequest("artifact exceeds u64 length".to_owned()))?;
    let content_hash = ContentHash::blake3(&request.bytes);
    let inline = request.bytes.len() <= ARTIFACT_INLINE_LIMIT;
    let chunks = if inline {
        Vec::new()
    } else {
        prepare_chunks(artifact_root, &request.bytes, &request.media_type)?
    };
    let record = ArtifactRecord {
        artifact_id,
        owner: request.owner.clone(),
        scope: request.scope,
        media_type: request.media_type.clone(),
        access: request.access,
        byte_length,
        content_hash: content_hash.clone(),
        inline,
        created_at,
    };
    transaction.execute(
        "INSERT INTO artifacts(
            artifact_id, scope, media_type, access_flags, byte_length,
            content_hash_algorithm, content_hash_hex, inline_bytes, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'blake3', ?6, ?7, ?8)",
        params![
            artifact_id.to_string(),
            scope_name(request.scope),
            request.media_type,
            i64::from(request.access.bits()),
            i64::try_from(byte_length).map_err(|_| {
                PersistenceError::InvalidRequest("artifact exceeds SQLite integer range".to_owned())
            })?,
            content_hash.hex,
            if inline { Some(request.bytes) } else { None },
            created_at.to_string()
        ],
    )?;
    let (owner_kind, owner_type, owner_id) = owner_parts(&request.owner)?;
    transaction.execute(
        "INSERT INTO artifact_owners(
            artifact_id, owner_kind, owner_type, owner_id, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![artifact_id.to_string(), owner_kind, owner_type, owner_id, created_at.to_string()],
    )?;
    for chunk in chunks {
        transaction.execute(
            "INSERT INTO artifact_object_refs(
                object_hash, relative_path, stored_length, ref_count
             ) VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(object_hash) DO UPDATE SET ref_count=ref_count+1",
            params![
                chunk.object_hash,
                chunk.relative_path,
                i64::try_from(chunk.stored_length).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "artifact chunk exceeds SQLite integer range".to_owned(),
                    )
                })?
            ],
        )?;
        transaction.execute(
            "INSERT INTO artifact_chunks(
                artifact_id, chunk_index, raw_offset, raw_length, stored_length,
                compression, raw_hash, object_hash
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                artifact_id.to_string(),
                i64::try_from(chunk.chunk_index).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "artifact chunk index exceeds SQLite range".to_owned(),
                    )
                })?,
                i64::try_from(chunk.raw_offset).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "artifact offset exceeds SQLite range".to_owned(),
                    )
                })?,
                i64::try_from(chunk.raw_length).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "artifact length exceeds SQLite range".to_owned(),
                    )
                })?,
                i64::try_from(chunk.stored_length).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "stored artifact length exceeds SQLite range".to_owned(),
                    )
                })?,
                chunk.compression,
                chunk.raw_hash,
                chunk.object_hash
            ],
        )?;
    }
    Ok(record)
}

pub(crate) fn insert_artifact_access_link_in_transaction(
    transaction: &Transaction<'_>,
    link: &ArtifactAccessLinkV1,
) -> Result<(), PersistenceError> {
    link.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    let ArtifactAccessSubjectV1::Session { session_id } = &link.subject else {
        return Err(PersistenceError::InvalidRequest(
            "this persistence version accepts only Session artifact access links".to_owned(),
        ));
    };
    let row = transaction
        .query_row(
            "SELECT a.scope, a.access_flags, o.owner_kind, o.owner_type
             FROM artifacts AS a
             JOIN artifact_owners AS o USING (artifact_id)
             WHERE a.artifact_id=?1",
            [link.artifact_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ArtifactNotFound)?;
    let artifact_access = parse_access(row.1)?;
    if row.0 != "tool_execution"
        || row.2 != "tool"
        || !matches!(row.3.as_str(), "tool_call" | "execution_attempt")
        || !artifact_access.contains(link.access)
    {
        return Err(PersistenceError::ArtifactAccessDenied);
    }
    transaction.execute(
        "INSERT INTO artifact_access_links(
            artifact_id, session_id, access_flags, created_at, evidence_hash_json
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            link.artifact_id.to_string(),
            session_id.to_string(),
            i64::from(link.access.bits()),
            link.created_at.to_string(),
            serde_json::to_string(&link.evidence_hash).map_err(|_| {
                PersistenceError::InvalidRequest(
                    "artifact access evidence hash cannot be serialized".to_owned(),
                )
            })?,
        ],
    )?;
    Ok(())
}

fn validate_write_request(request: &ArtifactWriteRequest) -> Result<(), PersistenceError> {
    if request.media_type.trim().is_empty() {
        return Err(PersistenceError::InvalidRequest(
            "artifact media_type must not be empty".to_owned(),
        ));
    }
    let compatible_scope = match &request.owner {
        ArtifactOwnerRef::Session { .. } => matches!(
            request.scope,
            ArtifactScope::Session
                | ArtifactScope::RuntimeEphemeral
                | ArtifactScope::WorkspaceCache
                | ArtifactScope::Quarantine
        ),
        ArtifactOwnerRef::Operation { .. } => {
            matches!(request.scope, ArtifactScope::SystemCache | ArtifactScope::RuntimeEphemeral)
        }
        ArtifactOwnerRef::System { .. } => request.scope == ArtifactScope::SystemCache,
        ArtifactOwnerRef::Tool { owner } => match owner {
            ToolArtifactOwnerRef::PackageVersion { .. }
            | ToolArtifactOwnerRef::RegistrySnapshot { .. } => {
                request.scope == ArtifactScope::PackageStore
            }
            ToolArtifactOwnerRef::Draft { .. } => matches!(
                request.scope,
                ArtifactScope::Quarantine
                    | ArtifactScope::ToolExecution
                    | ArtifactScope::RuntimeEphemeral
            ),
            ToolArtifactOwnerRef::ConformanceRun { .. } => {
                matches!(request.scope, ArtifactScope::Quarantine | ArtifactScope::ToolExecution)
            }
            ToolArtifactOwnerRef::Batch { .. }
            | ToolArtifactOwnerRef::ToolCall { .. }
            | ToolArtifactOwnerRef::ExecutionAttempt { .. } => matches!(
                request.scope,
                ArtifactScope::ToolExecution | ArtifactScope::RuntimeEphemeral
            ),
        },
    };
    if !compatible_scope {
        return Err(PersistenceError::InvalidRequest(
            "artifact owner and scope are incompatible".to_owned(),
        ));
    }
    let externally_reusable_access =
        ArtifactAccess::ATTACH.union(ArtifactAccess::EXPORT).union(ArtifactAccess::SEARCH);
    if matches!(
        &request.owner,
        ArtifactOwnerRef::Operation { .. }
            | ArtifactOwnerRef::System { .. }
            | ArtifactOwnerRef::Tool { .. }
    ) && request.access.intersects(externally_reusable_access)
    {
        return Err(PersistenceError::InvalidRequest(
            "operation, system, and tool artifacts cannot grant attach, export, or search access"
                .to_owned(),
        ));
    }
    Ok(())
}

fn prepare_chunks(
    root: &Path,
    bytes: &[u8],
    media_type: &str,
) -> Result<Vec<PreparedChunk>, PersistenceError> {
    let compressible = media_type.starts_with("text/") || media_type == "application/json";
    let mut prepared = Vec::new();
    for (chunk_index, raw) in bytes.chunks(ARTIFACT_CHUNK_SIZE).enumerate() {
        let compressed = if compressible {
            Some(zstd::stream::encode_all(Cursor::new(raw), 3).map_err(|source| {
                PersistenceError::Io {
                    action: "compress artifact chunk",
                    path: root.to_path_buf(),
                    source,
                }
            })?)
        } else {
            None
        };
        let (stored, compression) = match compressed {
            Some(compressed) if compressed.len() < raw.len() => (compressed, "zstd"),
            _ => (raw.to_vec(), "none"),
        };
        let raw_hash = blake3::hash(raw).to_hex().to_string();
        let object_hash = blake3::hash(&stored).to_hex().to_string();
        let relative_path = format!("{}/{}.obj", &object_hash[..2], object_hash);
        persist_object(root, &relative_path, &stored, &object_hash)?;
        prepared.push(PreparedChunk {
            chunk_index,
            raw_offset: chunk_index * ARTIFACT_CHUNK_SIZE,
            raw_length: raw.len(),
            stored_length: stored.len(),
            compression,
            raw_hash,
            object_hash,
            relative_path,
        });
    }
    Ok(prepared)
}

fn persist_object(
    root: &Path,
    relative_path: &str,
    bytes: &[u8],
    expected_hash: &str,
) -> Result<(), PersistenceError> {
    let path = resolve_object_path(root, relative_path, expected_hash)?;
    if path.exists() {
        return verify_stored_object(&path, expected_hash);
    }
    let parent = path.parent().expect("artifact object path has a parent");
    fs::create_dir_all(parent).map_err(|source| PersistenceError::Io {
        action: "create artifact object shard",
        path: parent.to_path_buf(),
        source,
    })?;
    let temporary = parent.join(format!(".object-{}.tmp", Ulid::new()));
    let mut file =
        OpenOptions::new().create_new(true).write(true).open(&temporary).map_err(|source| {
            PersistenceError::Io {
                action: "create artifact object temporary",
                path: temporary.clone(),
                source,
            }
        })?;
    file.write_all(bytes).and_then(|()| file.flush()).and_then(|()| file.sync_all()).map_err(
        |source| PersistenceError::Io {
            action: "flush artifact object temporary",
            path: temporary.clone(),
            source,
        },
    )?;
    drop(file);
    match fs::rename(&temporary, &path) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
            ) && path.exists() =>
        {
            let _ = fs::remove_file(&temporary);
            verify_stored_object(&path, expected_hash)
        }
        Err(source) => {
            Err(PersistenceError::Io { action: "publish artifact object", path, source })
        }
    }
}

fn verify_stored_object(path: &Path, expected_hash: &str) -> Result<(), PersistenceError> {
    let bytes = fs::read(path).map_err(|source| PersistenceError::Io {
        action: "read existing artifact object",
        path: path.to_path_buf(),
        source,
    })?;
    if blake3::hash(&bytes).to_hex().as_str() != expected_hash {
        return Err(PersistenceError::ArtifactIntegrity);
    }
    Ok(())
}

fn authorize_artifact(
    artifact: &ArtifactRow,
    request: &ArtifactReadRequest,
) -> Result<(), PersistenceError> {
    if artifact.scope == ArtifactScope::Quarantine && !request.allow_quarantine {
        return Err(PersistenceError::QuarantineAccessRequired);
    }
    let direct_owner_access = match (&artifact.owner, request.session_id) {
        (ArtifactOwnerRef::Session { session_id }, Some(request_session_id)) => {
            *session_id == request_session_id
        }
        (ArtifactOwnerRef::System { owner: ArtifactSystemOwner::ProviderCatalogCache }, None) => {
            true
        }
        _ => false,
    };
    if !direct_owner_access {
        let link_access = artifact.link_access.ok_or(PersistenceError::ArtifactAccessDenied)?;
        if !link_access.contains(request.required_access) {
            return Err(PersistenceError::ArtifactAccessDenied);
        }
    }
    if !artifact.access.contains(request.required_access) {
        return Err(PersistenceError::ArtifactAccessDenied);
    }
    Ok(())
}

fn read_chunk_range(
    connection: &rusqlite::Connection,
    root: &Path,
    artifact_id: ArtifactId,
    start: u64,
    end: u64,
) -> Result<Vec<u8>, PersistenceError> {
    let start_i64 = i64::try_from(start)
        .map_err(|_| PersistenceError::InvalidRequest("range start exceeds SQLite".to_owned()))?;
    let end_i64 = i64::try_from(end)
        .map_err(|_| PersistenceError::InvalidRequest("range end exceeds SQLite".to_owned()))?;
    let mut statement = connection.prepare(
        "SELECT c.raw_offset, c.raw_length, c.compression, c.raw_hash,
                c.object_hash, o.relative_path
         FROM artifact_chunks c
         JOIN artifact_object_refs o ON o.object_hash=c.object_hash
         WHERE c.artifact_id=?1
           AND c.raw_offset < ?3
           AND c.raw_offset + c.raw_length > ?2
         ORDER BY c.chunk_index",
    )?;
    let rows = statement
        .query_map(params![artifact_id.to_string(), start_i64, end_i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let capacity = usize::try_from(end - start)
        .map_err(|_| PersistenceError::InvalidRequest("range capacity exceeds usize".to_owned()))?;
    let mut result = Vec::with_capacity(capacity);
    for (raw_offset, raw_length, compression, raw_hash, object_hash, relative_path) in rows {
        let path = resolve_object_path(root, &relative_path, &object_hash)?;
        let stored = fs::read(&path).map_err(|source| PersistenceError::Io {
            action: "read artifact object",
            path: path.clone(),
            source,
        })?;
        if blake3::hash(&stored).to_hex().as_str() != object_hash {
            return Err(PersistenceError::ArtifactIntegrity);
        }
        let raw = match compression.as_str() {
            "none" => stored,
            "zstd" => zstd::stream::decode_all(Cursor::new(stored)).map_err(|source| {
                PersistenceError::Io {
                    action: "decompress artifact chunk",
                    path: path.clone(),
                    source,
                }
            })?,
            _ => return Err(PersistenceError::ArtifactIntegrity),
        };
        if i64::try_from(raw.len()).ok() != Some(raw_length)
            || blake3::hash(&raw).to_hex().as_str() != raw_hash
        {
            return Err(PersistenceError::ArtifactIntegrity);
        }
        let chunk_start =
            u64::try_from(raw_offset).map_err(|_| PersistenceError::ArtifactIntegrity)?;
        let chunk_end = chunk_start.saturating_add(
            u64::try_from(raw.len()).map_err(|_| PersistenceError::ArtifactIntegrity)?,
        );
        let slice_start = usize::try_from(start.saturating_sub(chunk_start))
            .map_err(|_| PersistenceError::ArtifactIntegrity)?;
        let slice_end = usize::try_from(end.min(chunk_end).saturating_sub(chunk_start))
            .map_err(|_| PersistenceError::ArtifactIntegrity)?;
        if slice_start > slice_end || slice_end > raw.len() {
            return Err(PersistenceError::ArtifactIntegrity);
        }
        result.extend_from_slice(&raw[slice_start..slice_end]);
    }
    if result.len() != capacity {
        return Err(PersistenceError::ArtifactIntegrity);
    }
    Ok(result)
}

fn scope_name(scope: ArtifactScope) -> &'static str {
    match scope {
        ArtifactScope::Session => "session",
        ArtifactScope::RuntimeEphemeral => "runtime_ephemeral",
        ArtifactScope::WorkspaceCache => "workspace_cache",
        ArtifactScope::SystemCache => "system_cache",
        ArtifactScope::Quarantine => "quarantine",
        ArtifactScope::PackageStore => "package_store",
        ArtifactScope::ToolExecution => "tool_execution",
    }
}

fn parse_scope(value: &str) -> Result<ArtifactScope, PersistenceError> {
    match value {
        "session" => Ok(ArtifactScope::Session),
        "runtime_ephemeral" => Ok(ArtifactScope::RuntimeEphemeral),
        "workspace_cache" => Ok(ArtifactScope::WorkspaceCache),
        "system_cache" => Ok(ArtifactScope::SystemCache),
        "quarantine" => Ok(ArtifactScope::Quarantine),
        "package_store" => Ok(ArtifactScope::PackageStore),
        "tool_execution" => Ok(ArtifactScope::ToolExecution),
        _ => Err(PersistenceError::InvalidRequest("unknown persisted artifact scope".to_owned())),
    }
}

fn parse_access(value: i64) -> Result<ArtifactAccess, PersistenceError> {
    let bits = u32::try_from(value).map_err(|_| {
        PersistenceError::InvalidRequest("invalid artifact access flags".to_owned())
    })?;
    ArtifactAccess::from_bits(bits)
        .ok_or_else(|| PersistenceError::InvalidRequest("unknown artifact access flags".to_owned()))
}

fn owner_parts(
    owner: &ArtifactOwnerRef,
) -> Result<(&'static str, &'static str, Option<String>), PersistenceError> {
    match owner {
        ArtifactOwnerRef::Session { session_id } => {
            Ok(("session", "session", Some(session_id.to_string())))
        }
        ArtifactOwnerRef::Operation {
            operation: ArtifactOperationOwnerRef::ModelDiscovery { model_discovery_run_id },
        } => Ok(("operation", "model_discovery", Some(model_discovery_run_id.to_string()))),
        ArtifactOwnerRef::Operation {
            operation:
                ArtifactOperationOwnerRef::ModelAvailabilityTest { model_availability_test_id },
        } => Ok((
            "operation",
            "model_availability_test",
            Some(model_availability_test_id.to_string()),
        )),
        ArtifactOwnerRef::System { owner: ArtifactSystemOwner::ProviderCatalogCache } => {
            Ok(("system", "provider_catalog_cache", None))
        }
        ArtifactOwnerRef::Tool { owner } => Ok((
            "tool",
            tool_owner_type(owner),
            Some(
                String::from_utf8(aura_domain_common::jcs::canonical_json_jcs(owner).map_err(
                    |_| {
                        PersistenceError::InvalidRequest(
                            "tool artifact owner cannot be canonically serialized".to_owned(),
                        )
                    },
                )?)
                .map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "tool artifact owner canonical form is not UTF-8".to_owned(),
                    )
                })?,
            ),
        )),
    }
}

fn tool_owner_type(owner: &ToolArtifactOwnerRef) -> &'static str {
    match owner {
        ToolArtifactOwnerRef::PackageVersion { .. } => "package_version",
        ToolArtifactOwnerRef::Draft { .. } => "draft",
        ToolArtifactOwnerRef::ConformanceRun { .. } => "conformance_run",
        ToolArtifactOwnerRef::Batch { .. } => "batch",
        ToolArtifactOwnerRef::ToolCall { .. } => "tool_call",
        ToolArtifactOwnerRef::ExecutionAttempt { .. } => "execution_attempt",
        ToolArtifactOwnerRef::RegistrySnapshot { .. } => "registry_snapshot",
    }
}

pub(crate) fn parse_owner(
    owner_kind: &str,
    owner_type: &str,
    owner_id: Option<&str>,
) -> Result<ArtifactOwnerRef, PersistenceError> {
    let invalid =
        || PersistenceError::InvalidRequest("invalid persisted artifact direct owner".to_owned());
    match (owner_kind, owner_type, owner_id) {
        ("session", "session", Some(owner_id)) => Ok(ArtifactOwnerRef::Session {
            session_id: SessionId::from_str(owner_id).map_err(|_| invalid())?,
        }),
        ("operation", "model_discovery", Some(owner_id)) => Ok(ArtifactOwnerRef::Operation {
            operation: ArtifactOperationOwnerRef::ModelDiscovery {
                model_discovery_run_id: ModelDiscoveryRunId::from_str(owner_id)
                    .map_err(|_| invalid())?,
            },
        }),
        ("operation", "model_availability_test", Some(owner_id)) => {
            Ok(ArtifactOwnerRef::Operation {
                operation: ArtifactOperationOwnerRef::ModelAvailabilityTest {
                    model_availability_test_id: ModelAvailabilityTestId::from_str(owner_id)
                        .map_err(|_| invalid())?,
                },
            })
        }
        ("system", "provider_catalog_cache", None) => {
            Ok(ArtifactOwnerRef::System { owner: ArtifactSystemOwner::ProviderCatalogCache })
        }
        ("tool", persisted_type, Some(owner_json)) => {
            let owner =
                serde_json::from_str::<ToolArtifactOwnerRef>(owner_json).map_err(|_| invalid())?;
            if tool_owner_type(&owner) != persisted_type {
                return Err(invalid());
            }
            let canonical =
                aura_domain_common::jcs::canonical_json_jcs(&owner).map_err(|_| invalid())?;
            if canonical.as_slice() != owner_json.as_bytes() {
                return Err(invalid());
            }
            Ok(ArtifactOwnerRef::Tool { owner })
        }
        _ => Err(invalid()),
    }
}

fn parse_content_hash(algorithm: &str, value: String) -> Result<ContentHash, PersistenceError> {
    let algorithm = match algorithm {
        "blake3" => ContentHashAlgorithm::Blake3,
        _ => return Err(PersistenceError::ArtifactIntegrity),
    };
    ContentHash::new(algorithm, value).map_err(|_| PersistenceError::ArtifactIntegrity)
}

fn resolve_object_path(
    root: &Path,
    relative_path: &str,
    object_hash: &str,
) -> Result<PathBuf, PersistenceError> {
    if object_hash.len() != 64
        || !object_hash.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(PersistenceError::ArtifactIntegrity);
    }
    let expected = format!("{}/{}.obj", &object_hash[..2], object_hash);
    if relative_path != expected {
        return Err(PersistenceError::ArtifactIntegrity);
    }
    Ok(root.join(expected))
}

#[cfg(test)]
mod tests {
    use aura_artifact_contract::{
        ArtifactAccess, ArtifactOwnerRef, ArtifactScope, ToolArtifactOwnerRef,
    };
    use aura_domain_common::CanonicalSemVer;
    use aura_domain_common::jcs::CanonicalHashV1;
    use aura_domain_common::{ArtifactId, ContentHash, SessionId};
    use aura_domain_common::{
        ToolBatchId, ToolCallId, ToolConformanceRunId, ToolDraftId, ToolExecutionAttemptId,
        ToolPackageId, ToolRegistrySnapshotId,
    };

    use super::{
        ArtifactReadRequest, ArtifactRow, ArtifactWriteRequest, authorize_artifact, owner_parts,
        parse_owner, validate_write_request,
    };

    #[test]
    fn every_tool_artifact_owner_round_trips_through_canonical_storage() {
        let owners = [
            ToolArtifactOwnerRef::PackageVersion {
                tool_package_id: ToolPackageId::new(),
                package_version: CanonicalSemVer::new("1.2.3").expect("canonical version"),
                package_fingerprint: CanonicalHashV1::sha256(b"package"),
            },
            ToolArtifactOwnerRef::Draft { tool_draft_id: ToolDraftId::new() },
            ToolArtifactOwnerRef::ConformanceRun {
                tool_conformance_run_id: ToolConformanceRunId::new(),
            },
            ToolArtifactOwnerRef::Batch { tool_batch_id: ToolBatchId::new() },
            ToolArtifactOwnerRef::ToolCall { tool_call_id: ToolCallId::new() },
            ToolArtifactOwnerRef::ExecutionAttempt {
                tool_execution_attempt_id: ToolExecutionAttemptId::new(),
            },
            ToolArtifactOwnerRef::RegistrySnapshot {
                tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
            },
        ];

        for tool_owner in owners {
            let owner = ArtifactOwnerRef::Tool { owner: tool_owner };
            let (kind, owner_type, payload) = owner_parts(&owner).expect("owner storage parts");
            let payload = payload.expect("tool owner payload");
            assert_eq!(kind, "tool");
            assert_eq!(
                parse_owner(kind, owner_type, Some(&payload)).expect("parse stored tool owner"),
                owner
            );
            assert!(parse_owner(kind, owner_type, Some(&format!(" {payload}"))).is_err());
        }
    }

    #[test]
    fn tool_result_read_access_is_only_a_ceiling_until_a_session_link_exists() {
        let request = ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/json".to_owned(),
            access: ArtifactAccess::USER_READ.union(ArtifactAccess::MODEL_READ),
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ExecutionAttempt {
                    tool_execution_attempt_id: ToolExecutionAttemptId::new(),
                },
            },
            bytes: b"{}".to_vec(),
        };
        validate_write_request(&request).expect("Tool result access ceiling");
        let session_id = SessionId::new();
        let mut row = ArtifactRow {
            owner: request.owner,
            scope: request.scope,
            access: request.access,
            byte_length: 2,
            content_hash: ContentHash::blake3(b"{}"),
            inline_bytes: Some(request.bytes),
            link_access: None,
        };
        let read = ArtifactReadRequest {
            artifact_id: ArtifactId::new(),
            session_id: Some(session_id),
            required_access: ArtifactAccess::MODEL_READ,
            offset: 0,
            length: 2,
            allow_quarantine: false,
        };
        assert!(matches!(
            authorize_artifact(&row, &read),
            Err(super::PersistenceError::ArtifactAccessDenied)
        ));
        row.link_access = Some(ArtifactAccess::MODEL_READ);
        authorize_artifact(&row, &read).expect("exact Session access link");
    }
}
