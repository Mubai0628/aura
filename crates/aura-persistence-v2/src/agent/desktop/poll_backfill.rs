//! Budgeted, fair repair of the derived history-poll copy.
use super::*;

/// Advances before doing work, so a rolled-back transaction cannot keep every
/// subsequent attempt pinned to the same failing owner. Restarting loses only
/// scheduling position; all copy progress remains in the database.
#[derive(Default)]
pub struct PollBackfillCursor {
    after: Option<String>,
    current: Option<(String, i64)>,
}
impl PollBackfillCursor {
    pub fn current(&self) -> Option<(&str, i64)> {
        self.current.as_ref().map(|(id, revision)| (id.as_str(), *revision))
    }
}

/// Copies at most the byte budget of new UTF-8 bytes for one owner. Round-robin
/// selection includes both existing captures and turns not admitted yet.
pub fn backfill_turn_poll(
    tx: &Transaction<'_>,
    byte_budget: u64,
    cursor: &mut PollBackfillCursor,
) -> StoreResult<u32> {
    if byte_budget == 0 {
        return Ok(0);
    }
    cursor.current = None;
    let next = next_owner(tx, cursor.after.as_deref())?;
    let next = match next {
        Some(id) => Some(id),
        None if cursor.after.is_some() => next_owner(tx, None)?,
        None => None,
    };
    let Some(id) = next else {
        return Ok(0);
    };
    cursor.after = Some(id.clone());
    let turn: Option<(i64, String, i64)> = tx
        .query_row(
            "SELECT revision, state, length(CAST(payload AS BLOB)) FROM turn_tasks WHERE id=?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((revision, state, total)) = turn else {
        // Only the derived orphan is removed; authoritative rows are untouched.
        tx.execute("DELETE FROM turn_poll_capture WHERE id=?1", [&id])?;
        return Ok(1);
    };
    cursor.current = Some((id.clone(), revision));
    if continue_poll_work(tx, &id, revision, byte_budget)? {
        return Ok(1);
    }
    let total = u64::try_from(total).map_err(|_| Error::Conflict)?;
    if state == "completed" {
        let report_len = report_byte_len(tx, &id)?;
        if report_len > byte_budget {
            remember_poll_capture(tx, &id, revision, &state, report_len)?;
            let wrote = write_report_slice(tx, &id, revision, 0, report_len, byte_budget)?;
            return Ok(u32::from(wrote));
        }
    }
    if total > byte_budget {
        capture_poll_slice(tx, &id, revision, &state, total, 0, byte_budget)?;
        return Ok(1);
    }
    let payload: String =
        tx.query_row("SELECT payload FROM turn_tasks WHERE id=?1", [&id], |row| row.get(0))?;
    write_poll_from_payload(tx, &id, revision, &state, &payload)?;
    Ok(1)
}

/// Whether any turn still needs its poll copy. Reads only, so a caller with nothing to copy
/// does not take a write transaction to find that out.
pub fn turn_poll_outstanding(db: &Connection) -> StoreResult<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM turn_tasks WHERE id NOT IN (SELECT id FROM turn_poll)) \
         OR EXISTS(SELECT 1 FROM turn_poll_capture)",
        [],
        |row| row.get(0),
    )
    .map_err(Error::from)
}

fn next_owner(tx: &Transaction<'_>, after: Option<&str>) -> StoreResult<Option<String>> {
    tx.query_row(
        "SELECT id FROM ( \
         SELECT id FROM turn_tasks WHERE id NOT IN (SELECT id FROM turn_poll) \
         UNION SELECT id FROM turn_poll_capture) \
         WHERE (?1 IS NULL OR id>?1) ORDER BY id LIMIT 1",
        [after],
        |row| row.get(0),
    )
    .optional()
    .map_err(Error::from)
}

fn continue_poll_work(
    tx: &Transaction<'_>,
    id: &str,
    live_revision: i64,
    byte_budget: u64,
) -> StoreResult<bool> {
    let current: Option<(i64, String, i64, i64)> = tx
        .query_row(
            "SELECT revision, state, total_len, copied FROM turn_poll_capture WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((revision, state, total, copied)) = current else {
        return Ok(false);
    };
    if live_revision != revision {
        tx.execute("DELETE FROM agent_visible_body WHERE owner=?1 AND source='completion'", [id])?;
        tx.execute("DELETE FROM turn_poll_capture WHERE id=?1", [id])?;
        return Ok(false);
    }
    if state == "completed" {
        let report_len = report_byte_len(tx, id)?;
        let written = completion_body_bytes(tx, id)?;
        if written < report_len {
            return write_report_slice(tx, id, revision, written, report_len, byte_budget);
        }
    } else {
        let total = u64::try_from(total).map_err(|_| Error::Conflict)?;
        let copied = u64::try_from(copied).map_err(|_| Error::Conflict)?;
        if copied < total {
            capture_poll_slice(tx, id, revision, &state, total, copied, byte_budget)?;
            return Ok(true);
        }
    }
    publish_poll_marker(tx, id)?;
    tx.execute("DELETE FROM turn_poll_capture WHERE id=?1", [id])?;
    Ok(true)
}

fn report_byte_len(tx: &Transaction<'_>, id: &str) -> StoreResult<u64> {
    let len: i64 = tx.query_row(
        "SELECT COALESCE(length(CAST(json_extract(payload, '$.state.final_report') AS BLOB)), 0) \
         FROM turn_tasks WHERE id=?1",
        [id],
        |row| row.get(0),
    )?;
    u64::try_from(len).map_err(|_| Error::Conflict)
}

fn completion_body_bytes(tx: &Transaction<'_>, id: &str) -> StoreResult<u64> {
    let len: i64 = tx.query_row(
        "SELECT COALESCE(SUM(byte_len), 0) FROM agent_visible_body \
         WHERE owner=?1 AND source='completion'",
        [id],
        |row| row.get(0),
    )?;
    u64::try_from(len).map_err(|_| Error::Conflict)
}

fn remember_poll_capture(
    tx: &Transaction<'_>,
    id: &str,
    revision: i64,
    state: &str,
    report_len: u64,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO turn_poll_capture(id,revision,state,total_len,copied,payload) \
         VALUES(?1,?2,?3,?4,0,zeroblob(0)) \
         ON CONFLICT(id) DO NOTHING",
        params![id, revision, state, i64::try_from(report_len).map_err(|_| Error::Conflict)?],
    )?;
    Ok(())
}

fn write_report_slice(
    tx: &Transaction<'_>,
    id: &str,
    revision: i64,
    written: u64,
    report_len: u64,
    byte_budget: u64,
) -> StoreResult<bool> {
    let take = byte_budget.min(report_len.saturating_sub(written));
    if take == 0 {
        return Ok(false);
    }
    let bytes: Vec<u8> = tx.query_row(
        "SELECT substr(CAST(json_extract(payload, '$.state.final_report') AS BLOB), ?2, ?3) \
         FROM turn_tasks WHERE id=?1",
        params![
            id,
            i64::try_from(written.saturating_add(1)).map_err(|_| Error::Conflict)?,
            i64::try_from(take).map_err(|_| Error::Conflict)?
        ],
        |row| row.get(0),
    )?;
    let piece = snap_utf8_prefix(&bytes);
    if piece.is_empty() {
        return Ok(false);
    }
    let generation = completion_generation(id, revision, report_len, piece, written == 0);
    let stored = stored_completion_revision(tx, id)?.unwrap_or(generation);
    append_completion_bytes(tx, id, &stored, piece)?;
    Ok(true)
}

fn completion_generation(
    id: &str,
    revision: i64,
    report_len: u64,
    first: &[u8],
    fresh: bool,
) -> String {
    if !fresh {
        return String::new();
    }
    let mut material = Vec::new();
    material.extend_from_slice(id.as_bytes());
    material.extend_from_slice(&revision.to_le_bytes());
    material.extend_from_slice(&report_len.to_le_bytes());
    material.extend_from_slice(&first[..first.len().min(32)]);
    ContentHash::blake3(&material).hex
}

fn stored_completion_revision(tx: &Transaction<'_>, id: &str) -> StoreResult<Option<String>> {
    tx.query_row(
        "SELECT revision FROM agent_visible_body WHERE owner=?1 AND source='completion' \
         ORDER BY ordinal LIMIT 1",
        [id],
        |row| row.get(0),
    )
    .optional()
    .map_err(Error::from)
}

fn snap_utf8_prefix(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    &bytes[..end]
}

const COMPLETION_CHUNK: usize = 64 * 1024;

fn append_completion_bytes(
    tx: &Transaction<'_>,
    id: &str,
    revision: &str,
    mut bytes: &[u8],
) -> StoreResult<()> {
    while !bytes.is_empty() {
        let last: Option<(i64, i64)> = tx
            .query_row(
                "SELECT ordinal, byte_len FROM agent_visible_body \
                 WHERE owner=?1 AND source='completion' AND revision=?2 \
                 ORDER BY ordinal DESC LIMIT 1",
                params![id, revision],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((ordinal, len)) = last {
            let len = usize::try_from(len).map_err(|_| Error::Conflict)?;
            if len < COMPLETION_CHUNK {
                let room = COMPLETION_CHUNK - len;
                let mut end = room.min(bytes.len());
                while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
                    end -= 1;
                }
                // One byte of room cannot hold the next character. Close this
                // short chunk and open another; a block does not have to be full.
                if end > 0 {
                    let piece = std::str::from_utf8(&bytes[..end]).map_err(|_| Error::Conflict)?;
                    tx.execute(
                        "UPDATE agent_visible_body SET text = text || ?4, byte_len = byte_len + ?5 \
                         WHERE owner=?1 AND source='completion' AND revision=?2 AND ordinal=?3",
                        params![
                            id,
                            revision,
                            ordinal,
                            piece,
                            i64::try_from(end).map_err(|_| Error::Conflict)?
                        ],
                    )?;
                    bytes = &bytes[end..];
                    continue;
                }
            }
        }
        let ordinal = last.map(|(ordinal, _)| ordinal.saturating_add(1)).unwrap_or(0);
        let mut end = COMPLETION_CHUNK.min(bytes.len());
        while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
            end -= 1;
        }
        if end == 0 {
            return Err(Error::Conflict);
        }
        let piece = std::str::from_utf8(&bytes[..end]).map_err(|_| Error::Conflict)?;
        tx.execute(
            "INSERT INTO agent_visible_body(owner, source, revision, ordinal, byte_len, text) \
             VALUES(?1,'completion',?2,?3,?4,?5)",
            params![id, revision, ordinal, i64::try_from(end).map_err(|_| Error::Conflict)?, piece],
        )?;
        bytes = &bytes[end..];
    }
    Ok(())
}

fn publish_poll_marker(tx: &Transaction<'_>, id: &str) -> StoreResult<()> {
    let present: bool =
        tx.query_row("SELECT EXISTS(SELECT 1 FROM turn_poll WHERE id=?1)", [id], |row| row.get(0))?;
    if present {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO turn_poll(id, revision, state, payload) \
         SELECT id, revision, state, json_replace(payload, '$.state.final_report', '') \
         FROM turn_tasks WHERE id=?1",
        [id],
    )?;
    Ok(())
}

fn capture_poll_slice(
    tx: &Transaction<'_>,
    id: &str,
    revision: i64,
    state: &str,
    total: u64,
    copied: u64,
    byte_budget: u64,
) -> StoreResult<()> {
    let take = byte_budget.min(total.saturating_sub(copied));
    if take == 0 {
        return Ok(());
    }
    let piece: Vec<u8> = tx.query_row(
        "SELECT substr(CAST(payload AS BLOB), ?2, ?3) FROM turn_tasks WHERE id=?1",
        params![
            id,
            i64::try_from(copied.saturating_add(1)).map_err(|_| Error::Conflict)?,
            i64::try_from(take).map_err(|_| Error::Conflict)?
        ],
        |row| row.get(0),
    )?;
    let next = copied.saturating_add(piece.len() as u64);
    tx.execute(
        "INSERT INTO turn_poll_capture(id,revision,state,total_len,copied,payload) VALUES(?1,?2,?3,?4,?5,?6) \
         ON CONFLICT(id) DO UPDATE SET copied=excluded.copied, \
         payload=CAST(turn_poll_capture.payload || excluded.payload AS BLOB)",
        params![id, revision, state, i64::try_from(total).map_err(|_| Error::Conflict)?, i64::try_from(next).map_err(|_| Error::Conflict)?, piece],
    )?;
    if next >= total && state != "completed" {
        publish_poll_marker(tx, id)?;
        tx.execute("DELETE FROM turn_poll_capture WHERE id=?1", [id])?;
    }
    Ok(())
}

fn write_poll_from_payload(
    tx: &Transaction<'_>,
    id: &str,
    revision: i64,
    state: &str,
    payload: &str,
) -> StoreResult<()> {
    let mut value: serde_json::Value = aura_persistence_core::record::decode(payload)?;
    if state == "completed" {
        let turn: TurnTask = aura_persistence_core::record::decode(payload)?;
        if let TurnTaskState::Completed { final_report } = &turn.state {
            let present: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_visible_body WHERE owner=?1 AND source='completion')",
                [id],
                |row| row.get(0),
            )?;
            if !present {
                history::record_chunks(
                    tx,
                    &history::ChunkBody {
                        owner: id.to_owned(),
                        source: history::VisibleSource::Completion,
                        revision: ContentHash::blake3(final_report.as_bytes()),
                    },
                    final_report,
                    UtcTimestamp::now(),
                )?;
            }
        }
    }
    if let Some(report) = value.pointer_mut("/state/final_report") {
        *report = serde_json::Value::String(String::new());
    }
    tx.execute(
        "INSERT INTO turn_poll(id,revision,state,payload) VALUES(?1,?2,?3,?4)",
        params![id, revision, state, aura_persistence_core::record::encode(&value)?],
    )?;
    Ok(())
}
