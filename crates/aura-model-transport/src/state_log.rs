//! The task state on a path that sends it before the conversation, where the provider caches whole
//! earlier requests. Each request of a conversation only adds to the one before: a state that
//! changes is added where it changed instead of replacing the one in front, so the conversation
//! before it stays cached. What was sent is kept in memory only. After a restart, a compaction or
//! any other change to what came before, the conversation starts again from its state then, which
//! costs one request without the cache.
use aura_domain_common::{ContentHash, SessionId};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

#[cfg(test)]
#[path = "state_log_tests.rs"]
mod tests;

/// Conversations remembered at once.
const CONVERSATIONS: usize = 64;
/// Changes added to one conversation before it starts again from its state then.
const CHANGES: usize = 32;

/// Where the task state goes in one request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatePlacement {
    /// The state before the conversation.
    pub leading: String,
    /// Each later state, after how many of the conversation's items it was added.
    pub changes: Vec<(usize, String)>,
}

/// A later state as it was sent: after how many items, and the hash of the items before it.
struct Change {
    at: usize,
    state: String,
    before: String,
}

struct Sent {
    leading: String,
    changes: Vec<Change>,
    used: u64,
}

#[derive(Default)]
struct Log {
    conversations: HashMap<(SessionId, String), Sent>,
    clock: u64,
}

fn log() -> &'static Mutex<Log> {
    static LOG: OnceLock<Mutex<Log>> = OnceLock::new();
    LOG.get_or_init(Mutex::default)
}

/// The hash of the first `count` items, from the hashes of each.
fn prefix(items: &[String], count: usize) -> String {
    ContentHash::sha256(items[..count].concat().as_bytes()).hex
}

/// Places the state `current` in a request of `session`'s conversation that begins with the
/// instructions `stable` and holds `items` before any closing question.
///
/// The state sent first stays in front, and each change is added after the items it followed when
/// it was sent. A request that holds fewer items than a change it would carry, such as one asked
/// about part of the conversation, gets its state after its own items and changes nothing kept.
#[must_use]
pub fn place_state(
    session: SessionId,
    stable: &str,
    items: &[Value],
    current: &str,
) -> StatePlacement {
    let hashes = items
        .iter()
        .map(|item| ContentHash::sha256(&serde_json::to_vec(item).unwrap_or_default()).hex)
        .collect::<Vec<_>>();
    let mut basis = stable.as_bytes().to_vec();
    if let Some(first) = hashes.first() {
        basis.push(b'\n');
        basis.extend(first.as_bytes());
    }
    let key = (session, ContentHash::sha256(&basis).hex);
    let mut log = log().lock().unwrap_or_else(PoisonError::into_inner);
    log.clock += 1;
    let clock = log.clock;
    let fresh = || Sent { leading: current.to_owned(), changes: Vec::new(), used: clock };
    let kept = log.conversations.get(&key).is_some_and(|sent| {
        sent.changes
            .iter()
            .all(|change| change.at > items.len() || prefix(&hashes, change.at) == change.before)
    });
    if !kept {
        if log.conversations.len() >= CONVERSATIONS && !log.conversations.contains_key(&key) {
            let oldest = log
                .conversations
                .iter()
                .min_by_key(|(_, sent)| sent.used)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                log.conversations.remove(&oldest);
            }
        }
        log.conversations.insert(key.clone(), fresh());
    }
    let Some(sent) = log.conversations.get_mut(&key) else {
        return StatePlacement { leading: current.to_owned(), changes: Vec::new() };
    };
    sent.used = clock;
    let applies = sent.changes.iter().filter(|change| change.at <= items.len()).count();
    let mut placement = StatePlacement {
        leading: sent.leading.clone(),
        changes: sent.changes[..applies]
            .iter()
            .map(|change| (change.at, change.state.clone()))
            .collect(),
    };
    let last = placement.changes.last().map_or(&placement.leading, |(_, state)| state);
    if last == current {
        return placement;
    }
    if applies < sent.changes.len() {
        placement.changes.push((items.len(), current.to_owned()));
    } else if sent.changes.len() < CHANGES {
        sent.changes.push(Change {
            at: items.len(),
            state: current.to_owned(),
            before: prefix(&hashes, items.len()),
        });
        placement.changes.push((items.len(), current.to_owned()));
    } else {
        *sent = fresh();
        placement = StatePlacement { leading: current.to_owned(), changes: Vec::new() };
    }
    placement
}
