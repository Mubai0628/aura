//! Rewriting `aura.db` while the run before a start has it alone. A long statement runs on a thread
//! of its own while the caller hears, at once and then about every second, that it goes on; the
//! caller may stop it, and SQLite then interrupts it and rolls it back.
//!
//! Rebuilding the file once its free pages are many: `VACUUM` writes the pages it keeps into a new
//! copy in the temporary folder and then back into the file through the write-ahead log, so it
//! either completes or leaves the file as it was. It needs about twice the bytes it keeps; without
//! them the rebuild is owed to a later run.
//!
//! The execution store rebuilds the same way (`aura_persistence_v2::slimming`). The code is not
//! shared: the sandbox host and the egress proxy still depend on this crate, so a store crate
//! under it would be a new route from them to SQL (`scripts/architecture/graph.mjs`).
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Serialize;

use crate::PersistenceError;

/// Fewer free bytes than this are not worth rewriting the whole file for; SQLite reuses them.
pub(crate) const REBUILD_AT: u64 = 64 * 1024 * 1024;
/// How often the caller hears that the statement still goes on.
const HEARTBEAT: Duration = Duration::from_secs(1);
/// SQLite instructions between two looks at whether the caller asked to stop.
const STOP_POLL: i32 = 100;

/// What became of the rebuild.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Rebuild {
    /// Too little of the file is free to be worth rewriting it.
    #[default]
    NotNeeded,
    Done,
    /// The disk lacks room for the rebuild; it is owed to a later run.
    NoRoom {
        needed: u64,
        free: u64,
    },
}

fn pragma(connection: &Connection, name: &str) -> Result<u64, PersistenceError> {
    Ok(connection.pragma_query_value(None, name, |row| row.get::<_, i64>(0))? as u64)
}

/// The bytes of the file's pages in use, and of those free.
pub(crate) fn page_bytes(connection: &Connection) -> Result<(u64, u64), PersistenceError> {
    let page = pragma(connection, "page_size")?;
    let free = pragma(connection, "freelist_count")?;
    let pages = pragma(connection, "page_count")?;
    Ok((pages.saturating_sub(free) * page, free * page))
}

/// Runs `sql`, telling `step` at once and then about every second that it goes on. `false` when
/// `step` stopped it: the statement was interrupted and rolled back. The connection is handed back
/// unless the statement failed.
pub(crate) fn run_heard(
    connection: Connection,
    sql: &str,
    step: &mut dyn FnMut() -> ControlFlow<()>,
) -> Result<(bool, Connection), PersistenceError> {
    let stop = Arc::new(AtomicBool::new(false));
    let asked = Arc::clone(&stop);
    connection.progress_handler(STOP_POLL, Some(move || asked.load(Ordering::Acquire)))?;
    if step().is_break() {
        stop.store(true, Ordering::Release);
    }
    let (result, connection) = std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let result = connection.execute_batch(sql);
            (result, connection)
        });
        let mut heard = Instant::now();
        while !worker.is_finished() {
            std::thread::sleep(Duration::from_millis(50));
            if heard.elapsed() >= HEARTBEAT {
                heard = Instant::now();
                if step().is_break() {
                    stop.store(true, Ordering::Release);
                }
            }
        }
        worker.join()
    })
    .map_err(|_| PersistenceError::InvalidRequest("store rewrite thread".to_owned()))?;
    connection.progress_handler(0, None::<fn() -> bool>)?;
    match result {
        Ok(()) => Ok((true, connection)),
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::OperationInterrupted
                && stop.load(Ordering::Acquire) =>
        {
            Ok((false, connection))
        }
        Err(error) => Err(error.into()),
    }
}

/// Rebuilds the file at `database` when enough of it is free and `free_space` says the disk has
/// room; `None` when `step`, told the bytes kept, stopped it. The connection is handed back either
/// way.
pub(crate) fn rebuild(
    connection: Connection,
    database: &Path,
    free_space: &dyn Fn(&Path) -> std::io::Result<u64>,
    step: &mut dyn FnMut(u64) -> ControlFlow<()>,
) -> Result<(Option<Rebuild>, Connection), PersistenceError> {
    let (kept, free_bytes) = page_bytes(&connection)?;
    if free_bytes < REBUILD_AT {
        return Ok((Some(Rebuild::NotNeeded), connection));
    }
    // The copy, then the same pages again in the write-ahead log, and a tenth to spare.
    let needed = kept * 2 + kept / 10;
    let measure = |path: &Path| {
        free_space(path).map_err(|source| PersistenceError::Io {
            action: "measure free disk space",
            path: path.to_path_buf(),
            source,
        })
    };
    let free = measure(database.parent().unwrap_or(database))?;
    let temporary = measure(&std::env::temp_dir())?;
    if free < needed {
        return Ok((Some(Rebuild::NoRoom { needed, free }), connection));
    }
    if temporary < kept + kept / 10 {
        let needed = kept + kept / 10;
        return Ok((Some(Rebuild::NoRoom { needed, free: temporary }), connection));
    }
    let (done, connection) = run_heard(connection, "VACUUM", &mut || step(kept))?;
    Ok((done.then_some(Rebuild::Done), connection))
}
