//! Rebuilding the file once its deleted pages are many: SQLite's `VACUUM` writes the pages it
//! keeps into a new copy in the temporary folder and then back into the store through the
//! write-ahead log, so it either completes or leaves the store as it was. It needs about twice the
//! bytes it keeps; without them the rebuild is owed to a later run.
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use aura_persistence_core::{DatabaseConnection, StoreResult};

use super::{Rebuild, Step};

/// Fewer deleted bytes than this are not worth rewriting the whole file for; SQLite reuses them.
const REBUILD_AT: u64 = 64 * 1024 * 1024;
/// How often the caller hears that the rebuild still goes on.
const HEARTBEAT: Duration = Duration::from_secs(1);
/// SQLite instructions between two looks at whether the caller asked to stop.
const STOP_POLL: i32 = 100;

fn pragma(store: &DatabaseConnection, name: &str) -> StoreResult<u64> {
    Ok(store.connection().pragma_query_value(None, name, |row| row.get::<_, i64>(0))? as u64)
}

/// Rebuilds the file when enough of it was deleted and the disk has room; `None` when the caller
/// stopped it. The store is handed back either way.
pub(super) fn rebuild(
    store: DatabaseConnection,
    database: &Path,
    free_space: &dyn Fn(&Path) -> std::io::Result<u64>,
    step: &mut dyn FnMut(Step) -> std::ops::ControlFlow<()>,
) -> StoreResult<(Option<Rebuild>, DatabaseConnection)> {
    let page = pragma(&store, "page_size")?;
    let free_pages = pragma(&store, "freelist_count")?;
    if free_pages * page < REBUILD_AT {
        return Ok((Some(Rebuild::NotNeeded), store));
    }
    let kept = (pragma(&store, "page_count")? - free_pages) * page;
    // The copy, then the same pages again in the write-ahead log, and a tenth to spare.
    let needed = kept * 2 + kept / 10;
    let folder = database.parent().unwrap_or(database);
    let free = free_space(folder)?;
    let temporary = free_space(&std::env::temp_dir())?;
    if free < needed {
        return Ok((Some(Rebuild::NoRoom { needed, free }), store));
    }
    if temporary < kept + kept / 10 {
        return Ok((Some(Rebuild::NoRoom { needed: kept + kept / 10, free: temporary }), store));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let asked = Arc::clone(&stop);
    store.connection().progress_handler(STOP_POLL, Some(move || asked.load(Ordering::Acquire)))?;
    if step(Step::Rebuilding { kept }).is_break() {
        stop.store(true, Ordering::Release);
    }
    let (result, store) = std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let result = store.connection().execute_batch("VACUUM");
            (result, store)
        });
        let mut heard = Instant::now();
        while !worker.is_finished() {
            std::thread::sleep(Duration::from_millis(50));
            if heard.elapsed() >= HEARTBEAT {
                heard = Instant::now();
                if step(Step::Rebuilding { kept }).is_break() {
                    stop.store(true, Ordering::Release);
                }
            }
        }
        worker.join()
    })
    .map_err(|_| aura_persistence_core::PersistenceCoreError::Invalid("store rebuild thread"))?;
    store.connection().progress_handler(0, None::<fn() -> bool>)?;
    match result {
        Ok(()) => Ok((Some(Rebuild::Done), store)),
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::OperationInterrupted
                && stop.load(Ordering::Acquire) =>
        {
            Ok((None, store))
        }
        Err(error) => Err(error.into()),
    }
}
