use std::cmp::Reverse;

use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_tool_contract::{
    MAX_TOOL_SEARCH_PAGE_SIZE, TOOL_SEARCH_ALGORITHM_VERSION_V1, ToolRegistryErrorCode,
    ToolRegistrySnapshotRef, ToolRegistrySnapshotV1, ToolSearchCursorV1, ToolSearchHitV1,
    ToolSearchPageV1, ToolSearchRequestV1,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ToolSearchDocumentV1, UnicodeCaseFoldError, unicode_default_casefold_v1};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolSearchCorpusV1 {
    pub snapshot: ToolRegistrySnapshotV1,
    pub documents: Vec<ToolSearchDocumentV1>,
}

impl ToolSearchCorpusV1 {
    pub fn new(
        snapshot: ToolRegistrySnapshotV1,
        documents: Vec<ToolSearchDocumentV1>,
    ) -> Result<Self, ToolSearchError> {
        snapshot.validate().map_err(|_| ToolSearchError::CorpusCorrupted)?;
        if snapshot.catalog_entries.len() != documents.len()
            || !documents.windows(2).all(|pair| pair[0].tool_id < pair[1].tool_id)
        {
            return Err(ToolSearchError::CorpusCorrupted);
        }
        for (entry, document) in snapshot.catalog_entries.iter().zip(&documents) {
            if entry.tool_id != document.tool_id
                || entry.tool_version != document.tool_version
                || entry.definition_fingerprint != document.definition_fingerprint
            {
                return Err(ToolSearchError::CorpusCorrupted);
            }
        }
        Ok(Self { snapshot, documents })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSearchCursorBindingRecordV1 {
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub query_hash: CanonicalHashV1,
    pub limit: u8,
    pub audience_visibility_fingerprint: CanonicalHashV1,
    pub search_algorithm_version: String,
    pub next_position: u32,
}

impl ToolSearchCursorBindingRecordV1 {
    pub fn validate(&self) -> Result<(), ToolSearchCursorStoreError> {
        if self.limit == 0
            || self.limit > MAX_TOOL_SEARCH_PAGE_SIZE
            || self.search_algorithm_version != TOOL_SEARCH_ALGORITHM_VERSION_V1
            || self.next_position == 0
            || self.query_hash.algorithm() != aura_domain_common::ContentHashAlgorithm::Sha256
        {
            return Err(ToolSearchCursorStoreError::Corrupted);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolSearchCorpusReadError {
    #[error("registry snapshot was garbage collected or does not exist")]
    NotFound,
    #[error("registry snapshot is temporarily unavailable")]
    Unavailable,
    #[error("registry snapshot or its search corpus failed integrity checks")]
    Corrupted,
}

pub trait ToolSearchCorpusStore: Send + Sync {
    fn read_corpus(
        &self,
        snapshot_ref: &ToolRegistrySnapshotRef,
    ) -> Result<ToolSearchCorpusV1, ToolSearchCorpusReadError>;
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolSearchCursorStoreError {
    #[error("opaque cursor token is unknown or failed authentication")]
    Unavailable,
    #[error("opaque cursor record failed integrity checks")]
    Corrupted,
}

pub trait ToolSearchCursorStore: Send + Sync {
    fn issue(
        &self,
        binding: ToolSearchCursorBindingRecordV1,
    ) -> Result<ToolSearchCursorV1, ToolSearchCursorStoreError>;

    fn resolve(
        &self,
        cursor: &ToolSearchCursorV1,
    ) -> Result<ToolSearchCursorBindingRecordV1, ToolSearchCursorStoreError>;
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolSearchError {
    #[error("tool search request is invalid")]
    InvalidRequest,
    #[error("tool search cursor does not match this request or audience")]
    CursorMismatch,
    #[error("tool search cursor is unavailable")]
    CursorUnavailable,
    #[error("the registry snapshot bound to this cursor has expired")]
    CursorExpired,
    #[error("the requested registry snapshot is unavailable")]
    SnapshotUnavailable,
    #[error("the registry search corpus failed integrity validation")]
    CorpusCorrupted,
}

impl ToolSearchError {
    #[must_use]
    pub const fn public_code(&self) -> ToolRegistryErrorCode {
        match self {
            Self::InvalidRequest => ToolRegistryErrorCode::SearchRequestInvalid,
            Self::CursorMismatch => ToolRegistryErrorCode::CursorMismatch,
            Self::CursorUnavailable => ToolRegistryErrorCode::CursorUnavailable,
            Self::CursorExpired => ToolRegistryErrorCode::CursorExpired,
            Self::SnapshotUnavailable => ToolRegistryErrorCode::SnapshotUnavailable,
            Self::CorpusCorrupted => ToolRegistryErrorCode::SnapshotCorrupted,
        }
    }
}

pub struct ToolSearchServiceV1<'a> {
    corpus_store: &'a dyn ToolSearchCorpusStore,
    cursor_store: &'a dyn ToolSearchCursorStore,
}

impl<'a> ToolSearchServiceV1<'a> {
    #[must_use]
    pub const fn new(
        corpus_store: &'a dyn ToolSearchCorpusStore,
        cursor_store: &'a dyn ToolSearchCursorStore,
    ) -> Self {
        Self { corpus_store, cursor_store }
    }

    pub fn search(
        &self,
        current_snapshot_ref: &ToolRegistrySnapshotRef,
        current_audience_visibility_fingerprint: &CanonicalHashV1,
        request: &ToolSearchRequestV1,
    ) -> Result<ToolSearchPageV1, ToolSearchError> {
        request.validate().map_err(|_| ToolSearchError::InvalidRequest)?;
        let query_hash = canonical_sha256(request.query.as_str())
            .map_err(|_| ToolSearchError::InvalidRequest)?;
        let (snapshot_ref, position, cursor_request) = if let Some(cursor) = &request.cursor {
            let binding = self
                .cursor_store
                .resolve(cursor)
                .map_err(|_| ToolSearchError::CursorUnavailable)?;
            if binding.query_hash != query_hash
                || binding.limit != request.limit
                || binding.audience_visibility_fingerprint
                    != *current_audience_visibility_fingerprint
                || binding.search_algorithm_version != TOOL_SEARCH_ALGORITHM_VERSION_V1
            {
                return Err(ToolSearchError::CursorMismatch);
            }
            (
                binding.registry_snapshot_ref,
                usize::try_from(binding.next_position)
                    .map_err(|_| ToolSearchError::CursorUnavailable)?,
                true,
            )
        } else {
            (current_snapshot_ref.clone(), 0, false)
        };
        let corpus = self.corpus_store.read_corpus(&snapshot_ref).map_err(|error| match error {
            ToolSearchCorpusReadError::NotFound if cursor_request => ToolSearchError::CursorExpired,
            ToolSearchCorpusReadError::NotFound | ToolSearchCorpusReadError::Unavailable => {
                ToolSearchError::SnapshotUnavailable
            }
            ToolSearchCorpusReadError::Corrupted => ToolSearchError::CorpusCorrupted,
        })?;
        if corpus.snapshot.snapshot_ref != snapshot_ref {
            return Err(ToolSearchError::CorpusCorrupted);
        }

        let folded_query =
            unicode_default_casefold_v1(request.query.as_str()).map_err(map_casefold_error)?;
        let query_terms = folded_query.split_whitespace().collect::<Vec<_>>();
        if query_terms.is_empty() {
            return Err(ToolSearchError::InvalidRequest);
        }
        let mut ranked = corpus
            .snapshot
            .catalog_entries
            .iter()
            .zip(&corpus.documents)
            .filter_map(|(entry, document)| {
                rank_entry(entry, document, &folded_query, &query_terms).map(|rank| (rank, entry))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(left_rank, left), (right_rank, right)| {
            left_rank.cmp(right_rank).then_with(|| left.tool_id.cmp(&right.tool_id))
        });
        if position > ranked.len() {
            return Err(ToolSearchError::CursorUnavailable);
        }
        let end = position.saturating_add(usize::from(request.limit)).min(ranked.len());
        let results = ranked[position..end]
            .iter()
            .map(|(_, entry)| ToolSearchHitV1 {
                tool_id: entry.tool_id,
                tool_version: entry.tool_version.clone(),
                canonical_name: entry.canonical_name.to_string(),
                display_name: entry.display_name.clone(),
                summary: entry.summary.clone(),
                source: entry.source,
                load_state: entry.load_state,
            })
            .collect();
        let next_cursor = if end < ranked.len() {
            Some(
                self.cursor_store
                    .issue(ToolSearchCursorBindingRecordV1 {
                        registry_snapshot_ref: snapshot_ref.clone(),
                        query_hash,
                        limit: request.limit,
                        audience_visibility_fingerprint: current_audience_visibility_fingerprint
                            .clone(),
                        search_algorithm_version: TOOL_SEARCH_ALGORITHM_VERSION_V1.to_owned(),
                        next_position: u32::try_from(end)
                            .map_err(|_| ToolSearchError::CorpusCorrupted)?,
                    })
                    .map_err(|_| ToolSearchError::CursorUnavailable)?,
            )
        } else {
            None
        };
        Ok(ToolSearchPageV1 { registry_snapshot: snapshot_ref, results, next_cursor })
    }
}

/// Lower tuples sort first. Relevance is reversed so a larger deterministic
/// term-occurrence count ranks before a smaller one.
fn rank_entry(
    entry: &aura_tool_contract::ToolRegistryCatalogEntryV1,
    document: &ToolSearchDocumentV1,
    folded_query: &str,
    query_terms: &[&str],
) -> Option<(u8, Reverse<usize>)> {
    let canonical = unicode_default_casefold_v1(entry.canonical_name.as_str()).ok()?;
    let display = unicode_default_casefold_v1(&entry.display_name).ok()?;
    if canonical == folded_query {
        return Some((0, Reverse(usize::MAX)));
    }
    if display == folded_query {
        return Some((1, Reverse(usize::MAX)));
    }
    let canonical_leaf = canonical.rsplit('.').next().unwrap_or(&canonical);
    if canonical.starts_with(folded_query) || canonical_leaf.starts_with(folded_query) {
        return Some((2, Reverse(usize::MAX)));
    }
    let searchable = unicode_default_casefold_v1(&format!(
        "{}\n{}\n{}\n{}",
        entry.canonical_name, entry.display_name, entry.summary, document.model_description
    ))
    .ok()?;
    let relevance =
        query_terms.iter().map(|term| searchable.match_indices(term).count()).sum::<usize>();
    (relevance > 0).then_some((3, Reverse(relevance)))
}

fn map_casefold_error(_error: UnicodeCaseFoldError) -> ToolSearchError {
    ToolSearchError::CorpusCorrupted
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Mutex};

    use aura_tool_contract::{ToolSearchCursorV1, ToolSearchQueryV1, ToolSearchRequestV1};

    use super::*;

    #[derive(Default)]
    struct MemoryCursorStore {
        records: Mutex<BTreeMap<String, ToolSearchCursorBindingRecordV1>>,
    }

    impl ToolSearchCursorStore for MemoryCursorStore {
        fn issue(
            &self,
            binding: ToolSearchCursorBindingRecordV1,
        ) -> Result<ToolSearchCursorV1, ToolSearchCursorStoreError> {
            let mut records =
                self.records.lock().map_err(|_| ToolSearchCursorStoreError::Unavailable)?;
            let token = format!("tc1.cursor{}", records.len() + 1);
            records.insert(token.clone(), binding);
            ToolSearchCursorV1::new(token).map_err(|_| ToolSearchCursorStoreError::Corrupted)
        }

        fn resolve(
            &self,
            cursor: &ToolSearchCursorV1,
        ) -> Result<ToolSearchCursorBindingRecordV1, ToolSearchCursorStoreError> {
            self.records
                .lock()
                .map_err(|_| ToolSearchCursorStoreError::Unavailable)?
                .get(cursor.as_str())
                .cloned()
                .ok_or(ToolSearchCursorStoreError::Unavailable)
        }
    }

    struct MemoryCorpusStore(ToolSearchCorpusV1);

    impl ToolSearchCorpusStore for MemoryCorpusStore {
        fn read_corpus(
            &self,
            snapshot_ref: &ToolRegistrySnapshotRef,
        ) -> Result<ToolSearchCorpusV1, ToolSearchCorpusReadError> {
            if &self.0.snapshot.snapshot_ref != snapshot_ref {
                return Err(ToolSearchCorpusReadError::NotFound);
            }
            Ok(self.0.clone())
        }
    }

    fn build_search_corpus() -> ToolSearchCorpusV1 {
        let mut input = crate::builder::tests_support::search_fixture_input();
        input.workspace_pins.clear();
        let output = crate::build_parent_registry_snapshot(input).expect("registry");
        ToolSearchCorpusV1::new(output.snapshot, output.search_documents).expect("corpus")
    }

    #[test]
    fn ranking_is_stable_and_cursor_is_bound_to_query_limit_and_visibility() {
        let corpus_store = MemoryCorpusStore(build_search_corpus());
        let cursor_store = MemoryCursorStore::default();
        let service = ToolSearchServiceV1::new(&corpus_store, &cursor_store);
        let visibility = CanonicalHashV1::sha256(b"parent-visible");
        let request = ToolSearchRequestV1 {
            query: ToolSearchQueryV1::new("tool").expect("query"),
            limit: 1,
            cursor: None,
        };
        let first = service
            .search(&corpus_store.0.snapshot.snapshot_ref, &visibility, &request)
            .expect("first page");
        assert_eq!(first.results.len(), 1);
        let cursor = first.next_cursor.expect("next cursor");

        let second = service
            .search(
                &corpus_store.0.snapshot.snapshot_ref,
                &visibility,
                &ToolSearchRequestV1 {
                    query: request.query.clone(),
                    limit: 1,
                    cursor: Some(cursor.clone()),
                },
            )
            .expect("second page");
        assert_ne!(first.results[0].tool_id, second.results[0].tool_id);

        let mismatch = service.search(
            &corpus_store.0.snapshot.snapshot_ref,
            &visibility,
            &ToolSearchRequestV1 {
                query: ToolSearchQueryV1::new("read").expect("query"),
                limit: 1,
                cursor: Some(cursor),
            },
        );
        assert_eq!(mismatch, Err(ToolSearchError::CursorMismatch));
    }
}
