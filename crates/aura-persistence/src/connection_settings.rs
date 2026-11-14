//! How each connection to `aura.db` is set up: the one writer and the readers.
use rusqlite::Connection;

use crate::{PersistenceError, migration::BUSY_TIMEOUT};

/// The write-ahead log a large write leaves behind, once it is written back. SQLite reuses the log
/// from its start without making the file smaller; past this it cuts the file back to it.
pub(crate) const LOG_KEPT_BYTES: i64 = 64 * 1024 * 1024;

pub(crate) fn configure_writer(connection: &Connection) -> Result<(), PersistenceError> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "wal_autocheckpoint", 1000_i64)?;
    connection.pragma_update(None, "journal_size_limit", LOG_KEPT_BYTES)?;
    Ok(())
}

pub(crate) fn configure_reader(connection: &Connection) -> Result<(), PersistenceError> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "query_only", "ON")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_writer_cuts_a_log_a_large_write_left_back_to_its_limit() {
        let directory = tempfile::tempdir().expect("temp root");
        let connection = Connection::open(directory.path().join("aura.db")).expect("open");
        configure_writer(&connection).expect("writer settings");
        let limit: i64 = connection
            .query_row("PRAGMA journal_size_limit", [], |row| row.get(0))
            .expect("journal_size_limit");
        assert_eq!(limit, LOG_KEPT_BYTES);
    }
}
