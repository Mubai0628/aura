//! The background copy of visible streams stored before the locatable tables existed.
//!
//! Finding the next stream only reads, so a pass with nothing to copy never takes the write
//! transaction. The caller keeps what was found in a [`RepairScan`] between passes: one sweep
//! over the steps without coverage, in bounded slices, and after it only the events appended
//! since the last pass. Finding used to ask every step without coverage whether it held a
//! delta, reading all of its events, on every pass and inside the write transaction.
//!
//! A copy reads the step's deltas once, in the order they were stored, and takes each piece by
//! its event's key. Each piece used to look its delta up again by scanning the step's events,
//! which made one pass quadratic in the step's deltas.
use super::*;
use aura_persistence_core::DatabaseConnection;
use std::{collections::BTreeSet, ops::Bound, time::Instant};

/// Steps the sweep lists at a time.
const SWEEP_BATCH: i64 = 64;
/// New events one call reads at most; the rest are read by the next call.
const NEW_EVENTS_PER_CALL: i64 = 50_000;

/// What the background repair has found, kept by its owner between passes.
///
/// A new scan sweeps the steps without coverage once; events appended after the sweep began
/// are read as they come. An owner leaves the set when it is finished, gone, or has nothing a
/// copy can take; a later delta of its own brings it back.
#[derive(Debug, Default)]
pub struct RepairScan {
    /// Events up to this sequence have been looked at.
    through: Option<i64>,
    sweep: Sweep,
    outstanding: BTreeSet<String>,
    /// The owner the last copy picked, so a failed pass can be moved past.
    current: Option<String>,
}

#[derive(Debug, Default)]
enum Sweep {
    #[default]
    NotStarted,
    /// Steps up to this id have been asked.
    After(String),
    Done,
}

impl RepairScan {
    /// Whether a copy pass has an owner to take.
    #[must_use]
    pub fn has_outstanding(&self) -> bool {
        !self.outstanding.is_empty()
    }

    /// Whether the sweep over steps without coverage is still under way.
    #[must_use]
    pub fn sweeping(&self) -> bool {
        !matches!(self.sweep, Sweep::Done)
    }

    /// The owner the last copy pass picked.
    #[must_use]
    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// The first outstanding owner after `last`, wrapping around: the round-robin order the
    /// cursor row keeps across passes and restarts.
    fn next_after(&self, last: Option<&str>) -> Option<String> {
        last.and_then(|last| {
            self.outstanding.range::<str, _>((Bound::Excluded(last), Bound::Unbounded)).next()
        })
        .or_else(|| self.outstanding.first())
        .cloned()
    }
}

/// Brings `scan` up to date. Reads only, so it runs on a read connection.
///
/// The first call notes where the event log ends and starts the sweep; later calls read the
/// events appended since, at most [`NEW_EVENTS_PER_CALL`]. The sweep then goes on until `until`,
/// one step at least; without a deadline it finishes.
pub fn discover_outstanding(
    db: &Connection,
    scan: &mut RepairScan,
    until: Option<Instant>,
) -> StoreResult<()> {
    // AUTOINCREMENT keeps the highest sequence ever used, so in one database the mark never
    // moves back. A lower mark is another database, and the scan starts over.
    let newest: i64 = db.query_row(
        "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='events'), 0)",
        [],
        |row| row.get(0),
    )?;
    match scan.through {
        Some(through) if through <= newest => {
            if through < newest {
                let to = newest.min(through.saturating_add(NEW_EVENTS_PER_CALL));
                for owner in owners_with_new_stream_events(db, through, to)? {
                    if is_outstanding(db, &owner)? {
                        scan.outstanding.insert(owner);
                    } else {
                        scan.outstanding.remove(&owner);
                    }
                }
                scan.through = Some(to);
            }
        }
        _ => {
            *scan = RepairScan {
                through: Some(newest),
                sweep: Sweep::After(String::new()),
                outstanding: coverage_without_first_row(db)?,
                current: None,
            };
        }
    }
    sweep(db, scan, until)
}

fn sweep(db: &Connection, scan: &mut RepairScan, until: Option<Instant>) -> StoreResult<()> {
    while let Sweep::After(after) = &scan.sweep {
        let steps = steps_without_coverage(db, after)?;
        if steps.is_empty() {
            scan.sweep = Sweep::Done;
            break;
        }
        for step in steps {
            if has_visible_events(db, &step)? {
                scan.outstanding.insert(step.clone());
            }
            scan.sweep = Sweep::After(step);
            if until.is_some_and(|until| Instant::now() >= until) {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn steps_without_coverage(db: &Connection, after: &str) -> StoreResult<Vec<String>> {
    let mut statement = db.prepare(
        "SELECT id FROM model_steps WHERE id > ?1 \
         AND id NOT IN (SELECT owner FROM agent_visible_coverage) ORDER BY id LIMIT ?2",
    )?;
    let steps = statement
        .query_map(params![after, SWEEP_BATCH], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(steps)
}

/// A step's events are read in their own order until the first visible one.
fn has_visible_events(db: &Connection, owner: &str) -> StoreResult<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM events WHERE owner=?1 AND version=1 \
         AND kind IN ('agent.visible_delta','agent.visible_delta_index'))",
        [owner],
        |row| row.get(0),
    )
    .map_err(Error::from)
}

fn coverage_without_first_row(db: &Connection) -> StoreResult<BTreeSet<String>> {
    let mut statement = db.prepare(
        "SELECT owner FROM agent_visible_coverage c WHERE NOT EXISTS ( \
         SELECT 1 FROM agent_visible_index i \
         WHERE i.owner=c.owner AND i.attempt=c.attempt AND i.sequence=c.first_sequence)",
    )?;
    let owners =
        statement.query_map([], |row| row.get(0))?.collect::<Result<BTreeSet<String>, _>>()?;
    Ok(owners)
}

/// Owners of stream events in `(after, through]`, read by the events' own key.
///
/// Coverage is dropped only where a hole is found, and every such write also appends the
/// stream's epoch event, so the epoch kind is read too.
fn owners_with_new_stream_events(
    db: &Connection,
    after: i64,
    through: i64,
) -> StoreResult<Vec<String>> {
    let mut statement = db.prepare(
        "SELECT DISTINCT owner FROM events WHERE sequence > ?1 AND sequence <= ?2 \
         AND version=1 AND kind IN \
         ('agent.visible_delta','agent.visible_delta_index','agent.visible_delta_epoch')",
    )?;
    let owners = statement
        .query_map(params![after, through], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(owners)
}

/// What the sweep and the event reads both mean by an unfinished stream: a step with visible
/// events and no coverage, or coverage whose first row is gone from the locatable index.
fn is_outstanding(db: &Connection, owner: &str) -> StoreResult<bool> {
    db.query_row(
        "SELECT (EXISTS(SELECT 1 FROM model_steps WHERE id=?1) \
           AND NOT EXISTS(SELECT 1 FROM agent_visible_coverage WHERE owner=?1) \
           AND EXISTS(SELECT 1 FROM events WHERE owner=?1 AND version=1 \
             AND kind IN ('agent.visible_delta','agent.visible_delta_index'))) \
         OR EXISTS(SELECT 1 FROM agent_visible_coverage c WHERE c.owner=?1 AND NOT EXISTS ( \
           SELECT 1 FROM agent_visible_index i \
           WHERE i.owner=c.owner AND i.attempt=c.attempt AND i.sequence=c.first_sequence))",
        [owner],
        |row| row.get(0),
    )
    .map_err(Error::from)
}

/// One copy pass in a transaction of its own: the next outstanding owner after the round-robin
/// cursor, at most `byte_budget` bytes.
///
/// `scan` learns the result only after the commit. The owner it picked is kept even when the
/// pass fails, so the caller can move the cursor past it.
pub fn repair_pass(
    database: &mut DatabaseConnection,
    scan: &mut RepairScan,
    at: UtcTimestamp,
    byte_budget: u64,
) -> StoreResult<u32> {
    if byte_budget == 0 || !scan.has_outstanding() {
        return Ok(0);
    }
    let mut picked = None;
    let result = database.unit_of_work(|tx| {
        let last = repair_turn(tx)?;
        let Some(owner) = scan.next_after(last.as_deref()) else { return Ok(None) };
        picked = Some(owner.clone());
        repair_owner(tx, &owner, at, byte_budget).map(|pass| Some((owner, pass)))
    });
    scan.current = picked;
    let Some((owner, pass)) = result? else { return Ok(0) };
    if !(pass.outstanding && pass.copied > 0) {
        scan.outstanding.remove(&owner);
    }
    Ok(pass.progress())
}

/// Copies at most `byte_budget` UTF-8 bytes of one unfinished visible stream, in the caller's
/// transaction, after a full sweep. For a caller that keeps no [`RepairScan`].
///
/// The first owner is not exempt. Each call returns only a slice of the source
/// JSON to the caller; SQLite still evaluates that value once inside the
/// statement. Length and “anything left” checks return integers. Coverage is
/// written only when every sequence of the attempt is fully copied.
pub fn repair_outstanding(
    tx: &Transaction<'_>,
    at: UtcTimestamp,
    byte_budget: u64,
) -> StoreResult<u32> {
    if byte_budget == 0 {
        return Ok(0);
    }
    let mut scan = RepairScan::default();
    discover_outstanding(tx, &mut scan, None)?;
    let last = repair_turn(tx)?;
    let Some(owner) = scan.next_after(last.as_deref()) else { return Ok(0) };
    Ok(repair_owner(tx, &owner, at, byte_budget)?.progress())
}

/// Moves the round-robin past `owner`, whose pass failed, without copying it.
pub fn advance_repair_cursor(tx: &Transaction<'_>, owner: &str) -> StoreResult<()> {
    remember_turn(tx, owner)
}

/// The owner the round-robin served last; the next pass starts after it.
fn remember_turn(tx: &Transaction<'_>, owner: &str) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO agent_visible_repair_turn(id, owner) VALUES(1, ?1) \
         ON CONFLICT(id) DO UPDATE SET owner=excluded.owner",
        [owner],
    )?;
    Ok(())
}

fn repair_turn(db: &Connection) -> StoreResult<Option<String>> {
    db.query_row("SELECT owner FROM agent_visible_repair_turn WHERE id=1", [], |row| row.get(0))
        .optional()
        .map_err(Error::from)
}

struct OwnerPass {
    copied: u64,
    /// Still unfinished after the pass.
    outstanding: bool,
    /// Unfinished before the pass and finished by it.
    settled: bool,
}

impl OwnerPass {
    fn progress(&self) -> u32 {
        u32::from(self.copied > 0 || self.settled)
    }
}

fn repair_owner(
    tx: &Transaction<'_>,
    owner: &str,
    at: UtcTimestamp,
    byte_budget: u64,
) -> StoreResult<OwnerPass> {
    // Found on a read connection, or in an earlier pass: asked again inside the transaction.
    if !is_outstanding(tx, owner)? {
        return Ok(OwnerPass { copied: 0, outstanding: false, settled: false });
    }
    remember_turn(tx, owner)?;
    let copied = copy_owner_within_budget(tx, owner, at, byte_budget)?;
    let outstanding = is_outstanding(tx, owner)?;
    Ok(OwnerPass { copied, outstanding, settled: !outstanding })
}

/// Copies the latest attempt's deltas in the order they were stored. Once nothing of the
/// attempt is left to copy, its index and coverage are published, also when this pass copied
/// nothing: a stream whose deltas the append path already stored needs only the index.
fn copy_owner_within_budget(
    tx: &Transaction<'_>,
    owner: &str,
    at: UtcTimestamp,
    byte_budget: u64,
) -> StoreResult<u64> {
    let step = owner.parse().map_err(|_| Error::Conflict)?;
    let Some(attempt) = latest_attempt(tx, step)? else {
        return Ok(0);
    };
    let (pending, listed_all) = pending_deltas(tx, owner, attempt, byte_budget)?;
    let mut remaining = byte_budget;
    let mut copied = 0_u64;
    let mut finished = listed_all;
    for mut delta in pending {
        for text in [true, false] {
            let (done, len) = if text {
                (delta.text_copied, delta.text_len)
            } else {
                (delta.reasoning_copied, delta.reasoning_len)
            };
            if done >= len || remaining == 0 {
                continue;
            }
            let piece = read_piece(tx, delta.event, text, done, remaining)?;
            if piece.is_empty() {
                break;
            }
            let piece = std::str::from_utf8(&piece).map_err(|_| Error::Conflict)?;
            write_delta_piece(tx, owner, attempt, delta.sequence, text, piece, !delta.has_row)?;
            delta.has_row = true;
            let bytes = i64::try_from(piece.len()).map_err(|_| Error::Conflict)?;
            if text {
                delta.text_copied = delta.text_copied.saturating_add(bytes);
            } else {
                delta.reasoning_copied = delta.reasoning_copied.saturating_add(bytes);
            }
            save_repair_offsets(
                tx,
                owner,
                attempt,
                RepairProgress {
                    sequence: delta.sequence,
                    text_copied: delta.text_copied,
                    reasoning_copied: delta.reasoning_copied,
                    text_len: delta.text_len,
                    reasoning_len: delta.reasoning_len,
                },
            )?;
            copied = copied.saturating_add(piece.len() as u64);
            remaining = remaining.saturating_sub(piece.len() as u64);
        }
        if delta.text_copied < delta.text_len || delta.reasoning_copied < delta.reasoning_len {
            finished = false;
            break;
        }
    }
    if finished {
        repair_byte_index(tx, step, at)?;
    }
    Ok(copied)
}

struct PendingDelta {
    /// The event's own key, by which each piece is read.
    event: i64,
    sequence: u64,
    text_len: i64,
    reasoning_len: i64,
    text_copied: i64,
    reasoning_copied: i64,
    /// The locatable row exists, so a piece is appended to it rather than inserted.
    has_row: bool,
}

/// The deltas of `attempt` that still have bytes to copy, in the order they were stored, until
/// their bytes cover the budget. One read of the step's events; `true` when no delta with
/// bytes left was left out.
///
/// A delta is copied as far as its progress row says, or whole when the append path wrote its
/// row and there is no progress row. A progress row whose locatable row is gone starts over.
fn pending_deltas(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    byte_budget: u64,
) -> StoreResult<(Vec<PendingDelta>, bool)> {
    let mut statement = tx.prepare(
        "SELECT e.sequence, CAST(json_extract(e.payload,'$.sequence') AS INTEGER), \
         COALESCE(length(CAST(json_extract(e.payload,'$.text') AS BLOB)), 0), \
         COALESCE(length(CAST(json_extract(e.payload,'$.reasoning') AS BLOB)), 0), \
         r.text_copied, r.reasoning_copied, d.sequence IS NOT NULL \
         FROM events e \
         LEFT JOIN agent_visible_repair r ON r.owner=?1 AND r.attempt=?2 \
           AND r.sequence=CAST(json_extract(e.payload,'$.sequence') AS INTEGER) \
         LEFT JOIN agent_visible_delta d ON d.owner=?1 AND d.attempt=?2 \
           AND d.sequence=CAST(json_extract(e.payload,'$.sequence') AS INTEGER) \
         WHERE e.owner=?1 AND e.kind='agent.visible_delta' AND e.version=1 \
         AND json_extract(e.payload,'$.attempt')=?2 ORDER BY e.sequence",
    )?;
    let mut rows = statement.query(params![owner, attempt.to_string()])?;
    let mut pending = Vec::new();
    let mut bytes_left = 0_u64;
    while let Some(row) = rows.next()? {
        let text_len: i64 = row.get(2)?;
        let reasoning_len: i64 = row.get(3)?;
        let progress: (Option<i64>, Option<i64>) = (row.get(4)?, row.get(5)?);
        let has_row: bool = row.get(6)?;
        let (text_copied, reasoning_copied) = match (has_row, progress) {
            (true, (Some(text), Some(reasoning))) => (text, reasoning),
            (true, _) => (text_len, reasoning_len),
            (false, _) => (0, 0),
        };
        let left = text_len.saturating_sub(text_copied).max(0)
            + reasoning_len.saturating_sub(reasoning_copied).max(0);
        if left == 0 {
            continue;
        }
        if bytes_left >= byte_budget {
            return Ok((pending, false));
        }
        bytes_left = bytes_left.saturating_add(u64::try_from(left).map_err(|_| Error::Conflict)?);
        pending.push(PendingDelta {
            event: row.get(0)?,
            sequence: u64::try_from(row.get::<_, i64>(1)?).map_err(|_| Error::Conflict)?,
            text_len,
            reasoning_len,
            text_copied,
            reasoning_copied,
            has_row,
        });
    }
    Ok((pending, true))
}

/// At most `budget` bytes of one delta's text or reasoning from `copied` on, by the event's key.
fn read_piece(
    tx: &Transaction<'_>,
    event: i64,
    text: bool,
    copied: i64,
    budget: u64,
) -> StoreResult<Vec<u8>> {
    let sql = if text {
        "SELECT substr(CAST(json_extract(payload, '$.text') AS BLOB), ?2, ?3) \
         FROM events WHERE sequence=?1"
    } else {
        "SELECT substr(CAST(json_extract(payload, '$.reasoning') AS BLOB), ?2, ?3) \
         FROM events WHERE sequence=?1"
    };
    let start = copied.checked_add(1).ok_or(Error::Conflict)?;
    let take = i64::try_from(budget).unwrap_or(i64::MAX);
    let bytes: Option<Vec<u8>> =
        tx.query_row(sql, params![event, start, take], |row| row.get(0)).optional()?;
    Ok(snap_utf8_end(&bytes.unwrap_or_default()).to_vec())
}

fn snap_utf8_end(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    &bytes[..end]
}

struct RepairProgress {
    sequence: u64,
    text_copied: i64,
    reasoning_copied: i64,
    text_len: i64,
    reasoning_len: i64,
}

fn save_repair_offsets(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    progress: RepairProgress,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO agent_visible_repair(owner,attempt,sequence,text_copied,reasoning_copied,text_len,reasoning_len) \
         VALUES(?1,?2,?3,?4,?5,?6,?7) \
         ON CONFLICT(owner,attempt,sequence) DO UPDATE SET \
         text_copied=excluded.text_copied, reasoning_copied=excluded.reasoning_copied, \
         text_len=excluded.text_len, reasoning_len=excluded.reasoning_len",
        params![
            owner,
            attempt.to_string(),
            i64::try_from(progress.sequence).map_err(|_| Error::Conflict)?,
            progress.text_copied,
            progress.reasoning_copied,
            progress.text_len,
            progress.reasoning_len,
        ],
    )?;
    Ok(())
}

fn write_delta_piece(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    sequence: u64,
    text: bool,
    piece: &str,
    insert: bool,
) -> StoreResult<()> {
    let sequence = i64::try_from(sequence).map_err(|_| Error::Conflict)?;
    let attempt = attempt.to_string();
    if insert {
        let (body, reasoning) = if text { (piece, "") } else { ("", piece) };
        tx.execute(
            "INSERT INTO agent_visible_delta(owner,attempt,sequence,text,reasoning) VALUES(?1,?2,?3,?4,?5)",
            params![owner, attempt, sequence, body, reasoning],
        )?;
        return Ok(());
    }
    let sql = if text {
        "UPDATE agent_visible_delta SET text = text || ?4 WHERE owner=?1 AND attempt=?2 AND sequence=?3"
    } else {
        "UPDATE agent_visible_delta SET reasoning = reasoning || ?4 WHERE owner=?1 AND attempt=?2 AND sequence=?3"
    };
    tx.execute(sql, params![owner, attempt, sequence, piece])?;
    Ok(())
}
