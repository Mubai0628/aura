//! The conversation by round, for the model's history tools and for the index its context keeps
//! after a compaction. A round is one task: it begins with the conversation's first turn or a turn
//! after a completed one, and takes in every turn until one completes, so a question answered, a
//! message added, or a turn stopped, failed or interrupted and then continued stays in its round.
//! Rounds and their entries are worked out from the turns and events whenever they are asked for;
//! a round that is over never changes, so its ids stay. A branch's carried turns come first.
mod entries;
mod rows;
mod search;
#[cfg(test)]
mod tests;
mod turns;

use aura_context_domain::*;
use aura_domain_common::SessionId;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Connection;

pub use entries::Entry;
pub use turns::Round;

/// Rounds listed when the model names none.
const DEFAULT_LISTED: usize = 10;
const MAX_LISTED: usize = 50;
const MAX_READ: usize = 10;
const DEFAULT_PAGE: u64 = 8_000;
const MAX_PAGE: u64 = 64_000;

/// The conversation's rounds, oldest first.
pub fn rounds(db: &Connection, session: SessionId) -> StoreResult<Vec<Round>> {
    turns::rounds(db, session)
}

/// One round's line, as listings and the context's index show it.
#[must_use]
pub fn line(round: &Round) -> HistoryTurnLine {
    let chars = round.first_message.chars().count();
    let mut first_message: String =
        round.first_message.chars().take(HISTORY_FIRST_MESSAGE_CHARS).collect();
    if chars > HISTORY_FIRST_MESSAGE_CHARS {
        first_message.push('…');
    }
    HistoryTurnLine {
        id: format!("T{}", round.number),
        sent_at: round.sent_at,
        first_message,
        first_message_chars: chars as u64,
    }
}

/// `history.list_turns`.
pub fn list(
    db: &Connection,
    session: SessionId,
    input: &HistoryListTurnsInput,
) -> StoreResult<HistoryListTurnsResult> {
    let all = rounds(db, session)?;
    let total = u32::try_from(all.len()).map_err(|_| Error::Conflict)?;
    let chosen = [input.last.is_some(), input.first.is_some(), input.ids.is_some()]
        .into_iter()
        .filter(|given| *given)
        .count()
        + usize::from(input.since.is_some() || input.until.is_some());
    if chosen > 1 {
        return Err(Error::Invalid(
            "history.list_turns: give one of last, first, ids, or since/until",
        ));
    }
    let picked: Vec<&Round> = if let Some(ids) = &input.ids {
        if ids.is_empty() || ids.len() > MAX_LISTED {
            return Err(Error::Invalid("history.list_turns: ids holds 1 to 50 rounds (T3)"));
        }
        let mut picked = Vec::new();
        for id in ids {
            let number = history_round_number(id)
                .ok_or(Error::Invalid("history.list_turns: a round id looks like T3"))?;
            if let Some(round) = all.iter().find(|round| round.number == number) {
                picked.push(round);
            }
        }
        picked
    } else if input.since.is_some() || input.until.is_some() {
        all.iter()
            .filter(|round| input.since.is_none_or(|since| round.sent_at >= since))
            .filter(|round| input.until.is_none_or(|until| round.sent_at < until))
            .take(MAX_LISTED)
            .collect()
    } else if let Some(first) = input.first {
        all.iter().take(bounded(first)?).collect()
    } else {
        let last = input.last.map_or(Ok(DEFAULT_LISTED), bounded)?;
        all.iter().skip(all.len().saturating_sub(last)).collect()
    };
    Ok(HistoryListTurnsResult { turns: picked.into_iter().map(line).collect(), total })
}

fn bounded(count: u32) -> StoreResult<usize> {
    let count = usize::try_from(count).map_err(|_| Error::Conflict)?;
    if (1..=MAX_LISTED).contains(&count) {
        Ok(count)
    } else {
        Err(Error::Invalid("history.list_turns: last and first are 1 to 50"))
    }
}

/// `history.search`.
pub fn search(
    db: &Connection,
    session: SessionId,
    input: &HistorySearchInput,
) -> StoreResult<HistorySearchResult> {
    search::search(db, session, input)
}

/// `history.read`: whole entries, a page of each.
pub fn read(
    db: &Connection,
    session: SessionId,
    input: &HistoryReadInput,
) -> StoreResult<HistoryReadResult> {
    if input.entries.is_empty() || input.entries.len() > MAX_READ {
        return Err(Error::Invalid("history.read: entries holds 1 to 10 entry ids (T3.17)"));
    }
    let offset = input.offset.unwrap_or(0);
    let limit = input.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(Error::Invalid("history.read: limit is 1 to 64000 characters"));
    }
    let all = rounds(db, session)?;
    let mut reader = rows::Reader::new(db, session);
    let mut read = std::collections::BTreeMap::<u32, Vec<Entry>>::new();
    let mut views = Vec::new();
    for id in &input.entries {
        let (number, entry) = history_entry_number(id)
            .ok_or(Error::Invalid("history.read: an entry id looks like T3.17"))?;
        let round = all
            .iter()
            .find(|candidate| candidate.number == number)
            .ok_or(Error::Invalid("history.read: no such round in this conversation"))?;
        let entries = match read.entry(number) {
            std::collections::btree_map::Entry::Occupied(known) => known.into_mut(),
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(entries::of(&reader.rows(round)?))
            }
        };
        let found = entries
            .iter()
            .find(|candidate| candidate.number == entry)
            .ok_or(Error::Invalid("history.read: no such entry in that round"))?;
        views.push(found.view(number, offset, limit));
    }
    Ok(HistoryReadResult { entries: views })
}

/// One of the history tools, by name, with the model's arguments.
pub(crate) fn dispatch(
    db: &Connection,
    session: SessionId,
    name: &str,
    arguments: &serde_json::Value,
) -> StoreResult<serde_json::Value> {
    Ok(match name {
        "history.list_turns" => serde_json::to_value(list(db, session, &input(arguments)?)?)?,
        "history.search" => serde_json::to_value(search(db, session, &input(arguments)?)?)?,
        "history.read" => serde_json::to_value(read(db, session, &input(arguments)?)?)?,
        _ => return Err(Error::Conflict),
    })
}

fn input<T: serde::de::DeserializeOwned>(arguments: &serde_json::Value) -> StoreResult<T> {
    serde_json::from_value(arguments.clone()).map_err(Into::into)
}

/// The newest `count` rounds' lines, oldest first, of those begun by `until` when it is given:
/// the index a compaction leaves in the context.
pub fn recent(
    db: &Connection,
    session: SessionId,
    count: usize,
    until: Option<aura_domain_common::UtcTimestamp>,
) -> StoreResult<Vec<HistoryTurnLine>> {
    let all = rounds(db, session)?
        .into_iter()
        .filter(|round| until.is_none_or(|until| round.sent_at <= until))
        .collect::<Vec<_>>();
    Ok(all.iter().skip(all.len().saturating_sub(count)).map(line).collect())
}
