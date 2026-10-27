//! The coverage watermark of one attempt, and the constant-cost append it allows.
//!
//! A full proof that the locatable index has no hole walks every row of the attempt. Running
//! it on every delta made one long response quadratic: a real 8192-token reasoning stream is
//! about 8000 deltas, and its last appends each re-checked thousands of rows (plus two JSON
//! scans of the events), so Core needed minutes to store a stream the Provider had finished in
//! seconds. The watermark already is that proof for every row it covers. An append that only
//! extends the watermark's tail therefore needs to check the tail, not the history.
use super::{Delta, epoch_covers, index_ends};
use aura_domain_common::TransportAttemptId;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{OptionalExtension, Transaction, params};

/// A watermark that still describes the index exactly, so the next delta can extend it.
pub(super) struct TrustedTail {
    max_sequence: i64,
    index_count: i64,
    pub(super) text_end: u64,
    pub(super) reasoning_end: u64,
}

/// The watermark, when the delta at `next_sequence` continues the tail it proves.
///
/// Every check reads one key or one end of the attempt's index, except the row count, which
/// is taken only when the index has doubled since the last one. `None` sends the append down
/// the full path, which re-proves the whole index and repairs it:
///
/// * the newest visible delta of this step is this attempt's and precedes `next_sequence`.
///   Deltas of one attempt are stored in sequence order, so no stored delta of the attempt
///   reaches `next_sequence` and this one cannot be a duplicate;
/// * the watermark ends at that delta, and the epoch covers it;
/// * the index's first and last rows are the watermark's, its last row carries the
///   watermark's ends, and, at each doubling, no row was removed since the watermark was
///   written;
/// * the event index ends where the watermark does.
pub(super) fn trusted(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    previous: Option<&Delta>,
    next_sequence: u64,
) -> StoreResult<Option<TrustedTail>> {
    let Some(previous) = previous else { return Ok(None) };
    if previous.attempt != attempt || previous.sequence >= next_sequence {
        return Ok(None);
    }
    let attempt_text = attempt.to_string();
    let watermark: Option<(i64, i64, i64, i64, i64)> = tx
        .query_row(
            "SELECT first_sequence, max_sequence, index_count, text_end, reasoning_end \
             FROM agent_visible_coverage WHERE owner=?1 AND attempt=?2",
            params![owner, attempt_text],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .optional()?;
    let Some((first_sequence, max_sequence, index_count, text_end, reasoning_end)) = watermark
    else {
        return Ok(None);
    };
    let previous_sequence = i64::try_from(previous.sequence).map_err(|_| Error::Conflict)?;
    if max_sequence != previous_sequence || !epoch_covers(tx, owner, attempt, previous.sequence)? {
        return Ok(None);
    }
    let first: Option<i64> = tx
        .query_row(
            "SELECT sequence FROM agent_visible_index WHERE owner=?1 AND attempt=?2 \
             ORDER BY sequence ASC LIMIT 1",
            params![owner, attempt_text],
            |row| row.get(0),
        )
        .optional()?;
    let last: Option<(i64, i64, i64)> = tx
        .query_row(
            "SELECT sequence, text_end, reasoning_end FROM agent_visible_index \
             WHERE owner=?1 AND attempt=?2 ORDER BY sequence DESC LIMIT 1",
            params![owner, attempt_text],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    if first != Some(first_sequence) || last != Some((max_sequence, text_end, reasoning_end)) {
        return Ok(None);
    }
    // A removed middle row leaves the first and last rows in place; only the count shows it.
    // Counting walks the attempt's rows, so it is done when the index has doubled since the
    // last count: a stream's appends stay linear in all, and a removed row is found at the next
    // doubling and repaired by the full path. Until then a page never drops its text: a page
    // whose rows do not cover it reads the event index instead (`window_covers`).
    if index_count > 0 && index_count.count_ones() == 1 {
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM agent_visible_index WHERE owner=?1 AND attempt=?2",
            params![owner, attempt_text],
            |row| row.get(0),
        )?;
        if count != index_count {
            return Ok(None);
        }
    }
    let text_end = u64::try_from(text_end).map_err(|_| Error::Conflict)?;
    let reasoning_end = u64::try_from(reasoning_end).map_err(|_| Error::Conflict)?;
    let (event_text, event_reasoning, known) = index_ends(tx, owner, attempt)?;
    if !known || event_text != text_end || event_reasoning != reasoning_end {
        return Ok(None);
    }
    Ok(Some(TrustedTail { max_sequence, index_count, text_end, reasoning_end }))
}

/// Moves the watermark to the row just appended after `tail`.
///
/// The new row starts where the watermark ended, so the index is still one unbroken run and
/// the watermark stays a proof. The update is conditional on the watermark the check read.
pub(super) fn extend(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
    tail: &TrustedTail,
    sequence: u64,
    text_end: u64,
    reasoning_end: u64,
) -> StoreResult<()> {
    let changed = tx.execute(
        "UPDATE agent_visible_coverage SET max_sequence=?3, index_count=?4, text_end=?5, \
         reasoning_end=?6 WHERE owner=?1 AND attempt=?2 AND max_sequence=?7 AND index_count=?8",
        params![
            owner,
            attempt.to_string(),
            i64::try_from(sequence).map_err(|_| Error::Conflict)?,
            tail.index_count.checked_add(1).ok_or(Error::Conflict)?,
            i64::try_from(text_end).map_err(|_| Error::Conflict)?,
            i64::try_from(reasoning_end).map_err(|_| Error::Conflict)?,
            tail.max_sequence,
            tail.index_count,
        ],
    )?;
    if changed == 1 { Ok(()) } else { Err(Error::Conflict) }
}

/// Records the first and last row of one attempt after those rows are present.
///
/// A length that has no cumulative end is not a watermark: the page then uses the
/// event index. Writing the watermark is what later reads treat as proof that this
/// generation covers the attempt, so it is not written over a hole.
pub(super) fn mark_coverage(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
) -> StoreResult<()> {
    let hole = index_has_byte_gap(tx, owner, attempt)?;
    if hole {
        tx.execute(
            "DELETE FROM agent_visible_coverage WHERE owner=?1 AND attempt=?2",
            params![owner, attempt.to_string()],
        )?;
        return Ok(());
    }
    let count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM agent_visible_index WHERE owner=?1 AND attempt=?2",
        params![owner, attempt.to_string()],
        |row| row.get(0),
    )?;
    if count == 0 {
        return Ok(());
    }
    let (first, max, text_end, reasoning_end): (i64, i64, i64, i64) = tx.query_row(
        "SELECT MIN(sequence), MAX(sequence), \
         (SELECT text_end FROM agent_visible_index WHERE owner=?1 AND attempt=?2 \
          ORDER BY sequence DESC LIMIT 1), \
         (SELECT reasoning_end FROM agent_visible_index WHERE owner=?1 AND attempt=?2 \
          ORDER BY sequence DESC LIMIT 1) \
         FROM agent_visible_index WHERE owner=?1 AND attempt=?2",
        params![owner, attempt.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    tx.execute(
        "INSERT INTO agent_visible_coverage(owner,attempt,first_sequence,max_sequence,index_count,text_end,reasoning_end) \
         VALUES(?1,?2,?3,?4,?5,?6,?7) \
         ON CONFLICT(owner,attempt) DO UPDATE SET \
         first_sequence=excluded.first_sequence, max_sequence=excluded.max_sequence, \
         index_count=excluded.index_count, text_end=excluded.text_end, \
         reasoning_end=excluded.reasoning_end",
        params![owner, attempt.to_string(), first, max, count, text_end, reasoning_end],
    )?;
    Ok(())
}

/// A missing middle row, an overlap, or a length that does not meet its end.
pub(super) fn index_has_byte_gap(
    tx: &Transaction<'_>,
    owner: &str,
    attempt: TransportAttemptId,
) -> StoreResult<bool> {
    tx.query_row(
        "SELECT EXISTS( \
            SELECT 1 FROM agent_visible_index AS row \
            WHERE row.owner=?1 AND row.attempt=?2 AND row.text_len > 0 AND ( \
              row.text_end <> row.text_start + row.text_len \
              OR row.text_end = 0 \
              OR (row.text_start <> 0 AND NOT EXISTS ( \
                SELECT 1 FROM agent_visible_index AS prev \
                WHERE prev.owner=row.owner AND prev.attempt=row.attempt \
                  AND prev.text_len > 0 AND prev.text_end = row.text_start)))) \
         OR EXISTS( \
            SELECT 1 FROM agent_visible_index AS row \
            WHERE row.owner=?1 AND row.attempt=?2 AND row.reasoning_len > 0 AND ( \
              row.reasoning_end <> row.reasoning_start + row.reasoning_len \
              OR row.reasoning_end = 0 \
              OR (row.reasoning_start <> 0 AND NOT EXISTS ( \
                SELECT 1 FROM agent_visible_index AS prev \
                WHERE prev.owner=row.owner AND prev.attempt=row.attempt \
                  AND prev.reasoning_len > 0 AND prev.reasoning_end = row.reasoning_start))))",
        params![owner, attempt.to_string()],
        |row| row.get(0),
    )
    .map_err(Error::from)
}
