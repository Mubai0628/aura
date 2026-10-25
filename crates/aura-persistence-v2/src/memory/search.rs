use super::MemoryIndex;
use aura_domain_common::UtcTimestamp;
use aura_memory_domain::MemoryMetadata;
use aura_persistence_core::StoreResult;
use rusqlite::params;

pub use aura_memory_domain::{MemorySearch, MemorySearchHit, MemorySearchResult};

impl MemoryIndex {
    pub fn search(&self, request: &MemorySearch) -> StoreResult<MemorySearchResult> {
        self.search_ordered(request, false)
    }
    pub fn recent(&self, limit: Option<u32>) -> StoreResult<MemorySearchResult> {
        self.search_ordered(&MemorySearch { limit, ..Default::default() }, true)
    }
    fn search_ordered(
        &self,
        request: &MemorySearch,
        recent: bool,
    ) -> StoreResult<MemorySearchResult> {
        let query = request.query.trim();
        let limit = request.limit.unwrap_or(20).clamp(1, 100);
        // A trigram cannot represent a one/two-codepoint Chinese query. The
        // bounded fallback scans at most 10,000 eligible notes and reports it.
        let short = query.chars().count() < 3;
        let fts_query = format!("\"{}\"", query.replace('"', "\"\""));
        let sql = if query.is_empty() || short {
            "SELECT e.relative_path,e.metadata_json,e.title,e.content_hash,f.body,0.0 FROM memory_entries e JOIN memory_fts f ON f.relative_path=e.relative_path WHERE e.valid=1 AND (?1 OR e.status NOT IN ('forgotten','archived','superseded')) AND (?2 IS NULL OR e.category=?2) ORDER BY e.updated_at DESC LIMIT 10001"
        } else {
            "SELECT e.relative_path,e.metadata_json,e.title,e.content_hash,memory_fts.body,bm25(memory_fts) FROM memory_fts JOIN memory_entries e ON e.relative_path=memory_fts.relative_path WHERE memory_fts MATCH ?3 AND e.valid=1 AND (?1 OR e.status NOT IN ('forgotten','archived','superseded')) AND (?2 IS NULL OR e.category=?2) ORDER BY rank LIMIT 10001"
        };
        let mut statement = self.connection.prepare(sql)?;
        let mut rows = if query.is_empty() || short {
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
            let title: String = row.get(2)?;
            let body: String = row.get(4)?;
            if !query.is_empty()
                && short
                && !format!("{title}\n{body}").to_lowercase().contains(&query.to_lowercase())
            {
                continue;
            }
            let metadata: MemoryMetadata = serde_json::from_str(&row.get::<_, String>(1)?)?;
            let age = (UtcTimestamp::now().value() - metadata.updated_at.value())
                .whole_days()
                .max(0) as f64;
            let decay = 1.0 / (1.0 + age / 90.0);
            let fulltext: f64 = row.get(5)?;
            let score = (1.0 + (-fulltext).max(0.0))
                * f64::from(metadata.importance)
                * metadata.confidence
                * metadata.recall_weight
                * decay;
            hits.push(MemorySearchHit {
                scope: metadata.scope,
                path: row.get(0)?,
                title,
                hash: row.get(3)?,
                excerpt: body.chars().take(400).collect(),
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
