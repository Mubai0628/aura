//! The workspace text index (`workspace_content_fts`). FTS5 keeps the entries of a replaced file
//! until the segments holding them merge, and an index updated all day seldom merges its oldest
//! segments: it grows far past its text. Rewriting it (`optimize`) merges every segment into one
//! and drops what was replaced; the pages it frees go back to the disk with the rebuild.
use rusqlite::Connection;

use crate::PersistenceError;

/// An index smaller than this is not worth rewriting, however it compares with its text.
const REWRITE_AT: u64 = 16 * 1024 * 1024;
/// How many times its text the index may grow before it is rewritten. Rewritten, it is about a
/// third of its text.
const TIMES_ITS_TEXT: u64 = 2;

pub(super) const OPTIMIZE: &str =
    "INSERT INTO workspace_content_fts(workspace_content_fts) VALUES('optimize')";

/// The index's bytes and its text's; `None` in a store without the index. Reads the whole index.
pub(super) fn measure(connection: &Connection) -> Result<Option<(u64, u64)>, PersistenceError> {
    let present: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='workspace_content_fts')",
        [],
        |row| row.get(0),
    )?;
    if !present {
        return Ok(None);
    }
    let bytes = |sql: &str| -> Result<u64, PersistenceError> {
        Ok(connection.query_row(sql, [], |row| row.get::<_, i64>(0))?.max(0) as u64)
    };
    let index = bytes("SELECT COALESCE(SUM(length(block)), 0) FROM workspace_content_fts_data")?;
    let text = bytes(
        "SELECT COALESCE(SUM(length(CAST(c2 AS BLOB))), 0) FROM workspace_content_fts_content",
    )?;
    Ok(Some((index, text)))
}

/// Whether an index of these bytes, over this much text, is owed a rewrite.
pub(super) fn owed(&(index, text): &(u64, u64)) -> bool {
    index >= REWRITE_AT && index > text.saturating_mul(TIMES_ITS_TEXT)
}
