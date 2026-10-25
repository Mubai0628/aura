//! Normalized visible-stream append; auxiliary requests never enter the conversation.
use super::*;
use aura_model_domain::{ModelStreamEvent, ModelStreamEventEnvelope};
pub fn append(tx: &Transaction<'_>, event: &ModelStreamEventEnvelope) -> StoreResult<()> {
    if crate::model::memory_maintenance::binding(tx, event.model_step_id)?.is_some() {
        return crate::model::memory_maintenance::record_usage(tx, event);
    }
    if crate::context::compaction::model_owner(tx, event.model_step_id)?.is_some() {
        return Ok(());
    }
    let (text, reasoning) = match &event.event {
        ModelStreamEvent::AssistantTextDelta { text, .. }
        | ModelStreamEvent::RefusalDelta { text } => (text.clone(), String::new()),
        ModelStreamEvent::ReasoningDelta { text, .. }
        | ModelStreamEvent::ReasoningSummaryDelta { text, .. } => (String::new(), text.clone()),
        _ => return Ok(()),
    };
    let owner = event.model_step_id.to_string();
    let value =
        Delta { attempt: event.transport_attempt_id, sequence: event.sequence, text, reasoning };
    let step = turns::step(tx, event.model_step_id)?;
    // A terminal commit may win the receiver race. It already contains all
    // final content, so late deltas can be discarded without changing history.
    if !matches!(step.state, ModelStepState::Running { .. }) {
        return Ok(());
    }
    let snapshot = crate::model::step_snapshot(tx, event.model_step_id)?;
    let belongs: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM transport_attempts WHERE id=?1 AND snapshot_id=?2)",
        [event.transport_attempt_id.to_string(), snapshot.id.to_string()],
        |r| r.get(0),
    )?;
    if !belongs {
        return Err(Error::Conflict);
    }
    // Retry attempts are ordered by their durable insertion, never by a late
    // receiver event or wall-clock timestamp. An older attempt cannot reclaim
    // the visible stream after a new attempt has begun.
    if latest_attempt(tx, event.model_step_id)? != Some(event.transport_attempt_id) {
        return Ok(());
    }
    let previous: Option<Delta> = facts::latest(tx, &owner, "agent.visible_delta")?;
    // The usual delta continues a tail the watermark already proves; only the tail is checked.
    let tail = tail::trusted(tx, &owner, value.attempt, previous.as_ref(), value.sequence)?;
    if tail.is_none() {
        let duplicate: Option<String> = tx.query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_delta' AND json_extract(payload,'$.attempt')=?2 AND json_extract(payload,'$.sequence')=?3",
            params![owner, value.attempt.to_string(), i64::try_from(value.sequence).map_err(|_| Error::Conflict)?], |r| r.get(0)).optional()?;
        if let Some(raw) = duplicate {
            return if record::decode::<Delta>(&raw)? == value {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        if previous
            .is_some_and(|old| old.attempt == value.attempt && old.sequence >= value.sequence)
        {
            return Err(Error::Conflict);
        }
    }
    let text_len = value.text.len() as u64;
    let reasoning_len = value.reasoning.len() as u64;
    let attempt = value.attempt;
    let sequence = value.sequence;
    // A stream stored before the index existed has deltas with no index row, and a page that
    // trusts a partial index silently drops the prefix it does not cover. Rather than
    // weakening the page, the index is brought up to date here — inside the same transaction
    // as the append, so a page either sees the complete index or the empty one — before the
    // new row is added. The new row's cumulative end is computed after that backfill, so it
    // includes the prefix. Rows that already have an index entry, or belong to an older
    // attempt whose deltas this attempt's pages never read, are left alone.
    let (text_base, reasoning_base, ends_known) = match &tail {
        Some(tail) => (tail.text_end, tail.reasoning_end, true),
        None => {
            backfill_index(tx, &owner, attempt, sequence, event.received_at)?;
            index_ends(tx, &owner, attempt)?
        }
    };
    let text_end = if ends_known { text_base.saturating_add(text_len) } else { 0 };
    let reasoning_end = if ends_known { reasoning_base.saturating_add(reasoning_len) } else { 0 };
    facts::append(tx, EventId::new(), &owner, "agent.visible_delta", &value, event.received_at)?;
    remember_delta(tx, &owner, attempt, sequence, &value.text, &value.reasoning)?;
    // The lengths of what was just appended, without the text. A page later reads these
    // small rows to find which deltas cover its window and fetches only those, so a page
    // never parses the whole stream to answer for one range of it. Lengths are UTF-8 bytes,
    // the same unit the page offset uses.
    facts::append(
        tx,
        EventId::new(),
        &owner,
        "agent.visible_delta_index",
        &DeltaIndex {
            attempt,
            sequence,
            text_len,
            reasoning_len,
            byte_lengths: true,
            text_end,
            reasoning_end,
        },
        event.received_at,
    )?;
    sync_visible_index(
        tx,
        &owner,
        &DeltaIndex {
            attempt,
            sequence,
            text_len,
            reasoning_len,
            byte_lengths: true,
            text_end,
            reasoning_end,
        },
    )?;
    match &tail {
        Some(tail) => tail::extend(tx, &owner, attempt, tail, sequence, text_end, reasoning_end)?,
        None => mark_coverage(tx, &owner, attempt)?,
    }
    record_epoch(tx, &owner, attempt, sequence, event.received_at)
}
