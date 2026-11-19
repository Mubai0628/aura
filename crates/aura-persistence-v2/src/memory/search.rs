use super::MemoryIndex;
use crate::retrieval::QueryTerms;
use aura_domain_common::UtcTimestamp;
use aura_memory_domain::MemoryMetadata;
use aura_persistence_core::StoreResult;
use rusqlite::params;

pub use aura_memory_domain::{MemorySearch, MemorySearchHit, MemorySearchResult};

impl MemoryIndex {
    pub fn search(&self, request: &MemorySearch) -> StoreResult<MemorySearchResult> {
        self.search_ordered(request, false, false)
    }
    pub fn search_related(&self, request: &MemorySearch) -> StoreResult<MemorySearchResult> {
        self.search_ordered(request, false, true)
    }
    pub fn recent(&self, limit: Option<u32>) -> StoreResult<MemorySearchResult> {
        self.search_ordered(&MemorySearch { limit, ..Default::default() }, true, false)
    }
    fn search_ordered(
        &self,
        request: &MemorySearch,
        recent: bool,
        semantic: bool,
    ) -> StoreResult<MemorySearchResult> {
        let query = request.query.trim();
        let limit = request.limit.unwrap_or(20).clamp(1, 100);
        let terms = QueryTerms::new(query);
        let terms = if semantic { terms.with_synonyms() } else { terms };
        let fts_query = terms.fts();
        let sql = if fts_query.is_empty() {
            "SELECT e.relative_path,e.metadata_json,e.title,e.content_hash,f.body,0.0 FROM memory_entries e JOIN memory_fts f ON f.relative_path=e.relative_path WHERE e.valid=1 AND (?1 OR e.status NOT IN ('forgotten','archived','superseded')) AND (?2 IS NULL OR e.category=?2) ORDER BY e.updated_at DESC LIMIT 10001"
        } else {
            "SELECT e.relative_path,e.metadata_json,e.title,e.content_hash,f.body,f.rank
            FROM memory_fts f JOIN memory_entries e ON e.relative_path=f.relative_path
            WHERE memory_fts MATCH ?3 AND e.valid=1
            AND (?1 OR e.status NOT IN ('forgotten','archived','superseded'))
            AND (?2 IS NULL OR e.category=?2)
            ORDER BY f.rank LIMIT 10001"
        };
        let mut statement = self.connection.prepare(sql)?;
        let mut rows = if fts_query.is_empty() {
            statement.query(params![request.include_inactive, request.category])?
        } else {
            statement.query(params![request.include_inactive, request.category, fts_query])?
        };
        let mut hits = vec![];
        let mut count = 0;
        while let Some(row) = rows.next()? {
            count += 1;
            if count > 10000 {
                break;
            }
            let path: String = row.get(0)?;
            let title: String = row.get(2)?;
            let body: String = row.get(4)?;
            let relevant = terms.passage(&format!("{title}\n{path}"), &body, 400);
            if !query.is_empty() && relevant.is_none() {
                continue;
            }
            let metadata: MemoryMetadata = serde_json::from_str(&row.get::<_, String>(1)?)?;
            let age = (UtcTimestamp::now().value() - metadata.updated_at.value())
                .whole_days()
                .max(0) as f64;
            let decay = 1.0 / (1.0 + age / 90.0);
            let fulltext: f64 = row.get(5)?;
            let score = (1.0
                + relevant.as_ref().map_or(0.0, |v| f64::from(v.score))
                + (-fulltext).max(0.0))
                * f64::from(metadata.importance)
                * metadata.confidence
                * metadata.recall_weight
                * decay;
            hits.push(MemorySearchHit {
                scope: metadata.scope,
                path,
                title,
                hash: row.get(3)?,
                excerpt: relevant.map_or_else(|| body.chars().take(400).collect(), |v| v.excerpt),
                metadata,
                score,
            });
        }
        if recent {
            hits.sort_by(|a, b| {
                b.metadata.updated_at.cmp(&a.metadata.updated_at).then_with(|| a.path.cmp(&b.path))
            });
        } else {
            hits.sort_by(|a, b| {
                b.score
                    .total_cmp(&a.score)
                    .then_with(|| b.metadata.updated_at.cmp(&a.metadata.updated_at))
                    .then_with(|| a.path.cmp(&b.path))
            });
        }
        hits.truncate(limit as usize);
        Ok(MemorySearchResult { hits, incomplete: count > 10000 || self.status()?.rebuilding })
    }
}
