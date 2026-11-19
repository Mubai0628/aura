use aura_persistence_core::StoreResult;
use rusqlite::{Connection, Transaction, params};
use std::collections::BTreeSet;

pub(super) const SCHEMA: &str = include_str!("search_index.sql");

pub(super) fn insert(db: &Connection, path: &str, title: &str, body: &str) -> StoreResult<()> {
    // SQLite's trigram tokenizer needs a third character to index a two-character lookup.
    // The prefixed pairs live in their own column, separate from the returned original text.
    let mut pairs = BTreeSet::new();
    for source in [path, title, body] {
        let mut previous = None;
        for ch in source.chars() {
            if ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                let ch = ch.to_lowercase().next().unwrap_or(ch);
                if let Some(first) = previous {
                    pairs.insert((first, ch));
                }
                previous = Some(ch);
            } else {
                previous = None;
            }
        }
    }
    let mut terms = String::new();
    for (a, b) in pairs {
        terms.extend(['_', a, b, ' ']);
    }
    db.execute(
        "INSERT INTO memory_fts(relative_path,title,body,short_terms) VALUES(?1,?2,?3,?4)",
        params![path, title, body, terms],
    )?;
    Ok(())
}

pub(super) fn upgrade_v1(tx: &Transaction<'_>) -> StoreResult<()> {
    tx.execute_batch("ALTER TABLE memory_fts RENAME TO memory_fts_v1")?;
    tx.execute_batch(SCHEMA)?;
    {
        let mut statement = tx.prepare("SELECT relative_path,title,body FROM memory_fts_v1")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            insert(
                tx,
                &row.get::<_, String>(0)?,
                &row.get::<_, String>(1)?,
                &row.get::<_, String>(2)?,
            )?;
        }
    }
    tx.execute_batch("DROP TABLE memory_fts_v1")?;
    tx.pragma_update(None, "user_version", super::CACHE_SCHEMA_VERSION)?;
    Ok(())
}
