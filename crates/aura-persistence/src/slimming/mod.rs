//! What `aura.db` keeps and nothing reads again, cleared by the run before a start (the launcher's
//! `aura-core --store-slim`) while nothing else has the file open:
//! - the text index, rewritten (FTS5 `optimize`) once it is many times its text (`text_index`);
//! - every model catalog version but the current one, with their raw responses (`catalog`);
//! - the free pages these and other deletions leave, given back to the disk by rebuilding the file
//!   (`VACUUM`) once they are many and the disk has room for it.
//!
//! The run changes no structure, so the version a newer one replaces still opens a store it slimmed.
//!
//! A marker beside the file (`MARKER`) holds the bytes in use that the last run kept, and whether a
//! run is owed. A normal exit marks one owed when enough of the file is free, or in use beyond what
//! the last run kept (`note_at_exit`). An owed run counts the runs that started on it, so the
//! launcher can stop retrying one that never ends or always fails; a run that ends clears it, and
//! one short of room for the rebuild leaves it owed without counting.
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::PersistenceError;

pub(crate) mod catalog;
mod rebuild;
mod text_index;

use rebuild::{REBUILD_AT, page_bytes, run_heard};

pub use rebuild::Rebuild;

/// Beside `aura.db`; see the module documentation.
pub const MARKER: &str = "aura-db-slimming.json";
/// How long a run waits for another connection to let go of the file.
const EXCLUSIVE_WAIT: Duration = Duration::from_secs(60);
/// Bytes in use beyond what the last run kept that owe a run, as free bytes do.
const GROWTH_AT: u64 = REBUILD_AT;

/// The marker: what the last run kept, and the runs started on an owed run without ending it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Maintenance {
    pub schema_version: u32,
    /// Bytes of the pages in use after the last run.
    pub kept_bytes: u64,
    /// `None` while no run is owed.
    pub owed_runs: Option<u32>,
}

/// The text index's bytes before and after it was rewritten.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TextIndex {
    pub before_bytes: u64,
    pub after_bytes: u64,
}

/// What a run did.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Report {
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub text_index: Option<TextIndex>,
    /// Catalog versions dropped.
    pub catalog_versions: u64,
    pub rebuild: Rebuild,
    /// The caller asked the run to stop; the rest is owed to the next run.
    pub stopped: bool,
}

fn marker_path(database: &Path) -> PathBuf {
    database.with_file_name(MARKER)
}

/// The marker beside `database`, when there is one this build reads.
pub fn maintenance(database: &Path) -> Option<Maintenance> {
    let marker: Maintenance =
        serde_json::from_slice(&std::fs::read(marker_path(database)).ok()?).ok()?;
    (marker.schema_version == 1).then_some(marker)
}

fn write_marker(
    database: &Path,
    kept_bytes: u64,
    owed_runs: Option<u32>,
) -> Result<(), PersistenceError> {
    let path = marker_path(database);
    let marker = Maintenance { schema_version: 1, kept_bytes, owed_runs };
    std::fs::write(&path, format!("{}\n", serde_json::to_string(&marker)?)).map_err(|source| {
        PersistenceError::Io { action: "write the slimming marker", path, source }
    })
}

/// At a normal exit, on the writer: marks a run owed when enough of the file is free or in use
/// beyond what the last run kept. Reads only the page counts. Whether a run is owed.
pub(crate) fn note_at_exit(
    connection: &Connection,
    database: &Path,
) -> Result<bool, PersistenceError> {
    let marker = maintenance(database);
    if marker.is_some_and(|marker| marker.owed_runs.is_some()) {
        return Ok(true);
    }
    let kept = marker.map_or(0, |marker| marker.kept_bytes);
    let (in_use, free) = page_bytes(connection)?;
    let owed = free >= REBUILD_AT || in_use >= kept.saturating_add(GROWTH_AT);
    if owed {
        write_marker(database, kept, Some(0))?;
    }
    Ok(owed)
}

fn file_bytes(database: &Path) -> u64 {
    let wal = PathBuf::from(format!("{}-wal", database.display()));
    [database, wal.as_path()]
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .sum()
}

/// Clears what `database` keeps and nothing reads again, its catalog's raw responses under
/// `artifact_root` included, then rebuilds the file when enough of it is free and `free_space` says
/// the disk has room. `step` hears the bytes the run works on, at once (the file and its log, before
/// the file is opened) and then about every second, and may stop it. Nothing else may have the file
/// open.
pub fn slim(
    database: &Path,
    artifact_root: &Path,
    free_space: &dyn Fn(&Path) -> std::io::Result<u64>,
    step: &mut dyn FnMut(u64) -> ControlFlow<()>,
) -> Result<Report, PersistenceError> {
    let earlier = maintenance(database);
    let runs = earlier.and_then(|marker| marker.owed_runs);
    let kept_before = earlier.map_or(0, |marker| marker.kept_bytes);
    let mut report = Report { before_bytes: file_bytes(database), ..Report::default() };
    // Opening the file reads its log, and measuring the text index reads the whole index, about a
    // second for each few hundred megabytes: the caller hears first.
    if step(report.before_bytes).is_break() {
        report.stopped = true;
        report.after_bytes = report.before_bytes;
        return Ok(report);
    }
    // An owed run counts before anything that may fail, so one that always fails is not retried
    // at every start.
    if let Some(runs) = runs {
        write_marker(database, kept_before, Some(runs.saturating_add(1)))?;
    }
    let mut connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    // The run needs the file alone; a process still leaving it is waited for.
    connection.busy_timeout(EXCLUSIVE_WAIT)?;
    connection.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let (in_use, free) = page_bytes(&connection)?;
    let rewrite = text_index::measure(&connection)?.filter(text_index::owed);
    let replaced = catalog::replaced_versions(&connection)?;
    if runs.is_none() && (rewrite.is_some() || replaced > 0 || free >= REBUILD_AT) {
        write_marker(database, kept_before, Some(1))?;
    }
    if let Some((before_bytes, _)) = rewrite {
        let (done, back) = run_heard(connection, text_index::OPTIMIZE, &mut || step(in_use))?;
        connection = back;
        if done {
            let after_bytes = text_index::measure(&connection)?.map_or(0, |(index, _)| index);
            report.text_index = Some(TextIndex { before_bytes, after_bytes });
        } else {
            report.stopped = true;
        }
    }
    if !report.stopped && replaced > 0 {
        let tx = connection.transaction()?;
        let (dropped, files) = catalog::drop_replaced(&tx, artifact_root, None)?;
        tx.commit()?;
        crate::artifact_gc::remove_files(files);
        report.catalog_versions = dropped;
    }
    if !report.stopped {
        let (rebuilt, back) = rebuild::rebuild(connection, database, free_space, step)?;
        connection = back;
        match rebuilt {
            None => report.stopped = true,
            Some(rebuilt) => report.rebuild = rebuilt,
        }
    }
    connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get::<_, i64>(0))?;
    let (kept, _) = page_bytes(&connection)?;
    drop(connection);
    report.after_bytes = file_bytes(database);
    match report.rebuild {
        // A stopped run stays owed, with the run it counted.
        _ if report.stopped => {}
        // Short of room is no failure: it does not count against the runs.
        Rebuild::NoRoom { .. } => write_marker(database, kept, Some(0))?,
        _ => write_marker(database, kept, None)?,
    }
    Ok(report)
}
