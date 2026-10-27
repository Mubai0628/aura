//! The revision of the history page a conversation polls, as the desktop compares it: it moves
//! when this conversation's page has changed, and only then.
//!
//! The store's revision also moves for every other conversation's events, because the journal's
//! sequence is shared, so a conversation re-read and re-rendered its history whenever another one
//! was streaming. The page is fingerprinted (without what running steps write into tool calls,
//! which the desktop takes from every poll) and its revision kept while the fingerprint is the
//! same. A changed page gets a revision above both the last one and the store's, so it never goes
//! back. Each conversation read in this process is remembered for the process's life: forgetting
//! one could hand it a lower revision than the desktop already holds, which it would then ignore.
use aura_domain_common::{ContentHash, Revision, SessionId};
use aura_projection_contract::AgentSessionProjection;
use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard, PoisonError},
};

/// The last fingerprint and revision of each conversation's newest page, by page size.
type Pages = BTreeMap<(SessionId, u32), (ContentHash, u64)>;
static PAGES: Mutex<Pages> = Mutex::new(BTreeMap::new());

fn pages() -> MutexGuard<'static, Pages> {
    PAGES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sets the revision of a conversation's newest page of `limit` turns, from its content.
pub(super) fn settle(
    projection: &mut AgentSessionProjection,
    limit: u32,
) -> serde_json::Result<()> {
    let stored = std::mem::replace(&mut projection.revision, Revision::INITIAL);
    let fingerprint = ContentHash::blake3(&serde_json::to_vec(&*projection)?);
    let mut pages = pages();
    let revision = match pages.get(&(projection.session_id, limit)) {
        Some((last, revision)) if *last == fingerprint => *revision,
        Some((_, revision)) => stored.value().max(revision + 1),
        None => stored.value(),
    };
    pages.insert((projection.session_id, limit), (fingerprint, revision));
    projection.revision = Revision::new(revision);
    Ok(())
}
