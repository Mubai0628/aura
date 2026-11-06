//! Whether an update must copy the store before this build first starts: it migrates the store,
//! or the version it replaces would refuse what it writes. Read without writing.
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::PersistenceError;
use crate::migration::{LATEST_SCHEMA_VERSION, current_schema_version};

/// The oldest version that reads every record this build writes into the store. 0.1.12 and
/// earlier refuse a model profile whose request defaults allow speed modes. Raised to a new
/// version whenever it starts storing a field or an enum value its predecessor refuses.
pub const RECORDS_READABLE_SINCE: &str = "0.2.0";

/// Whether the store at `database_path` needs a copy before this build first starts in place of
/// `replacing` (unknown: any older version): this build migrates it, or `replacing` would refuse
/// what this build writes into it. A missing or empty file is a new store, which nothing needs
/// to protect.
pub fn copy_before_start(
    database_path: &Path,
    replacing: Option<&str>,
) -> Result<bool, PersistenceError> {
    Ok(holds_data(database_path)
        && (store_migration_pending(database_path)? || refuses_records(replacing)))
}

fn holds_data(database_path: &Path) -> bool {
    database_path.is_file()
        && std::fs::metadata(database_path).is_ok_and(|metadata| metadata.len() > 0)
}

fn refuses_records(replacing: Option<&str>) -> bool {
    let parts = |version: &str| -> Vec<u64> {
        version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
    };
    replacing.is_none_or(|version| parts(version) < parts(RECORDS_READABLE_SINCE))
}

/// Whether this build would migrate the store at `database_path`.
fn store_migration_pending(database_path: &Path) -> Result<bool, PersistenceError> {
    if !holds_data(database_path) {
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

    /// 0.1.12 refuses a model profile whose request defaults allow speed modes: a store this build
    /// wrote would leave it unable to read the model settings, so the store is copied although
    /// its schema stays the same. A version that reads it, or a missing store, needs no copy.
    #[test]
    fn a_current_store_is_copied_before_replacing_a_version_that_refuses_its_records() {
        let directory = tempfile::tempdir().expect("temporary store directory");
        let path = directory.path().join("aura.db");
        assert!(!copy_before_start(&path, None).expect("missing"));
        let crate::migration::MigrationResult::Ready(connection) =
            crate::migration::open_and_migrate(&path, crate::DatabaseOpenOptions::default())
                .expect("a current store")
        else {
            panic!("a new store opens ready")
        };
        drop(connection);
        assert!(copy_before_start(&path, Some("0.1.12")).expect("0.1.12"));
        assert!(copy_before_start(&path, Some("0.1.7")).expect("0.1.7"));
        assert!(
            copy_before_start(&path, None).expect("unknown"),
            "a launcher that does not say what it replaces may be replacing any older version"
        );
        assert!(!copy_before_start(&path, Some(RECORDS_READABLE_SINCE)).expect("readable"));
        assert!(!copy_before_start(&path, Some("0.10.0")).expect("newer"));
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
