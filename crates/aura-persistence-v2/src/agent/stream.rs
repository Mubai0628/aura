//! Only normalized, user-visible deltas are projected. Partial tool arguments
//! never become executable calls or model history. Sealed output is authoritative.
use super::history::{VisibleWindow, WindowWriter};
use super::{facts, turns};
use aura_agent_domain::ModelStepState;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use tail::{index_has_byte_gap, mark_coverage};

mod tail;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Delta {
    attempt: TransportAttemptId,
    sequence: u64,
    text: String,
    reasoning: String,
}

mod append;
pub use append::append;
mod repair;
pub use repair::{
    RepairScan, advance_repair_cursor, discover_outstanding, repair_outstanding, repair_pass,
};

/// Gives every earlier delta of this attempt a UTF-8 byte index row.
///
/// `length()` on SQLite text counts characters, and a page offset counts bytes. A backfill
/// that stored the character count placed the next page in the middle of a multibyte
/// character. Those rows stay (events are immutable) but they are not authoritative: a row
/// is authoritative only when `byte_lengths` is set, and a later read ignores the others.
///
/// When the recorded epoch already covers every sequence before `next_sequence`, this does
/// not look at the old payloads again. A gap, or an index written before byte lengths,
/// is repaired once from the payloads' byte lengths.
fn backfill_index(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    next_sequence: u64,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if next_sequence == 0 {
        return Err(Error::Conflict);
    }
    // Provider sequences include events that are not visible text. Coverage is the
    // previous visible delta of this attempt, not `next_sequence - 1`, which may
    // never have been a text row and would force this backfill on every append.
    if previous_visible_is_indexed(tx, owner, attempt, next_sequence)? {
        return Ok(());
    }
    let mut missing = tx.prepare(
        "SELECT json_extract(payload,'$.sequence'), \
         COALESCE(length(CAST(json_extract(payload,'$.text') AS BLOB)), 0), \
         COALESCE(length(CAST(json_extract(payload,'$.reasoning') AS BLOB)), 0) FROM events \
         WHERE owner=?1 AND kind='agent.visible_delta' AND version=1 \
         AND json_extract(payload,'$.attempt')=?2 \
         AND json_extract(payload,'$.sequence') NOT IN \
         (SELECT json_extract(payload,'$.sequence') FROM events \
          WHERE owner=?1 AND kind='agent.visible_delta_index' AND version=1 \
          AND json_extract(payload,'$.attempt')=?2 \
          AND json_extract(payload,'$.byte_lengths')=1) \
         ORDER BY sequence",
    )?;
    let rows = missing.query_map(params![owner, attempt.to_string()], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
    })?;
    let mut pending = Vec::new();
    for row in rows {
        let (sequence, text_len, reasoning_len) = row?;
        pending.push(DeltaIndex {
            attempt,
            sequence: u64::try_from(sequence).map_err(|_| Error::Conflict)?,
            text_len: u64::try_from(text_len).map_err(|_| Error::Conflict)?,
            reasoning_len: u64::try_from(reasoning_len).map_err(|_| Error::Conflict)?,
            byte_lengths: true,
            text_end: 0,
            reasoning_end: 0,
        });
    }
    drop(missing);
    assign_backfill_ends(tx, owner, attempt, &mut pending)?;
    for entry in pending {
        facts::append(tx, EventId::new(), owner, "agent.visible_delta_index", &entry, at)?;
        sync_visible_index(tx, owner, &entry)?;
    }
    Ok(())
}

/// Gives a prefix that had no index a cumulative byte end, starting from zero
/// when nothing was indexed yet, or from the last addressed row before the hole.
///
/// A hole after an unaddressed row cannot be placed, so those lengths stay at
/// zero and the page walks the index in order instead of seeking.
fn assign_backfill_ends(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    pending: &mut [DeltaIndex],
) -> StoreResult<()> {
    let Some(first) = pending.first() else { return Ok(()) };
    let earlier: Option<String> = tx
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_delta_index' \
             AND version=1 AND json_extract(payload,'$.attempt')=?2 \
             AND json_extract(payload,'$.byte_lengths')=1 \
             AND json_extract(payload,'$.sequence') < ?3 \
             ORDER BY sequence DESC LIMIT 1",
            params![
                owner,
                attempt.to_string(),
                i64::try_from(first.sequence).map_err(|_| Error::Conflict)?
            ],
            |row| row.get(0),
        )
        .optional()?;
    let (mut text_end, mut reasoning_end) = if let Some(raw) = earlier {
        let entry: DeltaIndex = record::decode(&raw)?;
        if (entry.text_len > 0 && entry.text_end == 0)
            || (entry.reasoning_len > 0 && entry.reasoning_end == 0)
        {
            return Ok(());
        }
        (entry.text_end, entry.reasoning_end)
    } else {
        (0, 0)
    };
    for entry in pending.iter_mut() {
        text_end = text_end.saturating_add(entry.text_len);
        reasoning_end = reasoning_end.saturating_add(entry.reasoning_len);
        entry.text_end = text_end;
        entry.reasoning_end = reasoning_end;
    }
    Ok(())
}

/// Rewrites derived byte lengths for one step's latest attempt, without touching the deltas.
///
/// Derived index rows can be appended again. The original events and their order stay. A
/// read that finds a byte-length epoch does not scan the payloads to rediscover their sizes.
pub fn repair_byte_index(
    tx: &Transaction<'_>,
    step: ModelStepId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let owner = step.to_string();
    let Some(attempt) = latest_attempt(tx, step)? else { return Ok(()) };
    let max_sequence: Option<i64> = tx
        .query_row(
            "SELECT MAX(json_extract(payload,'$.sequence')) FROM events \
             WHERE owner=?1 AND kind='agent.visible_delta' AND version=1 \
             AND json_extract(payload,'$.attempt')=?2",
            params![owner, attempt.to_string()],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let Some(max_sequence) = max_sequence else { return Ok(()) };
    let max_sequence = u64::try_from(max_sequence).map_err(|_| Error::Conflict)?;
    backfill_index(tx, &owner, attempt, max_sequence.saturating_add(1), at)?;
    publish_visible_index(tx, &owner, attempt)?;
    publish_visible_deltas(tx, &owner, attempt)?;
    mark_coverage(tx, &owner, attempt)?;
    record_epoch(tx, &owner, attempt, max_sequence, at)
}

fn previous_visible_is_indexed(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    next_sequence: u64,
) -> StoreResult<bool> {
    let previous: Option<i64> = tx
        .query_row(
            "SELECT MAX(json_extract(payload,'$.sequence')) FROM events \
             WHERE owner=?1 AND kind='agent.visible_delta' AND version=1 \
             AND json_extract(payload,'$.attempt')=?2 \
             AND json_extract(payload,'$.sequence') < ?3",
            params![
                owner,
                attempt.to_string(),
                i64::try_from(next_sequence).map_err(|_| Error::Conflict)?
            ],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let Some(previous) = previous else {
        return Ok(true);
    };
    let through = u64::try_from(previous).map_err(|_| Error::Conflict)?;
    if !epoch_covers(tx, owner, attempt, through)? {
        return Ok(false);
    }
    // An epoch says the event index reached this sequence. It does not say the
    // locatable table still contains that prefix. A missing row is published from
    // the event index inside this transaction; if that cannot restore it, the
    // caller rebuilds the index instead of appending against a hole.
    let present = relation_has_sequence(tx, owner, attempt, previous)?;
    if present && !index_has_byte_gap(tx, owner, attempt)? {
        return Ok(true);
    }
    // The previous sequence can still be present while a middle row is gone.
    // Republish from the event index before this append, and do not treat the
    // remaining rows as a complete generation.
    publish_visible_index(tx, owner, attempt)?;
    publish_visible_deltas(tx, owner, attempt)?;
    Ok(relation_has_sequence(tx, owner, attempt, previous)?
        && !index_has_byte_gap(tx, owner, attempt)?)
}

fn relation_has_sequence(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    sequence: i64,
) -> StoreResult<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_visible_index WHERE owner=?1 AND attempt=?2 AND sequence=?3)",
        params![owner, attempt.to_string(), sequence],
        |row| row.get(0),
    )
    .map_err(Error::from)
}

fn epoch_covers(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    through_sequence: u64,
) -> StoreResult<bool> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_delta_epoch' \
             AND version=1 ORDER BY sequence DESC LIMIT 1",
            [owner],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(false) };
    let epoch: DeltaEpoch = record::decode(&raw)?;
    Ok(epoch.attempt == attempt
        && epoch.unit == BYTE_LENGTHS
        && epoch.through_sequence >= through_sequence)
}

fn record_epoch(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    through_sequence: u64,
    at: UtcTimestamp,
) -> StoreResult<()> {
    facts::append(
        tx,
        EventId::new(),
        owner,
        "agent.visible_delta_epoch",
        &DeltaEpoch { attempt, through_sequence, unit: BYTE_LENGTHS.to_owned() },
        at,
    )
}

const BYTE_LENGTHS: &str = "utf8_bytes";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeltaEpoch {
    attempt: TransportAttemptId,
    through_sequence: u64,
    unit: String,
}

/// The length of one appended delta, so a page can find its own range.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeltaIndex {
    attempt: TransportAttemptId,
    sequence: u64,
    text_len: u64,
    reasoning_len: u64,
    /// Set only when `text_len` and `reasoning_len` are UTF-8 byte lengths. Rows written by
    /// the character-count backfill omit it and must not place a page.
    #[serde(default)]
    byte_lengths: bool,
    /// Byte offset where this delta's text ends in the attempt. Zero on rows written
    /// before the cumulative index, which a page then cannot seek.
    #[serde(default)]
    text_end: u64,
    /// Byte offset where this delta's reasoning ends in the attempt.
    #[serde(default)]
    reasoning_end: u64,
}

fn latest_attempt(db: &Connection, step: ModelStepId) -> StoreResult<Option<TransportAttemptId>> {
    let value: Option<String> = db.query_row("SELECT a.id FROM transport_attempts a JOIN model_steps s ON s.snapshot_id=a.snapshot_id WHERE s.id=?1 ORDER BY a.rowid DESC LIMIT 1",
        [step.to_string()], |r| r.get(0)).optional()?;
    value.map(|id| id.parse().map_err(|_| Error::Conflict)).transpose()
}

/// The visible body of a running step: the deltas of the newest transport
/// attempt, in sequence order.
pub struct VisibleBody {
    pub text: String,
    pub reasoning: String,
    pub sequence: Option<u64>,
    /// Opaque identity of the attempt that produced this body. The text of one
    /// attempt only ever grows, so a client that read a prefix of it can keep
    /// appending; a different identity means a retry restarted the stream and
    /// the bytes the client holds are not a prefix of this body.
    pub body_id: Option<String>,
}

pub fn partial(db: &Connection, step: ModelStepId) -> StoreResult<VisibleBody> {
    let owner = step.to_string();
    let Some(attempt) = latest_attempt(db, step)? else {
        return Ok(VisibleBody {
            text: String::new(),
            reasoning: String::new(),
            sequence: None,
            body_id: None,
        });
    };
    let mut q = db.prepare("SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_delta' AND version=1 ORDER BY sequence")?;
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut sequence = None;
    for row in q.query_map([owner], |r| r.get::<_, String>(0))? {
        let delta: Delta = record::decode(&row?)?;
        if delta.attempt == attempt {
            text.push_str(&delta.text);
            reasoning.push_str(&delta.reasoning);
            sequence = Some(delta.sequence);
        }
    }
    Ok(VisibleBody { text, reasoning, sequence, body_id: Some(attempt.to_string()) })
}

/// One bounded window of a live body, plus the identity of the body it came from.
///
/// A page, or a preview, is copied out of the stored deltas instead of concatenating every
/// delta the attempt ever produced: each row is pushed through the window walk and dropped,
/// so the memory and the copying stay bounded by the window rather than by the body. The
/// deltas of one attempt are visited in the same order the whole-body reader uses, so the
/// offsets and the text are the same ones that reader would produce.
pub struct VisibleWindows {
    pub text: crate::agent::history::VisibleWindow,
    pub reasoning: crate::agent::history::VisibleWindow,
    pub sequence: Option<u64>,
    pub body_id: Option<String>,
}

pub fn partial_windows(
    db: &Connection,
    step: ModelStepId,
    offset: u64,
    limit: u64,
) -> StoreResult<VisibleWindows> {
    partial_windows_for(db, step, offset, limit, WindowSide::Both)
}

/// Which live bodies a page actually reads. A text page does not fetch reasoning.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowSide {
    Text,
    Reasoning,
    Both,
}

pub fn partial_windows_for(
    db: &Connection,
    step: ModelStepId,
    offset: u64,
    limit: u64,
    side: WindowSide,
) -> StoreResult<VisibleWindows> {
    let owner = step.to_string();
    let Some(attempt) = latest_attempt(db, step)? else {
        return Ok(VisibleWindows {
            text: VisibleWindow::whole(String::new()),
            reasoning: VisibleWindow::whole(String::new()),
            sequence: None,
            body_id: None,
        });
    };
    // Both sources are read at the same offset: a page request names one of them, and the
    // other is read as a preview from the caller's offset (zero for a preview). Reasoning
    // is a body of its own, so its pages continue from where the reader really was.
    let mut text_window = WindowWriter::new(offset, limit);
    let mut reasoning_window = WindowWriter::new(offset, limit);
    let mut sequence = None;
    // Lengths place every delta. Only a delta that intersects a window is fetched, and only
    // the bytes that window can use — a later delta is counted, not read, and a reasoning
    // page does not fetch assistant text. An index that is missing or was stored in
    // characters is not trusted; the byte lengths are taken from the payloads instead.
    let copy_from = offset.saturating_sub(3);
    let copy_to = offset.saturating_add(limit).saturating_add(3);
    // An unused side is an empty range. A sentinel past i64::MAX cannot be a SQL
    // parameter, and every real offset is non-negative, so nothing overlaps [0, 0).
    let (text_from, text_to) =
        if side == WindowSide::Reasoning { (0, 0) } else { (copy_from, copy_to) };
    let (reasoning_from, reasoning_to) =
        if side == WindowSide::Text { (0, 0) } else { (copy_from, copy_to) };
    let sought = seek_index(db, &owner, attempt, text_from, text_to, reasoning_from, reasoning_to)?;
    let mut latest_sequence = None;
    let mut absolute = false;
    let (index, text_total, reasoning_total) = match sought {
        Some(located) => {
            latest_sequence = located.sequence;
            absolute = true;
            (located.rows, located.text_total, located.reasoning_total)
        }
        None => {
            let index = page_lengths(db, &owner, attempt)?;
            let text_total = index.iter().map(|entry| entry.text_len).fold(0, u64::saturating_add);
            let reasoning_total =
                index.iter().map(|entry| entry.reasoning_len).fold(0, u64::saturating_add);
            (index, text_total, reasoning_total)
        }
    };
    let mut text_start = 0_u64;
    let mut reasoning_start = 0_u64;
    let mut text_cursor = 0_u64;
    let mut reasoning_cursor = 0_u64;
    let mut text_decoded = 0_u64;
    let mut reasoning_decoded = 0_u64;
    let mut text_chunks = 0_u32;
    let mut reasoning_chunks = 0_u32;
    for entry in &index {
        sequence = Some(entry.sequence);
        let text_at = if absolute && entry.text_end > 0 {
            entry.text_end.saturating_sub(entry.text_len)
        } else {
            text_start
        };
        let reasoning_at = if absolute && entry.reasoning_end > 0 {
            entry.reasoning_end.saturating_sub(entry.reasoning_len)
        } else {
            reasoning_start
        };
        if side != WindowSide::Reasoning {
            if text_cursor < text_at {
                text_window.advance(text_at - text_cursor);
                text_cursor = text_at;
            }
            let text_hit = intersects(text_at, entry.text_len, copy_from, copy_to);
            if text_hit {
                text_decoded = text_decoded.saturating_add(push_span(
                    DeltaLocator {
                        db,
                        owner: &owner,
                        attempt,
                        sequence: entry.sequence,
                        field: "text",
                    },
                    text_at,
                    entry.text_len,
                    &mut text_window,
                    copy_from,
                    copy_to,
                )?);
                text_chunks = text_chunks.saturating_add(1);
            } else {
                text_window.advance(entry.text_len);
            }
            text_cursor = text_cursor.saturating_add(entry.text_len);
        }
        if side != WindowSide::Text {
            if reasoning_cursor < reasoning_at {
                reasoning_window.advance(reasoning_at - reasoning_cursor);
                reasoning_cursor = reasoning_at;
            }
            let reasoning_hit = intersects(reasoning_at, entry.reasoning_len, copy_from, copy_to);
            if reasoning_hit {
                reasoning_decoded = reasoning_decoded.saturating_add(push_span(
                    DeltaLocator {
                        db,
                        owner: &owner,
                        attempt,
                        sequence: entry.sequence,
                        field: "reasoning",
                    },
                    reasoning_at,
                    entry.reasoning_len,
                    &mut reasoning_window,
                    copy_from,
                    copy_to,
                )?);
                reasoning_chunks = reasoning_chunks.saturating_add(1);
            } else {
                reasoning_window.advance(entry.reasoning_len);
            }
            reasoning_cursor = reasoning_cursor.saturating_add(entry.reasoning_len);
        }
        text_start = text_start.saturating_add(entry.text_len);
        reasoning_start = reasoning_start.saturating_add(entry.reasoning_len);
    }
    if side != WindowSide::Reasoning && text_cursor < text_total {
        text_window.advance(text_total - text_cursor);
    }
    if side != WindowSide::Text && reasoning_cursor < reasoning_total {
        reasoning_window.advance(reasoning_total - reasoning_cursor);
    }
    // A page may touch only the first deltas. The sequence is still the attempt's
    // latest visible delta, the same one the whole-body reader reports.
    if let Some(at) = latest_sequence {
        sequence = Some(at);
    }
    let mut text = text_window.finish();
    let mut reasoning = reasoning_window.finish();
    text.chunks_read = text_chunks;
    text.decoded_bytes = text_decoded;
    reasoning.chunks_read = reasoning_chunks;
    reasoning.decoded_bytes = reasoning_decoded;
    Ok(VisibleWindows { text, reasoning, sequence, body_id: Some(attempt.to_string()) })
}

fn index_has_unaddressed_row(
    db: &Connection,
    owner: &str,
    attempt: TransportAttemptId,
) -> StoreResult<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM events WHERE owner=?1 AND kind='agent.visible_delta_index' \
         AND version=1 AND json_extract(payload,'$.attempt')=?2 \
         AND json_extract(payload,'$.byte_lengths')=1 AND ( \
         (CAST(json_extract(payload,'$.text_len') AS INTEGER) > 0 \
          AND CAST(json_extract(payload,'$.text_end') AS INTEGER) = 0) \
         OR (CAST(json_extract(payload,'$.reasoning_len') AS INTEGER) > 0 \
          AND CAST(json_extract(payload,'$.reasoning_end') AS INTEGER) = 0)))",
        params![owner, attempt.to_string()],
        |row| row.get(0),
    )
    .map_err(Error::from)
}

fn intersects(start: u64, len: u64, from: u64, to: u64) -> bool {
    len > 0 && start < to && start.saturating_add(len) > from
}

/// Copies the part of one delta that can fall inside the window, then moves the cursor to
/// the end of that delta. The returned count is the number of bytes SQLite handed back.
struct DeltaLocator<'a> {
    db: &'a Connection,
    owner: &'a str,
    attempt: TransportAttemptId,
    sequence: u64,
    field: &'static str,
}

fn push_span(
    locator: DeltaLocator<'_>,
    piece_start: u64,
    piece_len: u64,
    window: &mut WindowWriter,
    copy_from: u64,
    copy_to: u64,
) -> StoreResult<u64> {
    let local_from = copy_from.saturating_sub(piece_start).min(piece_len);
    let local_to = copy_to.saturating_sub(piece_start).min(piece_len);
    if local_to <= local_from {
        window.advance(piece_len);
        return Ok(0);
    }
    let pad_from = local_from.saturating_sub(3);
    let pad_to = local_to.saturating_add(3).min(piece_len);
    let bytes = delta_bytes(&locator, pad_from, pad_to - pad_from)?;
    let (lead, text) = trim_utf8(&bytes);
    let lead = u64::try_from(lead).map_err(|_| Error::Conflict)?;
    window.advance(pad_from.saturating_add(lead));
    window.push(text);
    let consumed = pad_from.saturating_add(lead).saturating_add(text.len() as u64);
    window.advance(piece_len.saturating_sub(consumed));
    Ok(bytes.len() as u64)
}

fn trim_utf8(bytes: &[u8]) -> (usize, &str) {
    let mut start = 0;
    while start < bytes.len() && bytes[start] & 0b1100_0000 == 0b1000_0000 {
        start += 1;
    }
    let mut end = bytes.len();
    while end > start && std::str::from_utf8(&bytes[start..end]).is_err() {
        end -= 1;
    }
    (start, std::str::from_utf8(&bytes[start..end]).unwrap_or(""))
}

/// Rows whose cumulative text overlaps `[from, to)`, stopping at the first end that
/// reaches `to`. The upper bound is the indexed `text_end`, so a first or middle
/// page does not walk the rest of the attempt.
pub const TEXT_WINDOW_SQL: &str = "SELECT sequence, text_len, text_end, reasoning_len, reasoning_end \
     FROM agent_visible_index INDEXED BY agent_visible_index_text \
     WHERE owner=?1 AND attempt=?2 AND text_end > ?3 \
     AND text_end <= IFNULL((SELECT MIN(text_end) FROM agent_visible_index INDEXED BY agent_visible_index_text \
         WHERE owner=?1 AND attempt=?2 AND text_end >= ?4), ?4) \
     AND text_start < ?4 \
     ORDER BY text_end";

/// Rows whose cumulative reasoning overlaps `[from, to)`, with the same indexed stop.
pub const REASONING_WINDOW_SQL: &str = "SELECT sequence, text_len, text_end, reasoning_len, reasoning_end \
     FROM agent_visible_index INDEXED BY agent_visible_index_reasoning \
     WHERE owner=?1 AND attempt=?2 AND reasoning_end > ?3 \
     AND reasoning_end <= IFNULL((SELECT MIN(reasoning_end) FROM agent_visible_index INDEXED BY agent_visible_index_reasoning \
         WHERE owner=?1 AND attempt=?2 AND reasoning_end >= ?4), ?4) \
     AND reasoning_start < ?4 \
     ORDER BY reasoning_end";

/// The bytes of one delta, addressed by the primary key rather than the owner's event history.
pub const DELTA_TEXT_SQL: &str = "SELECT substr(CAST(text AS BLOB), ?4, ?5) FROM agent_visible_delta \
     WHERE owner=?1 AND attempt=?2 AND sequence=?3";

/// The reasoning bytes of one delta, addressed by the same primary key.
pub const DELTA_REASONING_SQL: &str = "SELECT substr(CAST(reasoning AS BLOB), ?4, ?5) FROM agent_visible_delta \
     WHERE owner=?1 AND attempt=?2 AND sequence=?3";

fn delta_bytes(locator: &DeltaLocator<'_>, start: u64, len: u64) -> StoreResult<Vec<u8>> {
    let direct = match locator.field {
        "text" => DELTA_TEXT_SQL,
        "reasoning" => DELTA_REASONING_SQL,
        _ => return Err(Error::Conflict),
    };
    // A repair row holds only the bytes copied so far. Reading it as the whole
    // delta would drop the rest, so an unfinished sequence stays on the event.
    if delta_copy_is_complete(locator)?
        && let Some(bytes) = delta_slice(locator, direct, start, len)?
    {
        return Ok(bytes);
    }
    let sql = match locator.field {
        "text" => {
            "SELECT substr(CAST(json_extract(payload,'$.text') AS BLOB), ?4, ?5) FROM events \
             WHERE owner=?1 AND kind='agent.visible_delta' AND version=1 \
             AND json_extract(payload,'$.attempt')=?2 AND json_extract(payload,'$.sequence')=?3 \
             LIMIT 1"
        }
        "reasoning" => {
            "SELECT substr(CAST(json_extract(payload,'$.reasoning') AS BLOB), ?4, ?5) FROM events \
             WHERE owner=?1 AND kind='agent.visible_delta' AND version=1 \
             AND json_extract(payload,'$.attempt')=?2 AND json_extract(payload,'$.sequence')=?3 \
             LIMIT 1"
        }
        _ => return Err(Error::Conflict),
    };
    delta_slice(locator, sql, start, len)?.ok_or(Error::Conflict)
}

fn delta_slice(
    locator: &DeltaLocator<'_>,
    sql: &str,
    start: u64,
    len: u64,
) -> StoreResult<Option<Vec<u8>>> {
    locator
        .db
        .query_row(
            sql,
            params![
                locator.owner,
                locator.attempt.to_string(),
                i64::try_from(locator.sequence).map_err(|_| Error::Conflict)?,
                i64::try_from(start.saturating_add(1)).map_err(|_| Error::Conflict)?,
                i64::try_from(len).map_err(|_| Error::Conflict)?,
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(Error::from)
}

/// Byte lengths of one attempt, preferring an index that was stored in bytes.
///
/// A character-count index, or a missing one, is not used to place the page. The lengths
/// then come from the payloads as integers, and the page still fetches only the span it
/// shows. Once a byte-length epoch covers the attempt, later pages do not take that pass.
fn index_ends(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
) -> StoreResult<(u64, u64, bool)> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_delta_index' \
             AND version=1 AND json_extract(payload,'$.attempt')=?2 \
             ORDER BY sequence DESC LIMIT 1",
            params![owner, attempt.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok((0, 0, true));
    };
    let entry: DeltaIndex = record::decode(&raw)?;
    let known = entry.text_end > 0
        || entry.reasoning_end > 0
        || (entry.text_len == 0 && entry.reasoning_len == 0);
    if known { Ok((entry.text_end, entry.reasoning_end, true)) } else { Ok((0, 0, false)) }
}

/// Index rows that overlap the requested windows, plus the attempt totals.
///
/// `None` means the cumulative ends are not trustworthy, so the caller reads the
/// whole index. A trustworthy index returns only the rows the page can touch.
struct SoughtIndex {
    rows: Vec<DeltaIndex>,
    text_total: u64,
    reasoning_total: u64,
    sequence: Option<u64>,
}

fn sync_visible_index(tx: &Transaction<'_>, owner: &str, entry: &DeltaIndex) -> StoreResult<()> {
    if !entry.byte_lengths {
        return Ok(());
    }
    let text_start = entry.text_end.saturating_sub(entry.text_len);
    let reasoning_start = entry.reasoning_end.saturating_sub(entry.reasoning_len);
    tx.execute(
        "INSERT INTO agent_visible_index(owner,attempt,sequence,text_start,text_end,text_len,reasoning_start,reasoning_end,reasoning_len) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) \
         ON CONFLICT(owner,attempt,sequence) DO UPDATE SET \
         text_start=excluded.text_start, text_end=excluded.text_end, text_len=excluded.text_len, \
         reasoning_start=excluded.reasoning_start, reasoning_end=excluded.reasoning_end, \
         reasoning_len=excluded.reasoning_len",
        params![
            owner,
            entry.attempt.to_string(),
            i64::try_from(entry.sequence).map_err(|_| Error::Conflict)?,
            i64::try_from(text_start).map_err(|_| Error::Conflict)?,
            i64::try_from(entry.text_end).map_err(|_| Error::Conflict)?,
            i64::try_from(entry.text_len).map_err(|_| Error::Conflict)?,
            i64::try_from(reasoning_start).map_err(|_| Error::Conflict)?,
            i64::try_from(entry.reasoning_end).map_err(|_| Error::Conflict)?,
            i64::try_from(entry.reasoning_len).map_err(|_| Error::Conflict)?,
        ],
    )?;
    Ok(())
}

/// Copies derived index rows into the locatable table. Event rows stay as they were.
fn publish_visible_index(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
) -> StoreResult<()> {
    let mut statement = tx.prepare(
        "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_delta_index' \
         AND version=1 AND json_extract(payload,'$.attempt')=?2 \
         AND json_extract(payload,'$.byte_lengths')=1 ORDER BY sequence",
    )?;
    let rows =
        statement.query_map(params![owner, attempt.to_string()], |row| row.get::<_, String>(0))?;
    let mut entries = Vec::new();
    for row in rows {
        entries.push(record::decode::<DeltaIndex>(&row?)?);
    }
    drop(statement);
    for entry in &entries {
        if entry.attempt == attempt {
            sync_visible_index(tx, owner, entry)?;
        }
    }
    Ok(())
}

fn remember_delta(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    sequence: u64,
    text: &str,
    reasoning: &str,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO agent_visible_delta(owner,attempt,sequence,text,reasoning) VALUES(?1,?2,?3,?4,?5) \
         ON CONFLICT(owner,attempt,sequence) DO UPDATE SET text=excluded.text, reasoning=excluded.reasoning",
        params![
            owner,
            attempt.to_string(),
            i64::try_from(sequence).map_err(|_| Error::Conflict)?,
            text,
            reasoning,
        ],
    )?;
    Ok(())
}

/// Copies delta text into the locatable table. The event rows stay the authority.
fn publish_visible_deltas(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO agent_visible_delta(owner,attempt,sequence,text,reasoning) \
         SELECT ?1, ?2, json_extract(payload,'$.sequence'), \
         COALESCE(json_extract(payload,'$.text'), ''), \
         COALESCE(json_extract(payload,'$.reasoning'), '') \
         FROM events WHERE owner=?1 AND kind='agent.visible_delta' AND version=1 \
         AND json_extract(payload,'$.attempt')=?2 \
         AND json_extract(payload,'$.sequence') NOT IN \
         (SELECT sequence FROM agent_visible_delta WHERE owner=?1 AND attempt=?2) \
         ON CONFLICT(owner,attempt,sequence) DO NOTHING",
        params![owner, attempt.to_string()],
    )?;
    Ok(())
}

fn seek_relation(
    db: &Connection,
    owner: &str,
    attempt: TransportAttemptId,
    text_from: u64,
    text_to: u64,
    reasoning_from: u64,
    reasoning_to: u64,
) -> StoreResult<Option<SoughtIndex>> {
    // One row, or an old epoch, is not a complete generation. The watermark names
    // the first and last sequence this attempt published; either boundary missing
    // sends the page back to the event index, which still holds the prefix.
    let watermark: Option<(i64, i64, i64, i64)> = db
        .query_row(
            "SELECT first_sequence, max_sequence, text_end, reasoning_end \
             FROM agent_visible_coverage WHERE owner=?1 AND attempt=?2",
            params![owner, attempt.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((first_sequence, max_sequence, covered_text, covered_reasoning)) = watermark else {
        return Ok(None);
    };
    let actual_first: Option<i64> = db
        .query_row(
            "SELECT sequence FROM agent_visible_index WHERE owner=?1 AND attempt=?2 \
             ORDER BY sequence ASC LIMIT 1",
            params![owner, attempt.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    if actual_first != Some(first_sequence) {
        return Ok(None);
    }
    let latest: Option<(i64, i64, i64)> = db
        .query_row(
            "SELECT sequence, text_end, reasoning_end FROM agent_visible_index \
             WHERE owner=?1 AND attempt=?2 ORDER BY sequence DESC LIMIT 1",
            params![owner, attempt.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((sequence, text_total, reasoning_total)) = latest else { return Ok(None) };
    if sequence != max_sequence
        || text_total != covered_text
        || reasoning_total != covered_reasoning
    {
        return Ok(None);
    }
    let text_total = u64::try_from(text_total).map_err(|_| Error::Conflict)?;
    let reasoning_total = u64::try_from(reasoning_total).map_err(|_| Error::Conflict)?;
    let text_rows =
        window_rows(db, TEXT_WINDOW_SQL, owner, &attempt.to_string(), text_from, text_to)?;
    let reasoning_rows = window_rows(
        db,
        REASONING_WINDOW_SQL,
        owner,
        &attempt.to_string(),
        reasoning_from,
        reasoning_to,
    )?;
    // First and last rows can agree with the watermark while a middle row is
    // gone. The page then falls back to the event index instead of dropping
    // the bytes that row held.
    if !window_covers(&text_rows, text_from, text_to, text_total, true)
        || !window_covers(&reasoning_rows, reasoning_from, reasoning_to, reasoning_total, false)
    {
        return Ok(None);
    }
    let mut merged = std::collections::BTreeMap::new();
    for row in text_rows.into_iter().chain(reasoning_rows) {
        merged.insert(row.0, row);
    }
    let mut index = Vec::new();
    for (_, (sequence, text_len, text_end, reasoning_len, reasoning_end)) in merged {
        index.push(DeltaIndex {
            attempt,
            sequence: u64::try_from(sequence).map_err(|_| Error::Conflict)?,
            text_len: u64::try_from(text_len).map_err(|_| Error::Conflict)?,
            reasoning_len: u64::try_from(reasoning_len).map_err(|_| Error::Conflict)?,
            byte_lengths: true,
            text_end: u64::try_from(text_end).map_err(|_| Error::Conflict)?,
            reasoning_end: u64::try_from(reasoning_end).map_err(|_| Error::Conflict)?,
        });
    }
    Ok(Some(SoughtIndex {
        rows: index,
        text_total,
        reasoning_total,
        sequence: Some(u64::try_from(sequence).map_err(|_| Error::Conflict)?),
    }))
}

type WindowRow = (i64, i64, i64, i64, i64);

fn window_rows(
    db: &Connection,
    sql: &str,
    owner: &str,
    attempt: &str,
    from: u64,
    to: u64,
) -> StoreResult<Vec<WindowRow>> {
    let mut statement = db.prepare(sql)?;
    let rows = statement.query_map(
        params![
            owner,
            attempt,
            i64::try_from(from).map_err(|_| Error::Conflict)?,
            i64::try_from(to).map_err(|_| Error::Conflict)?,
        ],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Error::from)
}

fn seek_index(
    db: &Connection,
    owner: &str,
    attempt: TransportAttemptId,
    text_from: u64,
    text_to: u64,
    reasoning_from: u64,
    reasoning_to: u64,
) -> StoreResult<Option<SoughtIndex>> {
    if let Some(indexed) =
        seek_relation(db, owner, attempt, text_from, text_to, reasoning_from, reasoning_to)?
    {
        return Ok(Some(indexed));
    }
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_delta_index' \
             AND version=1 AND json_extract(payload,'$.attempt')=?2 \
             ORDER BY sequence DESC LIMIT 1",
            params![owner, attempt.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let latest: DeltaIndex = record::decode(&raw)?;
    let known = latest.byte_lengths
        && (latest.text_end > 0
            || latest.reasoning_end > 0
            || (latest.text_len == 0 && latest.reasoning_len == 0));
    if !known || !read_epoch_covers(db, owner, attempt, latest.sequence)? {
        return Ok(None);
    }
    // A prefix written before cumulative ends existed is still in the index with
    // a zero end. Seeking past it would drop those bytes, so the page walks
    // every row in order instead.
    if index_has_unaddressed_row(db, owner, attempt)? {
        return Ok(None);
    }
    let mut statement = db.prepare(
        "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_delta_index' \
         AND version=1 AND json_extract(payload,'$.attempt')=?2 \
         AND json_extract(payload,'$.byte_lengths')=1 AND ( \
         (CAST(json_extract(payload,'$.text_end') AS INTEGER) > ?3 \
          AND CAST(json_extract(payload,'$.text_end') AS INTEGER) \
              - CAST(json_extract(payload,'$.text_len') AS INTEGER) < ?4) \
         OR (CAST(json_extract(payload,'$.reasoning_end') AS INTEGER) > ?5 \
          AND CAST(json_extract(payload,'$.reasoning_end') AS INTEGER) \
              - CAST(json_extract(payload,'$.reasoning_len') AS INTEGER) < ?6)) \
         ORDER BY sequence",
    )?;
    let rows = statement.query_map(
        params![
            owner,
            attempt.to_string(),
            i64::try_from(text_from).map_err(|_| Error::Conflict)?,
            i64::try_from(text_to).map_err(|_| Error::Conflict)?,
            i64::try_from(reasoning_from).map_err(|_| Error::Conflict)?,
            i64::try_from(reasoning_to).map_err(|_| Error::Conflict)?,
        ],
        |row| row.get::<_, String>(0),
    )?;
    let mut index = Vec::new();
    for row in rows {
        let entry: DeltaIndex = record::decode(&row?)?;
        if entry.attempt == attempt {
            index.push(entry);
        }
    }
    Ok(Some(SoughtIndex {
        rows: index,
        text_total: latest.text_end,
        reasoning_total: latest.reasoning_end,
        sequence: Some(latest.sequence),
    }))
}

fn page_lengths(
    db: &Connection,
    owner: &str,
    attempt: TransportAttemptId,
) -> StoreResult<Vec<DeltaIndex>> {
    let mut statement = db.prepare(
        "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_delta_index' \
         AND version=1 ORDER BY sequence",
    )?;
    let rows = statement.query_map([owner], |row| row.get::<_, String>(0))?;
    let mut by_sequence = std::collections::BTreeMap::<u64, DeltaIndex>::new();
    for row in rows {
        let entry: DeltaIndex = record::decode(&row?)?;
        if entry.attempt != attempt {
            continue;
        }
        match by_sequence.get(&entry.sequence) {
            Some(current) if current.byte_lengths && !entry.byte_lengths => {}
            _ => {
                by_sequence.insert(entry.sequence, entry);
            }
        }
    }
    drop(statement);
    let indexed: Vec<_> = by_sequence.into_values().collect();
    let through = indexed.last().map(|entry| entry.sequence).unwrap_or(0);
    let trusted = !indexed.is_empty()
        && indexed.iter().all(|entry| entry.byte_lengths)
        && read_epoch_covers(db, owner, attempt, through)?;
    if trusted {
        return Ok(indexed);
    }
    payload_byte_lengths(db, owner, attempt)
}

fn read_epoch_covers(
    db: &Connection,
    owner: &str,
    attempt: TransportAttemptId,
    through_sequence: u64,
) -> StoreResult<bool> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_delta_epoch' \
             AND version=1 ORDER BY sequence DESC LIMIT 1",
            [owner],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(false) };
    let epoch: DeltaEpoch = record::decode(&raw)?;
    Ok(epoch.attempt == attempt
        && epoch.unit == BYTE_LENGTHS
        && epoch.through_sequence >= through_sequence)
}

fn payload_byte_lengths(
    db: &Connection,
    owner: &str,
    attempt: TransportAttemptId,
) -> StoreResult<Vec<DeltaIndex>> {
    let mut statement = db.prepare(
        "SELECT json_extract(payload,'$.sequence'), \
         COALESCE(length(CAST(json_extract(payload,'$.text') AS BLOB)), 0), \
         COALESCE(length(CAST(json_extract(payload,'$.reasoning') AS BLOB)), 0) \
         FROM events WHERE owner=?1 AND kind='agent.visible_delta' AND version=1 \
         AND json_extract(payload,'$.attempt')=?2 ORDER BY sequence",
    )?;
    let rows = statement.query_map(params![owner, attempt.to_string()], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
    })?;
    let mut index = Vec::new();
    for row in rows {
        let (sequence, text_len, reasoning_len) = row?;
        index.push(DeltaIndex {
            attempt,
            sequence: u64::try_from(sequence).map_err(|_| Error::Conflict)?,
            text_len: u64::try_from(text_len).map_err(|_| Error::Conflict)?,
            reasoning_len: u64::try_from(reasoning_len).map_err(|_| Error::Conflict)?,
            byte_lengths: true,
            text_end: 0,
            reasoning_end: 0,
        });
    }
    index.sort_by_key(|entry| entry.sequence);
    Ok(index)
}

/// The requested byte span is covered by abutting rows. A hole, an overlap, or a
/// row whose stored length does not reach its end fails the proof.
fn window_covers(rows: &[WindowRow], from: u64, to: u64, total: u64, text: bool) -> bool {
    let end = to.min(total);
    if from >= end {
        return true;
    }
    let mut spans = Vec::new();
    for row in rows {
        let (len, stop) = if text { (row.1, row.2) } else { (row.3, row.4) };
        if len <= 0 {
            continue;
        }
        let Ok(len) = u64::try_from(len) else { return false };
        let Ok(stop) = u64::try_from(stop) else { return false };
        if stop < len {
            return false;
        }
        spans.push((stop - len, stop));
    }
    spans.sort_unstable();
    let mut cursor = from;
    for (start, stop) in spans {
        if start > cursor {
            return false;
        }
        if stop > cursor {
            cursor = stop;
        }
        if cursor >= end {
            return true;
        }
    }
    cursor >= end
}

fn delta_copy_is_complete(locator: &DeltaLocator<'_>) -> StoreResult<bool> {
    let progress: Option<(i64, i64, i64, i64)> = locator
        .db
        .query_row(
            "SELECT text_copied, reasoning_copied, text_len, reasoning_len \
             FROM agent_visible_repair WHERE owner=?1 AND attempt=?2 AND sequence=?3",
            params![
                locator.owner,
                locator.attempt.to_string(),
                i64::try_from(locator.sequence).map_err(|_| Error::Conflict)?,
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    Ok(progress.is_none_or(|(text_copied, reasoning_copied, text_len, reasoning_len)| {
        text_copied == text_len && reasoning_copied == reasoning_len
    }))
}
