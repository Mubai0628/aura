use super::{entries, rows::Reader};
use crate::retrieval::QueryTerms;
use aura_context_domain::HistoryEntryKind;
use aura_domain_common::{ContentHash, SessionId, UtcTimestamp};
use aura_persistence_core::StoreResult;
use rusqlite::Connection;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

const MAX_ROUNDS: usize = 256;
const MAX_DETAILED_ROUNDS: usize = 16;
const DETAIL_BUDGET: Duration = Duration::from_millis(150);

#[derive(Clone, Serialize)]
pub struct RecallHistoryHit {
    pub entry: String,
    pub version: ContentHash,
    pub at: UtcTimestamp,
    pub quote: String,
}

/// Only user-supplied source text is automatically recalled. Other entries remain
/// accessible through the explicit history tools, with their original authority.
pub fn recall_user_sources(
    db: &Connection,
    session: SessionId,
    input: &str,
) -> StoreResult<Vec<RecallHistoryHit>> {
    let query = QueryTerms::new(input).with_synonyms();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let rounds = super::context_index::recent(db, session, MAX_ROUNDS, None, 4096)?;
    let mut found = Vec::new();
    let mut consider = |entry, at, text: &str, version: Option<&ContentHash>| {
        if let Some(hit) = query.passage("", text, 420) {
            found.push((
                hit.score,
                RecallHistoryHit {
                    entry,
                    version: version
                        .cloned()
                        .unwrap_or_else(|| ContentHash::sha256(text.as_bytes())),
                    at,
                    quote: hit.excerpt,
                },
            ));
        }
    };
    let mut reader = Reader::new(db, session).without_tool_results();
    let started = Instant::now();
    for (age, head) in rounds.iter().rev().enumerate() {
        let round = &head.round;
        // Opening messages can be searched even after the detailed scan has reached its budget.
        consider(
            format!("T{}.1", round.number),
            round.sent_at,
            &round.first_message,
            Some(&head.hash),
        );
        if head.more_users
            && age < MAX_DETAILED_ROUNDS
            && started.elapsed() < DETAIL_BUDGET
            && let Ok(rows) = reader.rows(round)
        {
            for entry in entries::of(&rows) {
                if entry.number > 1 && entry.kind == HistoryEntryKind::UserMessage {
                    consider(entry.id(round.number), entry.at, &entry.text, None);
                }
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.at.cmp(&a.1.at)));
    let mut seen = BTreeSet::new();
    Ok(found
        .into_iter()
        .map(|(_, hit)| hit)
        .filter(|hit| seen.insert(hit.version.to_string()))
        .take(8)
        .collect())
}
