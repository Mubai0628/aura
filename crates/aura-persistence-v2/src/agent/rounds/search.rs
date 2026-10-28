//! Searching the conversation's own history, newest round first. There is no index: each round's
//! entries are read and looked through, within a time budget, and the result says how far back
//! it got when it stopped early.
use super::entries::{self, Entry};
use super::rows::Reader;
use aura_context_domain::*;
use aura_domain_common::SessionId;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const DEFAULT_HITS: usize = 10;
const MAX_HITS: usize = 30;
/// How long one search may read before it answers with what it found.
const BUDGET: Duration = Duration::from_millis(2_500);

pub(super) fn search(
    db: &Connection,
    session: SessionId,
    input: &HistorySearchInput,
) -> StoreResult<HistorySearchResult> {
    let words = input.query.split_whitespace().map(lower).collect::<Vec<_>>();
    if words.is_empty() {
        return Err(Error::Invalid("history.search: query holds the words to find"));
    }
    let limit = match input.limit {
        None => DEFAULT_HITS,
        Some(limit) => usize::try_from(limit)
            .ok()
            .filter(|limit| (1..=MAX_HITS).contains(limit))
            .ok_or(Error::Invalid("history.search: limit is 1 to 30"))?,
    };
    let wanted = match &input.turns {
        None => None,
        Some(ids) => Some(
            ids.iter()
                .map(|id| history_round_number(id))
                .collect::<Option<BTreeSet<_>>>()
                .ok_or(Error::Invalid("history.search: a round id looks like T3"))?,
        ),
    };
    let rounds = super::rounds(db, session)?;
    let started = Instant::now();
    let mut reader = Reader::new(db, session);
    let mut hits = Vec::new();
    let mut searched_back_to = None;
    let mut complete = true;
    let chosen = rounds
        .iter()
        .rev()
        .filter(|round| wanted.as_ref().is_none_or(|wanted| wanted.contains(&round.number)))
        .collect::<Vec<_>>();
    for (index, round) in chosen.iter().enumerate() {
        if started.elapsed() > BUDGET {
            complete = false;
            break;
        }
        searched_back_to = Some(format!("T{}", round.number));
        let found = entries::of(&reader.rows(round)?);
        let mut stopped = false;
        for entry in found.iter().rev() {
            if input.kinds.as_ref().is_some_and(|kinds| !kinds.contains(&entry.kind)) {
                continue;
            }
            if let Some(snippet) = matching(entry, &words) {
                if hits.len() == limit {
                    stopped = true;
                    break;
                }
                hits.push(HistorySearchHit {
                    entry: entry.id(round.number),
                    kind: entry.kind,
                    tool: entry.tool.clone(),
                    at: entry.at,
                    snippet,
                });
            }
        }
        if stopped || (hits.len() == limit && index + 1 < chosen.len()) {
            complete = false;
            break;
        }
    }
    Ok(HistorySearchResult { hits, complete, searched_back_to })
}

/// One lowercase character for each character, so places in the two line up.
fn lower(text: &str) -> String {
    text.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

/// The snippet around the first word, when the entry holds every word.
fn matching(entry: &Entry, words: &[String]) -> Option<String> {
    let haystack = entry.haystack();
    let folded = lower(&haystack);
    if !words.iter().all(|word| folded.contains(word.as_str())) {
        return None;
    }
    let at = folded[..folded.find(words[0].as_str())?].chars().count();
    let chars = haystack.chars().collect::<Vec<_>>();
    let length = words[0].chars().count();
    let start = at.saturating_sub(HISTORY_SNIPPET_SIDE_CHARS);
    let end = (at + length + HISTORY_SNIPPET_SIDE_CHARS).min(chars.len());
    let body = chars[start..end]
        .iter()
        .map(|c| if c.is_whitespace() { ' ' } else { *c })
        .collect::<String>();
    let body = body.split(' ').filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ");
    Some(format!(
        "{}{body}{}",
        if start > 0 { "…" } else { "" },
        if end < chars.len() { "…" } else { "" }
    ))
}
