//! The store's read-only connections: one lent to each read while it runs, opened when the reads
//! running at once first need it and kept for the next read. How many run at once is the read
//! lane's limit (`disk_pressure`), so there are never more connections than that lane's size.
//! Each kept connection has a number of its own, so what one connection read can be remembered
//! with what it saw (`PRAGMA data_version` is the connection's own).
use aura_persistence_core::{Connection, PersistenceCoreError};
use std::{
    cell::Cell,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

thread_local! {
    /// The number of the kept connection lent to the read running on this thread.
    static LENT: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Says, until it drops, that the read running on this thread runs on kept connection `number`.
pub(crate) struct Lent(());

impl Lent {
    pub(crate) fn to(number: u64) -> Self {
        LENT.set(Some(number));
        Self(())
    }
}

impl Drop for Lent {
    fn drop(&mut self) {
        LENT.set(None);
    }
}

/// The number of the kept read-only connection the read running on this thread was lent; `None`
/// on the write connection and outside a read.
pub(crate) fn lent() -> Option<u64> {
    LENT.get()
}

pub(crate) struct Readers {
    path: PathBuf,
    idle: Mutex<Vec<(u64, Connection)>>,
    opened: AtomicU64,
}

impl Readers {
    /// The first connection is opened at once, so a store that cannot be read this way is
    /// known at start.
    pub(crate) fn open(path: PathBuf) -> Result<Self, PersistenceCoreError> {
        let first = aura_persistence_core::open_reader(&path)?;
        Ok(Self { path, idle: Mutex::new(vec![(0, first)]), opened: AtomicU64::new(1) })
    }

    /// A connection of its own for one read, a kept one or a new one, with its number.
    pub(crate) fn take(&self) -> Result<(u64, Connection), PersistenceCoreError> {
        let kept = self
            .idle
            .lock()
            .map_err(|_| PersistenceCoreError::Invalid("agent.store_reader_lock"))?
            .pop();
        match kept {
            Some(reader) => Ok(reader),
            None => Ok((
                self.opened.fetch_add(1, Ordering::Relaxed),
                aura_persistence_core::open_reader(&self.path)?,
            )),
        }
    }

    /// Kept for the next read. One a panic took is opened again, with a new number, when needed.
    pub(crate) fn put_back(&self, reader: (u64, Connection)) {
        if let Ok(mut idle) = self.idle.lock() {
            idle.push(reader);
        }
    }

    /// Connections kept for the next reads.
    #[cfg(test)]
    pub(crate) fn kept(&self) -> usize {
        self.idle.lock().map(|idle| idle.len()).unwrap_or(0)
    }
}
