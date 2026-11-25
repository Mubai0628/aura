use super::MemoryIndex;
use aura_memory_domain::{MemoryHistory, MemoryTimelineEntry, MemoryTimelineQuery};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::params;

const COLUMNS: &str =
    "memory_id,relative_path,action,actor,session_id,turn_id,before_hash,after_hash,title,at";

impl MemoryIndex {
    pub fn timeline(&self, query: &MemoryTimelineQuery) -> StoreResult<Vec<MemoryTimelineEntry>> {
        let before = query
            .before
            .as_ref()
            .map(|hash| {
                self.connection.query_row(
                    "SELECT sequence FROM memory_history WHERE record_hash=?1",
                    [hash],
                    |r| r.get::<_, i64>(0),
                )
            })
            .transpose()?;
        let sql = format!("SELECT {COLUMNS},record_hash,note_category FROM (
             SELECT *, (SELECT e.category FROM memory_entries e WHERE e.memory_id=memory_history.memory_id AND e.valid=1 ORDER BY e.relative_path LIMIT 1) AS note_category
             FROM memory_history WHERE (?1 IS NULL OR session_id=?1)
             AND (?2 IS NULL OR julianday(at)>=julianday(?2)) AND (?3 IS NULL OR julianday(at)<julianday(?3))
             AND (?4 IS NULL OR sequence<?4)) WHERE (?5 IS NULL OR note_category=?5) ORDER BY sequence DESC LIMIT ?6");
        let mut stmt = self.connection.prepare(&sql)?;
        let mut rows = stmt.query(params![
            query.author_session_id.map(|id| id.to_string()),
            query.since.map(|at| at.to_string()),
            query.until.map(|at| at.to_string()),
            before,
            query.category,
            query.limit.unwrap_or(30).clamp(1, 100) + 1
        ])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(MemoryTimelineEntry {
                event: decode(row)?,
                cursor: row.get(10)?,
                category: row.get(11)?,
                session_name_at_write: None,
            });
        }
        Ok(items)
    }

    pub fn creation_history(&self, id: &str) -> StoreResult<Option<MemoryHistory>> {
        let mut s = self.connection.prepare(&format!("SELECT {COLUMNS} FROM memory_history WHERE memory_id=?1 AND action='create' ORDER BY sequence LIMIT 2"))?;
        let mut rows = s.query([id])?;
        let one = rows.next()?.map(decode).transpose()?;
        Ok(if rows.next()?.is_some() { None } else { one })
    }

    pub fn append_history(&mut self, history: &[MemoryHistory]) -> StoreResult<()> {
        let tx = self.connection.transaction()?;
        for h in history {
            let record_hash = blake3::hash(&serde_json::to_vec(h)?).to_hex().to_string();
            tx.execute("INSERT OR IGNORE INTO memory_history(record_hash,memory_id,relative_path,action,actor,session_id,turn_id,before_hash,after_hash,title,at) VALUES(?11,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![h.id.map(|id|id.to_string()),h.path,h.action,h.actor,h.session_id.map(|s|s.to_string()),h.turn_id,h.before_hash,h.after_hash,h.title,h.at.to_string(),record_hash])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn recent_history(&self, id: Option<&str>, limit: u32) -> StoreResult<Vec<MemoryHistory>> {
        let mut s = self.connection.prepare(&format!("SELECT {COLUMNS} FROM memory_history WHERE (?1 IS NULL OR memory_id=?1) ORDER BY sequence DESC LIMIT ?2"))?;
        let mut rows = s.query(params![id, limit.min(100)])?;
        let mut result = vec![];
        while let Some(r) = rows.next()? {
            result.push(decode(r)?);
        }
        Ok(result)
    }
    /// The newest record of each given path. A watcher delta needs only these to
    /// tell Aura's own publications from external edits; it never rereads all records.
    pub fn latest_history_for(&self, paths: &[String]) -> StoreResult<Vec<MemoryHistory>> {
        let mut s = self.connection.prepare(&format!(
            "SELECT {COLUMNS} FROM memory_history WHERE relative_path=?1 ORDER BY sequence DESC LIMIT 1"
        ))?;
        let mut result = vec![];
        for path in paths {
            let mut rows = s.query([path])?;
            if let Some(r) = rows.next()? {
                result.push(decode(r)?);
            }
        }
        Ok(result)
    }
}

fn decode(r: &rusqlite::Row<'_>) -> StoreResult<MemoryHistory> {
    let id: Option<String> = r.get(0)?;
    let session: Option<String> = r.get(4)?;
    Ok(MemoryHistory {
        id: id.map(|s| s.parse()).transpose().map_err(|_| Error::Invalid("memory id"))?,
        path: r.get(1)?,
        action: r.get(2)?,
        actor: r.get(3)?,
        session_id: session
            .map(|s| s.parse())
            .transpose()
            .map_err(|_| Error::Invalid("session id"))?,
        turn_id: r.get(5)?,
        before_hash: r.get(6)?,
        after_hash: r.get(7)?,
        title: r.get(8)?,
        at: r.get::<_, String>(9)?.parse().map_err(|_| Error::Invalid("memory timestamp"))?,
    })
}
