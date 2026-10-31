//! The newest history page of a conversation, kept per read connection while nothing is committed.
//!
//! The desktop polls the open conversation's newest turns every second, four times a second while
//! a turn runs, and each poll built the page again from every step of those turns: about 3 MB of
//! reading a poll on a long conversation, 80–170 MB a minute while nothing happened. A read
//! connection tells by its own `PRAGMA data_version` whether another connection committed since it
//! last read; while none did, the page it built last is still the page.
use aura_domain_common::SessionId;
use aura_persistence_core::{Connection, PersistenceCoreError};
use aura_projection_contract::AgentSessionProjection;
use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard, PoisonError},
};

/// Pages kept per read connection: the conversation the desktop polls and one read ahead of a click.
const PER_CONNECTION: usize = 2;

/// A page by the read connection's number, the conversation and the page size.
type Key = (u64, SessionId, u32);

/// What read connections last built, each page with the connection's data version then.
struct Kept<P> {
    pages: BTreeMap<Key, (i64, u64, P)>,
    /// Orders the pages by when they were last used.
    tick: u64,
}

impl<P: Clone> Kept<P> {
    const fn new() -> Self {
        Self { pages: BTreeMap::new(), tick: 0 }
    }

    /// The page `key` names, when its connection built it at `version`.
    fn get(&mut self, key: Key, version: i64) -> Option<P> {
        self.tick += 1;
        let (built_at, used, page) = self.pages.get_mut(&key)?;
        if *built_at != version {
            return None;
        }
        *used = self.tick;
        Some(page.clone())
    }

    /// Keeps `page`, built at `version`, in place of the page its connection used longest ago.
    fn put(&mut self, key: Key, version: i64, page: P) {
        self.tick += 1;
        let others: Vec<(Key, u64)> = self
            .pages
            .iter()
            .filter(|(other, _)| other.0 == key.0 && **other != key)
            .map(|(other, (_, used, _))| (*other, *used))
            .collect();
        if others.len() >= PER_CONNECTION
            && let Some((oldest, _)) = others.iter().min_by_key(|(_, used)| *used)
        {
            self.pages.remove(oldest);
        }
        self.pages.insert(key, (version, self.tick, page));
    }
}

static HISTORY: Mutex<Kept<AgentSessionProjection>> = Mutex::new(Kept::new());

fn history() -> MutexGuard<'static, Kept<AgentSessionProjection>> {
    HISTORY.lock().unwrap_or_else(PoisonError::into_inner)
}

fn data_version(db: &Connection) -> Result<i64, PersistenceCoreError> {
    Ok(db.query_row("PRAGMA data_version", [], |row| row.get(0))?)
}

/// The newest page of `session` with `limit` turns, built by `build` unless read connection
/// `reader` built it and nothing was committed since.
pub(super) fn newest_page(
    db: &Connection,
    reader: u64,
    session: SessionId,
    limit: u32,
    build: impl FnOnce() -> Result<AgentSessionProjection, PersistenceCoreError>,
) -> Result<AgentSessionProjection, PersistenceCoreError> {
    let key = (reader, session, limit);
    let version = data_version(db)?;
    if let Some(page) = history().get(key, version) {
        return Ok(page);
    }
    let page = build()?;
    history().put(key, version, page.clone());
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn kept_or_built(kept: &mut Kept<String>, key: Key, version: i64, built: &Cell<u32>) -> String {
        if let Some(page) = kept.get(key, version) {
            return page;
        }
        built.set(built.get() + 1);
        let page = format!("page {} at {version}", key.1);
        kept.put(key, version, page.clone());
        page
    }

    #[test]
    fn a_connection_builds_the_page_again_only_after_another_connection_committed() {
        let directory = tempfile::tempdir().expect("directory");
        let mut store =
            aura_persistence_v2::V2Store::create(&directory.path().join("v2")).expect("store");
        let reader =
            aura_persistence_core::open_reader(&store.database_path()).expect("read connection");
        let mut kept = Kept::new();
        let key = (1, SessionId::new(), 2);
        let built = Cell::new(0);
        let poll = |kept: &mut Kept<String>| {
            let version = data_version(&reader).expect("data version");
            kept_or_built(kept, key, version, &built)
        };
        let first = poll(&mut kept);
        assert_eq!(poll(&mut kept), first);
        assert_eq!(built.get(), 1, "nothing was committed between the two polls");
        store
            .database
            .unit_of_work(|tx| {
                tx.execute(
                    "INSERT INTO artifact_objects(hash, byte_length, bytes) VALUES ('h', 0, x'')",
                    [],
                )
            })
            .expect("commit");
        poll(&mut kept);
        assert_eq!(built.get(), 2, "the writer committed");
        poll(&mut kept);
        assert_eq!(built.get(), 2);
    }

    #[test]
    fn a_connection_keeps_two_pages_and_gives_up_the_one_used_longest_ago() {
        let mut kept = Kept::new();
        let built = Cell::new(0);
        let [first, second, third] = [SessionId::new(), SessionId::new(), SessionId::new()];
        kept_or_built(&mut kept, (1, first, 2), 7, &built);
        kept_or_built(&mut kept, (1, second, 2), 7, &built);
        kept_or_built(&mut kept, (1, first, 2), 7, &built);
        kept_or_built(&mut kept, (2, third, 2), 7, &built);
        assert_eq!(built.get(), 3, "the first conversation was kept");
        kept_or_built(&mut kept, (1, third, 2), 7, &built);
        kept_or_built(&mut kept, (1, first, 2), 7, &built);
        assert_eq!(built.get(), 4, "the second, used longest ago, made room for the third");
        kept_or_built(&mut kept, (1, second, 2), 7, &built);
        assert_eq!(built.get(), 5);
        kept_or_built(&mut kept, (2, third, 2), 7, &built);
        assert_eq!(built.get(), 5, "another connection keeps its own pages");
    }
}
