use aura_domain_common::jcs::{
    CanonicalHashV1, canonical_json_jcs, canonical_sha256, parse_json_strict,
};
use aura_domain_common::{ContentHash, Revision, UtcTimestamp};
use aura_tool_contract::{ToolRegistrySnapshotRef, ToolRegistrySnapshotV1, ToolSearchCursorV1};
use aura_tool_registry::{
    ToolSearchCorpusReadError, ToolSearchCorpusStore, ToolSearchCorpusV1,
    ToolSearchCursorBindingRecordV1, ToolSearchCursorStore, ToolSearchCursorStoreError,
    ToolSearchDocumentV1,
};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::de::DeserializeOwned;

use crate::{PersistenceCoordinator, PersistenceError};

const TOOL_SEARCH_CURSOR_RANDOM_BYTES: usize = 32;
const TOOL_SEARCH_CURSOR_PREFIX: &str = "tsc1.";

impl PersistenceCoordinator {
    pub fn store_tool_registry_search_corpus(
        &self,
        corpus: ToolSearchCorpusV1,
    ) -> Result<(), PersistenceError> {
        corpus.snapshot.validate().map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            insert_tool_registry_search_corpus(&transaction, &corpus)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn read_tool_registry_search_corpus(
        &self,
        snapshot_ref: &ToolRegistrySnapshotRef,
    ) -> Result<Option<ToolSearchCorpusV1>, PersistenceError> {
        let snapshot_id = snapshot_ref.tool_registry_snapshot_id.to_string();
        let expected_revision = sqlite_revision(snapshot_ref.registry_revision)?;
        let expected_hash = snapshot_ref.content_hash.clone();
        self.with_reader(move |connection| {
            let row = connection
                .query_row(
                    "SELECT registry_revision, content_hash_json, snapshot_json,
                            search_documents_json, search_documents_hash_json,
                            snapshot_contract_version
                     FROM tool_registry_snapshots
                     WHERE tool_registry_snapshot_id=?1",
                    [snapshot_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, i64>(5)?,
                        ))
                    },
                )
                .optional()?;
            let Some((
                revision,
                hash_json,
                snapshot_json,
                documents_json,
                documents_hash_json,
                snapshot_contract_version,
            )) = row
            else {
                return Ok(None);
            };
            if snapshot_contract_version != 2 {
                // Historical v1 snapshots remain immutable evidence but may
                // never drive a new search/load/dispatch decision.
                return Ok(None);
            }
            if revision != expected_revision
                || decode_strict::<CanonicalHashV1>(&hash_json)? != expected_hash
            {
                return Err(PersistenceError::ToolRegistryIntegrity);
            }
            let (Some(documents_json), Some(documents_hash_json)) =
                (documents_json, documents_hash_json)
            else {
                return Ok(None);
            };
            let snapshot = decode_strict::<ToolRegistrySnapshotV1>(&snapshot_json)
                .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
            let documents = decode_strict::<Vec<ToolSearchDocumentV1>>(&documents_json)
                .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
            let persisted_documents_hash = decode_strict::<CanonicalHashV1>(&documents_hash_json)
                .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
            let actual_documents_hash = canonical_sha256(&documents)
                .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
            if snapshot.snapshot_ref != *snapshot_ref
                || persisted_documents_hash != actual_documents_hash
            {
                return Err(PersistenceError::ToolRegistryIntegrity);
            }
            ToolSearchCorpusV1::new(snapshot, documents)
                .map(Some)
                .map_err(|_| PersistenceError::ToolRegistryIntegrity)
        })
    }

    pub fn retire_tool_registry_search_corpus(
        &self,
        snapshot_ref: &ToolRegistrySnapshotRef,
        retired_at: UtcTimestamp,
    ) -> Result<bool, PersistenceError> {
        let snapshot_id = snapshot_ref.tool_registry_snapshot_id.to_string();
        let revision = sqlite_revision(snapshot_ref.registry_revision)?;
        let hash_json = encode_canonical(&snapshot_ref.content_hash)?;
        self.dispatch(move |connection| {
            let changed = connection.execute(
                "UPDATE tool_registry_snapshots
                 SET search_documents_json=NULL,
                     search_documents_hash_json=NULL,
                     search_corpus_retired_at=?1
                 WHERE tool_registry_snapshot_id=?2
                   AND registry_revision=?3
                   AND content_hash_json=?4
                   AND search_documents_json IS NOT NULL",
                params![retired_at.to_string(), snapshot_id, revision, hash_json],
            )?;
            Ok(changed == 1)
        })
    }

    pub fn issue_tool_search_cursor(
        &self,
        binding: ToolSearchCursorBindingRecordV1,
    ) -> Result<ToolSearchCursorV1, PersistenceError> {
        binding.validate().map_err(|_| PersistenceError::ToolSearchCursorIntegrity)?;
        let mut random = [0_u8; TOOL_SEARCH_CURSOR_RANDOM_BYTES];
        getrandom::fill(&mut random).map_err(|source| PersistenceError::Io {
            action: "generate Tool Search Cursor",
            path: self.database_path().to_path_buf(),
            source: std::io::Error::other(source),
        })?;
        let token = format!("{TOOL_SEARCH_CURSOR_PREFIX}{}", lower_hex(&random));
        let cursor = ToolSearchCursorV1::new(token.clone())
            .map_err(|_| PersistenceError::ToolSearchCursorIntegrity)?;
        let token_hash = ContentHash::sha256(token.as_bytes()).hex;
        let snapshot_id = binding.registry_snapshot_ref.tool_registry_snapshot_id.to_string();
        let revision = sqlite_revision(binding.registry_snapshot_ref.registry_revision)?;
        let content_hash_json = encode_canonical(&binding.registry_snapshot_ref.content_hash)?;
        let binding_json = encode_canonical(&binding)?;
        let binding_hash_json = encode_canonical(
            &canonical_sha256(&binding).map_err(|_| PersistenceError::ToolSearchCursorIntegrity)?,
        )?;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let source_exists = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM tool_registry_snapshots
                    WHERE tool_registry_snapshot_id=?1
                      AND registry_revision=?2
                      AND content_hash_json=?3
                      AND search_documents_json IS NOT NULL
                )",
                params![snapshot_id, revision, content_hash_json],
                |row| row.get::<_, bool>(0),
            )?;
            if !source_exists {
                return Err(PersistenceError::ToolRegistryIntegrity);
            }
            let inserted = transaction.execute(
                "INSERT OR IGNORE INTO tool_search_cursor_bindings(
                    cursor_token_hash, tool_registry_snapshot_id, binding_json,
                    binding_hash_json, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    token_hash,
                    binding.registry_snapshot_ref.tool_registry_snapshot_id.to_string(),
                    binding_json,
                    binding_hash_json,
                    UtcTimestamp::now().to_string(),
                ],
            )?;
            if inserted != 1 {
                return Err(PersistenceError::ToolSearchCursorIntegrity);
            }
            transaction.commit()?;
            Ok(cursor)
        })
    }

    pub fn resolve_tool_search_cursor(
        &self,
        cursor: &ToolSearchCursorV1,
    ) -> Result<Option<ToolSearchCursorBindingRecordV1>, PersistenceError> {
        let token_hash = ContentHash::sha256(cursor.as_str().as_bytes()).hex;
        self.with_reader(move |connection| {
            let row = connection
                .query_row(
                    "SELECT tool_registry_snapshot_id, binding_json, binding_hash_json
                     FROM tool_search_cursor_bindings
                     WHERE cursor_token_hash=?1",
                    [token_hash],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?;
            let Some((snapshot_id, binding_json, binding_hash_json)) = row else {
                return Ok(None);
            };
            let binding = decode_strict::<ToolSearchCursorBindingRecordV1>(&binding_json)
                .map_err(|_| PersistenceError::ToolSearchCursorIntegrity)?;
            let persisted_hash = decode_strict::<CanonicalHashV1>(&binding_hash_json)
                .map_err(|_| PersistenceError::ToolSearchCursorIntegrity)?;
            let actual_hash = canonical_sha256(&binding)
                .map_err(|_| PersistenceError::ToolSearchCursorIntegrity)?;
            if binding.registry_snapshot_ref.tool_registry_snapshot_id.to_string() != snapshot_id
                || persisted_hash != actual_hash
                || binding.validate().is_err()
            {
                return Err(PersistenceError::ToolSearchCursorIntegrity);
            }
            Ok(Some(binding))
        })
    }
}

impl ToolSearchCorpusStore for PersistenceCoordinator {
    fn read_corpus(
        &self,
        snapshot_ref: &ToolRegistrySnapshotRef,
    ) -> Result<ToolSearchCorpusV1, ToolSearchCorpusReadError> {
        match self.read_tool_registry_search_corpus(snapshot_ref) {
            Ok(Some(corpus)) => Ok(corpus),
            Ok(None) => Err(ToolSearchCorpusReadError::NotFound),
            Err(PersistenceError::ToolRegistryIntegrity) => {
                Err(ToolSearchCorpusReadError::Corrupted)
            }
            Err(_) => Err(ToolSearchCorpusReadError::Unavailable),
        }
    }
}

impl ToolSearchCursorStore for PersistenceCoordinator {
    fn issue(
        &self,
        binding: ToolSearchCursorBindingRecordV1,
    ) -> Result<ToolSearchCursorV1, ToolSearchCursorStoreError> {
        self.issue_tool_search_cursor(binding).map_err(|error| match error {
            PersistenceError::ToolSearchCursorIntegrity
            | PersistenceError::ToolRegistryIntegrity => ToolSearchCursorStoreError::Corrupted,
            _ => ToolSearchCursorStoreError::Unavailable,
        })
    }

    fn resolve(
        &self,
        cursor: &ToolSearchCursorV1,
    ) -> Result<ToolSearchCursorBindingRecordV1, ToolSearchCursorStoreError> {
        match self.resolve_tool_search_cursor(cursor) {
            Ok(Some(binding)) => Ok(binding),
            Ok(None) => Err(ToolSearchCursorStoreError::Unavailable),
            Err(PersistenceError::ToolSearchCursorIntegrity) => {
                Err(ToolSearchCursorStoreError::Corrupted)
            }
            Err(_) => Err(ToolSearchCursorStoreError::Unavailable),
        }
    }
}

pub(crate) fn insert_tool_registry_search_corpus(
    transaction: &Transaction<'_>,
    corpus: &ToolSearchCorpusV1,
) -> Result<(), PersistenceError> {
    corpus.snapshot.validate().map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
    let snapshot_id = corpus.snapshot.snapshot_ref.tool_registry_snapshot_id.to_string();
    let registry_revision = sqlite_revision(corpus.snapshot.snapshot_ref.registry_revision)?;
    let content_hash_json = encode_canonical(&corpus.snapshot.snapshot_ref.content_hash)?;
    let snapshot_json = encode_canonical(&corpus.snapshot)?;
    let search_documents_json = encode_canonical(&corpus.documents)?;
    let search_documents_hash_json = encode_canonical(
        &canonical_sha256(&corpus.documents)
            .map_err(|_| PersistenceError::ToolRegistryIntegrity)?,
    )?;
    let created_at = corpus.snapshot.created_at.to_string();
    let inserted = transaction.execute(
        "INSERT OR IGNORE INTO tool_registry_snapshots(
            tool_registry_snapshot_id, registry_revision, content_hash_json,
            snapshot_json, search_documents_json, search_documents_hash_json,
            search_corpus_retired_at, created_at, snapshot_contract_version
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, 2)",
        params![
            snapshot_id,
            registry_revision,
            content_hash_json,
            snapshot_json,
            search_documents_json,
            search_documents_hash_json,
            created_at,
        ],
    )?;
    if inserted == 0 {
        let existing = transaction
            .query_row(
                "SELECT registry_revision, content_hash_json, snapshot_json,
                        search_documents_json, search_documents_hash_json, created_at,
                        snapshot_contract_version
                 FROM tool_registry_snapshots WHERE tool_registry_snapshot_id=?1",
                [&snapshot_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                },
            )
            .optional()?;
        if existing
            != Some((
                registry_revision,
                content_hash_json,
                snapshot_json,
                Some(search_documents_json),
                Some(search_documents_hash_json),
                created_at,
                2_i64,
            ))
        {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
    }
    Ok(())
}

pub(crate) fn sqlite_revision(revision: Revision) -> Result<i64, PersistenceError> {
    i64::try_from(revision.value()).map_err(|_| {
        PersistenceError::InvalidRequest("Tool Registry Revision exceeds SQLite range".to_owned())
    })
}

pub(crate) fn encode_canonical<T: serde::Serialize + ?Sized>(
    value: &T,
) -> Result<String, PersistenceError> {
    let bytes = canonical_json_jcs(value).map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
    String::from_utf8(bytes).map_err(|_| PersistenceError::ToolRegistryIntegrity)
}

pub(crate) fn decode_strict<T: DeserializeOwned>(value: &str) -> Result<T, PersistenceError> {
    let value =
        parse_json_strict(value.as_bytes()).map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
    serde_json::from_value(value).map_err(PersistenceError::from)
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use aura_domain_common::ToolRegistrySnapshotId;
    use aura_domain_common::{RuntimeInstanceId, WorkspaceId};
    use aura_tool_contract::{TOOL_SEARCH_ALGORITHM_VERSION_V1, ToolRegistryScopeV1};
    use tempfile::TempDir;

    use super::*;
    use crate::{PersistenceOptions, PersistenceStartup};

    fn start() -> (TempDir, PersistenceCoordinator) {
        let temp = tempfile::tempdir().expect("temp");
        let startup = PersistenceCoordinator::start(PersistenceOptions::new(
            temp.path().join("aura.db"),
            temp.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("start");
        let PersistenceStartup::Ready(coordinator) = startup else {
            panic!("fixture must be ready");
        };
        (temp, coordinator)
    }

    fn empty_corpus() -> ToolSearchCorpusV1 {
        let snapshot = ToolRegistrySnapshotV1::new(
            ToolRegistrySnapshotId::new(),
            Revision::INITIAL,
            ToolRegistryScopeV1::Workspace { workspace_id: WorkspaceId::new() },
            None,
            None,
            CanonicalHashV1::sha256(b"eligibility"),
            vec![],
            vec![],
            vec![],
            UtcTimestamp::now(),
        )
        .expect("snapshot");
        ToolSearchCorpusV1::new(snapshot, vec![]).expect("corpus")
    }

    #[test]
    fn immutable_registry_corpus_and_cursor_survive_until_corpus_retirement() {
        let (_temp, coordinator) = start();
        let corpus = empty_corpus();
        let snapshot_ref = corpus.snapshot.snapshot_ref.clone();
        coordinator.store_tool_registry_search_corpus(corpus.clone()).expect("store corpus");
        coordinator.store_tool_registry_search_corpus(corpus).expect("idempotent store");
        assert_eq!(
            coordinator
                .read_tool_registry_search_corpus(&snapshot_ref)
                .expect("read")
                .expect("corpus")
                .snapshot
                .snapshot_ref,
            snapshot_ref
        );

        let binding = ToolSearchCursorBindingRecordV1 {
            registry_snapshot_ref: snapshot_ref.clone(),
            query_hash: CanonicalHashV1::sha256(b"query"),
            limit: 1,
            audience_visibility_fingerprint: CanonicalHashV1::sha256(b"audience"),
            search_algorithm_version: TOOL_SEARCH_ALGORITHM_VERSION_V1.to_owned(),
            next_position: 1,
        };
        let cursor = coordinator.issue_tool_search_cursor(binding.clone()).expect("cursor");
        assert_eq!(
            coordinator.resolve_tool_search_cursor(&cursor).expect("resolve"),
            Some(binding)
        );

        coordinator
            .retire_tool_registry_search_corpus(&snapshot_ref, UtcTimestamp::now())
            .expect("retire");
        assert_eq!(
            coordinator.read_tool_registry_search_corpus(&snapshot_ref).expect("read retired"),
            None
        );
        assert!(
            coordinator.resolve_tool_search_cursor(&cursor).expect("binding retained").is_some()
        );
    }

    #[test]
    fn cursor_secret_is_never_persisted_in_cleartext() {
        let (_temp, coordinator) = start();
        let corpus = empty_corpus();
        let snapshot_ref = corpus.snapshot.snapshot_ref.clone();
        coordinator.store_tool_registry_search_corpus(corpus).expect("store corpus");
        let cursor = coordinator
            .issue_tool_search_cursor(ToolSearchCursorBindingRecordV1 {
                registry_snapshot_ref: snapshot_ref,
                query_hash: CanonicalHashV1::sha256(b"query"),
                limit: 1,
                audience_visibility_fingerprint: CanonicalHashV1::sha256(b"audience"),
                search_algorithm_version: TOOL_SEARCH_ALGORITHM_VERSION_V1.to_owned(),
                next_position: 1,
            })
            .expect("cursor");
        let stored_hash = coordinator
            .with_reader(|connection| {
                connection
                    .query_row(
                        "SELECT cursor_token_hash FROM tool_search_cursor_bindings",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(PersistenceError::from)
            })
            .expect("stored hash");
        assert_ne!(stored_hash, cursor.as_str());
        assert_eq!(stored_hash, ContentHash::sha256(cursor.as_str().as_bytes()).hex);
    }
}
