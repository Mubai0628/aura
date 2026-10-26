//! Whether opening a store would migrate it, read without writing. An update copies a store
//! before its first start only when the new version will change it.
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::PersistenceError;
use crate::migration::{LATEST_SCHEMA_VERSION, current_schema_version};

/// Whether this build would migrate the store at `database_path`. A missing or empty file is a
/// new store, which nothing needs to protect.
pub fn store_migration_pending(database_path: &Path) -> Result<bool, PersistenceError> {
    if !database_path.is_file()
        || std::fs::metadata(database_path).map(|metadata| metadata.len() == 0).unwrap_or(true)
    {
        return Ok(false);
    }
    let connection = Connection::open_with_flags(
        database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(current_schema_version(&connection)? != LATEST_SCHEMA_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_store_needs_nothing_and_an_older_one_needs_migrating() {
        let directory = tempfile::tempdir().expect("temporary store directory");
        let path = directory.path().join("aura.db");
        assert!(!store_migration_pending(&path).expect("missing"));
        let connection = Connection::open(&path).expect("store");
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY);
                 INSERT INTO schema_migrations(version) VALUES (1);",
            )
            .expect("an old store");
        drop(connection);
        assert!(store_migration_pending(&path).expect("old"));
    }

    #[test]
    fn a_store_this_build_created_needs_no_migration() {
        let directory = tempfile::tempdir().expect("temporary store directory");
        let path = directory.path().join("aura.db");
        let crate::migration::MigrationResult::Ready(connection) =
            crate::migration::open_and_migrate(&path, crate::DatabaseOpenOptions::default())
                .expect("a current store")
        else {
            panic!("a new store opens ready")
        };
        drop(connection);
        assert!(!store_migration_pending(&path).expect("current"));
    }
}
