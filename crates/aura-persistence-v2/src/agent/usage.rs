//! The tokens a conversation's answers used, counted on from where the last count ended.
use aura_domain_common::SessionId;
use aura_persistence_core::StoreResult;
use rusqlite::{Connection, params};

/// The tokens of conversation `session`'s answers recorded after event `after`, as their
/// providers reported them, and the newest such event (`after` when there is none).
pub fn tokens_after(db: &Connection, session: SessionId, after: i64) -> StoreResult<(i64, u64)> {
    let (newest, tokens): (Option<i64>, i64) = db.query_row(
        "SELECT max(e.sequence), \
         coalesce(sum(CAST(json_extract(e.payload, '$.usage.total_tokens') AS INTEGER)), 0) \
         FROM turn_tasks t JOIN agent_runs r ON r.turn_task_id=t.id \
         JOIN events e ON e.owner=r.id AND e.kind='agent.assistant_response' AND e.sequence>?2 \
         WHERE t.session_id=?1",
        params![session.to_string(), after],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((newest.unwrap_or(after), u64::try_from(tokens).unwrap_or_default()))
}
