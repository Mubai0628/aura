use crate::{DatabaseConnection, PersistenceCoreError as Error, StoreResult, open_reader};
use rusqlite::{Connection, params};
use std::path::Path;

pub const CORE_SCHEMA: &str = include_str!("schema.sql");
pub const APPLICATION_ID: i32 = 0x41555232;

#[derive(Clone, Copy)]
pub struct SchemaModule {
    pub owner: &'static str,
    pub sql: &'static str,
}

/// Only composition knows domain modules; this crate never imports a Store.
pub fn initialize(db: &mut DatabaseConnection, modules: &[SchemaModule]) -> StoreResult<()> {
    db.unit_of_work(|tx| {
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        if count != 0 {
            return Err(Error::Invalid("empty V2 database required"));
        }
        tx.execute_batch(CORE_SCHEMA)?;
        for module in std::iter::once(SchemaModule { owner: "core", sql: CORE_SCHEMA })
            .chain(modules.iter().copied())
        {
            if module.owner != "core" {
                tx.execute_batch(module.sql)?;
            }
            tx.execute(
                "INSERT INTO v2_schema_metadata VALUES(?1,1,?2)",
                params![module.owner, blake3::hash(module.sql.as_bytes()).to_hex().as_str()],
            )?;
        }
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        tx.pragma_update(None, "user_version", 1)?;
        Ok(())
    })
}

/// How much of a store is checked before a writer opens it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenCheck {
    /// Every page, index and foreign key: the previous process did not say it was exiting.
    Full,
    /// The previous process exited normally, so only the structure is
    /// checked first, and the caller runs [`quick_check`] while the store serves.
    Structure,
}

/// What a store still owes once it has opened with its structure
/// checked, run on its own connection while the store serves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingCheck {
    /// The store was made by this process and checked as it was made.
    None,
    /// The previous process exited normally: SQLite's quick check.
    Quick,
    /// The previous process did not say it was exiting: every page, index and foreign key. On a
    /// store of several gigabytes this takes minutes, longer than Main waits for Core to start.
    Full,
}

/// How many pages a check reads between two looks at whether it should stop.
const STOP_POLL_PAGES: i32 = 64;

/// The check a store owes, on its own read-only connection. `stopped` is asked as the check reads;
/// once it answers yes the check ends at once with an error, so a quitting Core need not wait.
pub fn background_check(
    path: &Path,
    check: PendingCheck,
    stopped: impl FnMut() -> bool + Send + 'static,
) -> StoreResult<()> {
    if check == PendingCheck::None {
        return Ok(());
    }
    let reader = open_reader(path)?;
    reader.progress_handler(STOP_POLL_PAGES, Some(stopped))?;
    if check == PendingCheck::Full {
        return integrity(&reader);
    }
    let result: String = reader.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if result != "ok" {
        return Err(Error::Invalid("SQLite quick check"));
    }
    Ok(())
}

pub fn verify(connection: &Connection, modules: &[SchemaModule]) -> StoreResult<()> {
    verify_structure(connection, modules)?;
    integrity(connection)
}

/// The application, version, owners, digests and table structure; no page is read.
fn verify_structure(connection: &Connection, modules: &[SchemaModule]) -> StoreResult<()> {
    let app: i32 = connection.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version: i32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if app != APPLICATION_ID || version != 1 {
        return Err(Error::Invalid("V2 baseline required; legacy import is explicit"));
    }
    let count: i64 =
        connection.query_row("SELECT count(*) FROM v2_schema_metadata", [], |r| r.get(0))?;
    if count != modules.len() as i64 + 1 {
        return Err(Error::Invalid("schema owner inventory"));
    }
    for module in std::iter::once(SchemaModule { owner: "core", sql: CORE_SCHEMA })
        .chain(modules.iter().copied())
    {
        let digest: String = connection.query_row(
            "SELECT digest FROM v2_schema_metadata WHERE owner=?1 AND version=1",
            [module.owner],
            |r| r.get(0),
        )?;
        if digest != blake3::hash(module.sql.as_bytes()).to_hex().as_str() {
            return Err(Error::Invalid("schema digest"));
        }
    }
    let expected = Connection::open_in_memory()?;
    expected.execute_batch(CORE_SCHEMA)?;
    for module in modules {
        expected.execute_batch(module.sql)?;
    }
    if structure(connection)? != structure(&expected)? {
        return Err(Error::Invalid("actual schema structure"));
    }
    Ok(())
}

fn structure(connection: &Connection) -> StoreResult<Vec<(String, String)>> {
    let mut query=connection.prepare("SELECT name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL ORDER BY name")?;
    Ok(query.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?)
}

pub fn integrity(connection: &Connection) -> StoreResult<()> {
    let result: String = connection.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if result != "ok" {
        return Err(Error::Invalid("SQLite integrity"));
    }
    if connection.prepare("PRAGMA foreign_key_check")?.query([])?.next()?.is_some() {
        return Err(Error::Invalid("foreign key integrity"));
    }
    Ok(())
}

/// Reader validation precedes any writable connection or journal PRAGMA.
pub fn open(path: &Path, modules: &[SchemaModule]) -> StoreResult<DatabaseConnection> {
    open_checked(path, modules, OpenCheck::Full)
}

/// [`open`] with the check the caller chose: the full one, or the structure alone when the
/// previous process exited normally.
pub fn open_checked(
    path: &Path,
    modules: &[SchemaModule],
    check: OpenCheck,
) -> StoreResult<DatabaseConnection> {
    let reader = open_reader(path)?;
    verify_structure(&reader, modules)?;
    if check == OpenCheck::Full {
        integrity(&reader)?;
    }
    drop(reader);
    DatabaseConnection::open_existing(path)
}

/// SQLite's quick check, which reads every page but not every index entry, on its own
/// read-only connection so writers go on.
pub fn quick_check(path: &Path) -> StoreResult<()> {
    background_check(path, PendingCheck::Quick, || false)
}
