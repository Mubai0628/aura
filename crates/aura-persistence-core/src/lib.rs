#![forbid(unsafe_code)]

pub mod artifact;
pub mod event;
pub mod import_report;
pub mod operation;
pub mod record;
pub mod schema;

/// Re-exported so a caller that must read an optional row does not need its own
/// `rusqlite` dependency, which is optional in several crates.
pub use rusqlite::OptionalExtension;
pub use rusqlite::{Connection, Transaction};
use rusqlite::{OpenFlags, TransactionBehavior};
use std::{path::Path, time::Duration};

#[derive(Debug, thiserror::Error)]
pub enum PersistenceCoreError {
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("cannot create a new database: {0}")]
    Create(#[from] std::io::Error),
    #[error("persistent JSON is invalid")]
    Json(#[from] serde_json::Error),
    #[error("persistent state violates {0}")]
    Invalid(&'static str),
    #[error("persistent revision or identity conflict")]
    Conflict,
}

pub type StoreResult<T> = Result<T, PersistenceCoreError>;

/// Which class of real SQLite failure a caller is looking at.
///
/// The classification exists so a caller can decide "the database could not serve this" from
/// "this operation was answered" **by type** instead of by matching a message. A message is
/// provider- and locale-dependent text; a SQLite result code is the fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqliteFailureClass {
    /// The database is locked or busy: contention, not damage.
    Busy,
    /// The file is not a valid database, or its image is malformed.
    Corrupt,
    NotADatabase,
    CannotOpen,
    /// The file or one of its side files cannot be written.
    ReadOnly,
    DiskFull,
    PermissionDenied,
    /// The operating system refused an IO operation.
    IoFailure,
    /// A real SQLite failure of another class.
    Other,
}

impl SqliteFailureClass {
    #[must_use]
    pub fn of(code: rusqlite::ErrorCode) -> Option<Self> {
        match code {
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => {
                Some(Self::Busy)
            }
            rusqlite::ErrorCode::DatabaseCorrupt => Some(Self::Corrupt),
            rusqlite::ErrorCode::NotADatabase => Some(Self::NotADatabase),
            rusqlite::ErrorCode::CannotOpen => Some(Self::CannotOpen),
            rusqlite::ErrorCode::ReadOnly => Some(Self::ReadOnly),
            rusqlite::ErrorCode::DiskFull => Some(Self::DiskFull),
            rusqlite::ErrorCode::PermissionDenied => Some(Self::PermissionDenied),
            rusqlite::ErrorCode::SystemIoFailure => Some(Self::IoFailure),
            _ => Some(Self::Other),
        }
    }

    /// The stable code a caller records or shows.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Busy => "sqlite.busy",
            Self::Corrupt => "sqlite.corrupt",
            Self::NotADatabase => "sqlite.not_a_database",
            Self::CannotOpen => "sqlite.cannot_open",
            Self::ReadOnly => "sqlite.read_only",
            Self::DiskFull => "sqlite.disk_full",
            Self::PermissionDenied => "sqlite.permission_denied",
            Self::IoFailure => "sqlite.io_failure",
            Self::Other => "sqlite.other",
        }
    }
}

/// Connection and transaction boundary, not a domain Store. R02 owns the V2
/// baseline, importer, Outbox, artifact and operation journal table protocols.
/// There is no automatic opening/migration of a user or legacy database here.
pub struct DatabaseConnection {
    connection: Connection,
}

impl DatabaseConnection {
    pub fn in_memory() -> Result<Self, PersistenceCoreError> {
        Self::configure(Connection::open_in_memory()?)
    }

    /// Never truncates or reuses an existing file, including a zero-byte file.
    pub fn create_new(path: &Path) -> Result<Self, PersistenceCoreError> {
        drop(std::fs::OpenOptions::new().write(true).create_new(true).open(path)?);
        Self::open_existing(path)
    }

    pub fn open_existing(path: &Path) -> Result<Self, PersistenceCoreError> {
        Self::configure(Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?)
    }

    fn configure(connection: Connection) -> Result<Self, PersistenceCoreError> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        Ok(Self { connection })
    }

    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Domain stores share this transaction for state, idempotency, journal and
    /// Outbox writes. Nothing is published before this method returns success.
    /// On error or unwinding SQLite rolls back the entire Unit of Work.
    pub fn unit_of_work<T, E: From<rusqlite::Error>>(
        &mut self,
        work: impl FnOnce(&Transaction<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let transaction =
            self.connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = work(&transaction)?;
        transaction.commit()?;
        Ok(result)
    }
}

pub fn open_reader(path: &Path) -> Result<Connection, PersistenceCoreError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "query_only", "ON")?;
    Ok(connection)
}
