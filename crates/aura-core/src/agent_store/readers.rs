//! The store's read-only connections: one lent to each read while it runs, opened when the reads
//! running at once first need it and kept for the next read. How many run at once is the read
//! lane's limit (`disk_pressure`), so there are never more connections than that lane's size.
use aura_persistence_core::{Connection, PersistenceCoreError};
use std::{path::PathBuf, sync::Mutex};

pub(crate) struct Readers {
    path: PathBuf,
    idle: Mutex<Vec<Connection>>,
}

impl Readers {
    /// The first connection is opened at once, so a store that cannot be read this way is
    /// known at start.
    pub(crate) fn open(path: PathBuf) -> Result<Self, PersistenceCoreError> {
        let first = aura_persistence_core::open_reader(&path)?;
        Ok(Self { path, idle: Mutex::new(vec![first]) })
    }

    /// A connection of its own for one read: a kept one, or a new one.
    pub(crate) fn take(&self) -> Result<Connection, PersistenceCoreError> {
        let kept = self
            .idle
            .lock()
            .map_err(|_| PersistenceCoreError::Invalid("agent.store_reader_lock"))?
            .pop();
        match kept {
            Some(connection) => Ok(connection),
            None => aura_persistence_core::open_reader(&self.path),
        }
    }

    /// Kept for the next read. One a panic took is opened again when next needed.
    pub(crate) fn put_back(&self, connection: Connection) {
        if let Ok(mut idle) = self.idle.lock() {
            idle.push(connection);
        }
    }

    /// Connections kept for the next reads.
    #[cfg(test)]
    pub(crate) fn kept(&self) -> usize {
        self.idle.lock().map(|idle| idle.len()).unwrap_or(0)
    }
}
