//! Session-owned terminal metadata and bounded UTF-8 artifact segments.
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact};
use aura_windows_user_contract::*;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

pub const OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;
const SEGMENT_LIMIT: usize = 32_768;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentTerminalRecord {
    pub view: TerminalViewV1,
    pub operation_id: SecurityOperationId,
    pub runtime_instance_id: RuntimeInstanceId,
    pub working_directory: String,
    pub requires_network: bool,
    #[serde(default)]
    pub tab_closed: bool,
}
pub fn save(tx: &Transaction<'_>, record: &AgentTerminalRecord) -> StoreResult<()> {
    tx.execute("INSERT INTO agent_terminals(id,session_id,payload,output_start,output_end) VALUES(?1,?2,?3,0,0) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload WHERE session_id=excluded.session_id",
        params![record.view.terminal_id,record.view.session_id.to_string(),serde_json::to_string(record)?])?;
    Ok(())
}
pub fn get(db: &Connection, session: SessionId, id: &str) -> StoreResult<AgentTerminalRecord> {
    let payload: Option<String> = db
        .query_row(
            "SELECT payload FROM agent_terminals WHERE id=?1 AND session_id=?2",
            params![id, session.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    serde_json::from_str(&payload.ok_or(Error::Invalid(
        "terminal.not_found_in_session: only Agent terminals in this session are visible",
    ))?)
    .map_err(Into::into)
}
pub fn list(db: &Connection, session: Option<SessionId>) -> StoreResult<Vec<AgentTerminalRecord>> {
    let mut query = db.prepare(
        "SELECT payload FROM agent_terminals WHERE ?1 IS NULL OR session_id=?1 ORDER BY id",
    )?;
    let rows =
        query.query_map([session.map(|value| value.to_string())], |row| row.get::<_, String>(0))?;
    rows.map(|value| Ok(serde_json::from_str(&value?)?)).collect()
}
pub fn append(tx: &Transaction<'_>, session: SessionId, id: &str, text: &str) -> StoreResult<()> {
    get(tx, session, id)?;
    if text.is_empty() {
        return Ok(());
    }
    let mut end =
        tx.query_row("SELECT output_end FROM agent_terminals WHERE id=?1", [id], |row| {
            row.get::<_, i64>(0)
        })? as u64;
    if end.checked_add(text.len() as u64).is_none_or(|value| value > i64::MAX as u64) {
        return Err(Error::Invalid("terminal output offset exhausted"));
    }
    let mut rest = merge_tail(tx, id, &mut end, text)?;
    while !rest.is_empty() {
        let mut length = rest.len().min(SEGMENT_LIMIT);
        while !rest.is_char_boundary(length) {
            length -= 1;
        }
        let (part, tail) = rest.split_at(length);
        let artifact_id = ArtifactId::new();
        artifact::put(
            tx,
            artifact_id,
            id,
            "text/plain; charset=utf-8",
            part.as_bytes(),
            UtcTimestamp::now(),
        )?;
        tx.execute(
            "INSERT INTO terminal_output_segments VALUES(?1,?2,?3,?4)",
            params![id, end as i64, (end + length as u64) as i64, artifact_id.to_string()],
        )?;
        end += length as u64;
        rest = tail;
    }
    let mut query = tx.prepare(
        "SELECT artifact_id FROM terminal_output_segments WHERE terminal_id=?1 AND start_offset<?2",
    )?;
    let old = query
        .query_map(params![id, end.saturating_sub(OUTPUT_LIMIT) as i64], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(query);
    tx.execute(
        "DELETE FROM terminal_output_segments WHERE terminal_id=?1 AND start_offset<?2",
        params![id, end.saturating_sub(OUTPUT_LIMIT) as i64],
    )?;
    for artifact_id in old {
        artifact::release_owner(tx, artifact_id.parse().map_err(|_| Error::Conflict)?, id)?;
    }
    tx.execute("UPDATE agent_terminals SET output_start=coalesce((SELECT min(start_offset) FROM terminal_output_segments WHERE terminal_id=?1),?2),output_end=?2 WHERE id=?1",params![id,end as i64])?;
    artifact::collect(tx)?;
    Ok(())
}
/// Replace only the bounded last segment. Its immutable artifact and offsets
/// change in the same transaction, so readers see either complete version.
fn merge_tail<'a>(
    tx: &Transaction<'_>,
    id: &str,
    end: &mut u64,
    text: &'a str,
) -> StoreResult<&'a str> {
    let tail: Option<(u64, String)> = tx.query_row(
        "SELECT start_offset,artifact_id FROM terminal_output_segments WHERE terminal_id=?1 AND next_offset=?2 ORDER BY start_offset DESC LIMIT 1",
        params![id, *end as i64],
        |row| Ok((row.get::<_, i64>(0)? as u64, row.get(1)?)),
    ).optional()?;
    let Some((start, previous)) = tail else {
        return Ok(text);
    };
    let previous: ArtifactId = previous.parse().map_err(|_| Error::Conflict)?;
    let mut bytes = artifact::read(tx, previous, id)?;
    if bytes.len() >= SEGMENT_LIMIT {
        return Ok(text);
    }
    if start.checked_add(bytes.len() as u64) != Some(*end) || std::str::from_utf8(&bytes).is_err() {
        return Err(Error::Invalid("terminal corrupt UTF-8 segment"));
    }
    let mut length = text.len().min(SEGMENT_LIMIT - bytes.len());
    while !text.is_char_boundary(length) {
        length -= 1;
    }
    if length == 0 {
        return Ok(text);
    }
    bytes.extend_from_slice(&text.as_bytes()[..length]);
    let replacement = ArtifactId::new();
    artifact::put(tx, replacement, id, "text/plain; charset=utf-8", &bytes, UtcTimestamp::now())?;
    *end += length as u64;
    tx.execute(
        "UPDATE terminal_output_segments SET next_offset=?3,artifact_id=?4 WHERE terminal_id=?1 AND start_offset=?2",
        params![id, start as i64, *end as i64, replacement.to_string()],
    )?;
    artifact::release_owner(tx, previous, id)?;
    Ok(&text[length..])
}
pub fn read(
    db: &Connection,
    record: &AgentTerminalRecord,
    offset: u64,
    max_bytes: usize,
) -> StoreResult<AgentTerminalResultV1> {
    if !(1..=65536).contains(&max_bytes) {
        return Err(Error::Invalid("terminal.max_bytes: use 1..65536"));
    }
    let id = &record.view.terminal_id;
    let (first, last) = db.query_row(
        "SELECT output_start,output_end FROM agent_terminals WHERE id=?1",
        [id],
        |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64)),
    )?;
    let mut start = offset.max(first).min(last);
    let mut output = String::new();
    let mut cursor = start;
    let mut query=db.prepare("SELECT start_offset,artifact_id FROM terminal_output_segments WHERE terminal_id=?1 AND next_offset>?2 ORDER BY start_offset")?;
    let rows = query.query_map(params![id, start as i64], |row| {
        Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (base, artifact_id) = row?;
        let bytes = artifact::read(db, artifact_id.parse().map_err(|_| Error::Conflict)?, id)?;
        let text =
            std::str::from_utf8(&bytes).map_err(|_| Error::Invalid("terminal corrupt UTF-8"))?;
        let mut begin = cursor.saturating_sub(base) as usize;
        while begin < text.len() && !text.is_char_boundary(begin) {
            begin += 1;
        }
        if output.is_empty() {
            cursor = base + begin as u64;
            start = cursor;
        }
        let mut length = (text.len() - begin).min(max_bytes - output.len());
        while !text.is_char_boundary(begin + length) {
            length -= 1;
        }
        output.push_str(&text[begin..begin + length]);
        cursor = base + (begin + length) as u64;
        if length < text.len() - begin || output.len() == max_bytes {
            break;
        }
    }
    Ok(AgentTerminalResultV1 {
        terminal_id: id.clone(),
        output,
        start_offset: start,
        next_offset: cursor,
        truncated: offset < first || cursor < last,
        running: record.view.end.is_none(),
        end: record.view.end.clone(),
    })
}
pub fn delete_session(tx: &Transaction<'_>, session: SessionId) -> StoreResult<()> {
    for record in list(tx, Some(session))? {
        let id = &record.view.terminal_id;
        let mut query =
            tx.prepare("SELECT artifact_id FROM terminal_output_segments WHERE terminal_id=?1")?;
        let artifacts =
            query.query_map([id], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        drop(query);
        tx.execute("DELETE FROM terminal_output_segments WHERE terminal_id=?1", [id])?;
        tx.execute("DELETE FROM agent_terminals WHERE id=?1", [id])?;
        for value in artifacts {
            artifact::release_owner(tx, value.parse().map_err(|_| Error::Conflict)?, id)?;
        }
    }
    artifact::collect(tx)?;
    Ok(())
}
