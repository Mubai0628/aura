//! What older versions wrote and nothing reads again, cleared once before Core serves: the
//! outbox copy of every event, the whole attempt record in each `model.transport` event, every
//! context snapshot but the two a conversation reads (`context::keep_two`) with their section
//! texts, the raw streams of calls that completed, and the streamed deltas of replies that
//! completed. The file is then rebuilt (`VACUUM`) so the space goes back to the disk.
//!
//! The launcher copies the store after the run and before Core first opens it, so going back to
//! the version it replaces puts back a slimmed store. The run therefore changes no structure: a
//! store that owes this build only indexes is worked on as it is, and Core adds them when it opens
//! it; one that owes tables is left for the first start after Core has migrated it.
//!
//! Each part is deleted in batches, each in its own transaction, so a run that is ended at any
//! point leaves a store that opens. A marker beside the store (`MARKER`) says a run is owed: it is
//! written before the first change and removed when the run ends, and the next run continues from
//! what is left. Without room for the rebuild only the deletion is done and the rebuild stays owed.
//! Every older version also writes an outbox copy of each event, so an empty outbox, no completed
//! reply with deltas left and no marker mean there is nothing to clear. Nothing else may have the
//! store open while this runs; the launcher runs it before the desktop starts.
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use aura_persistence_core::{
    DatabaseConnection, StoreResult,
    schema::{self, OpenCheck},
};
use serde::{Deserialize, Serialize};

use crate::schema_migration::Pending;

mod command;
mod compact;
mod delete;
mod snapshots;
mod streams;

pub use command::command;

/// Beside the store while a run is owed; see the module documentation.
pub const MARKER: &str = "store-slimming.json";
/// How long a run waits for another connection to let go of the store.
const EXCLUSIVE_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// How far a run got, reported as it goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    /// About this many bytes deleted so far, out of about the total.
    Deleting { done: u64, total: u64 },
    /// The file is being rebuilt around the bytes it keeps; reported again while it lasts.
    Rebuilding { kept: u64 },
}

/// What a run did.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Report {
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub outbox_rows: u64,
    pub transport_events: u64,
    pub snapshots: u64,
    pub raw_streams: u64,
    /// Completed replies whose streamed deltas went.
    pub streamed_replies: u64,
    pub rebuild: Rebuild,
    /// The caller asked the run to stop; the rest is owed to the next run.
    pub stopped: bool,
    /// The store owes this build tables first; the run is owed to the start after Core adds them.
    pub waits_for_migration: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Rebuild {
    /// Nothing was cleared, or too little to be worth rewriting the file.
    #[default]
    NotNeeded,
    Done,
    /// The disk lacks room for the rebuild; it is owed to a later run.
    NoRoom {
        needed: u64,
        free: u64,
    },
}

/// What the marker says is left.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Delete,
    Rebuild,
}

/// The marker: what is left and how many runs started on it. A run that never ended (the process
/// was ended, or it failed) still counts, so the launcher can stop retrying one that always fails.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Owed {
    pub schema_version: u32,
    pub stage: Stage,
    pub runs: u32,
}

/// The run owed for the store in `root`, if one is.
pub fn owed(root: &Path) -> Option<Owed> {
    let owed: Owed = serde_json::from_slice(&std::fs::read(root.join(MARKER)).ok()?).ok()?;
    (owed.schema_version == 1).then_some(owed)
}

/// Notes a run as owed when the store Core opened still holds older versions' copies. A version's
/// first start can be run by the launcher of the version it replaces (an update applied from
/// inside Aura restarts the launcher that was running), and one older than the slimming never
/// runs it; the marker has the next start, by the new launcher, run it.
pub(crate) fn note_older_copies(root: &Path, db: &rusqlite::Connection) -> StoreResult<()> {
    if owed(root).is_none() && delete::outbox_rows(db)? > 0 {
        write_owed(root, Stage::Delete, 0)?;
    }
    Ok(())
}

fn write_owed(root: &Path, stage: Stage, runs: u32) -> StoreResult<()> {
    let owed = Owed { schema_version: 1, stage, runs };
    std::fs::write(root.join(MARKER), format!("{}\n", serde_json::to_string(&owed)?))?;
    Ok(())
}

fn remove_owed(root: &Path) -> StoreResult<()> {
    match std::fs::remove_file(root.join(MARKER)) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// Clears what the store in `root` (the V2 directory) keeps and nothing reads again, then rebuilds
/// the file when enough was cleared and `free_space` says the disk has room. `step` hears how far
/// the run got and may stop it between two batches or during the rebuild.
pub fn slim(
    root: &Path,
    free_space: &dyn Fn(&Path) -> std::io::Result<u64>,
    step: &mut dyn FnMut(Step) -> ControlFlow<()>,
) -> StoreResult<Report> {
    crate::check_namespace(root)?;
    let database = root.join(crate::DATABASE);
    let earlier = owed(root);
    let pending = crate::schema_migration::pending(&database)?;
    if pending == Pending::Tables {
        // Waiting for Core is no failure: it does not count against the runs.
        write_owed(root, Stage::Delete, earlier.map_or(0, |owed| owed.runs))?;
        let bytes = file_bytes(&database);
        return Ok(Report {
            before_bytes: bytes,
            after_bytes: bytes,
            waits_for_migration: true,
            ..Report::default()
        });
    }
    // A run that ends without a word is an exit that was not normal: the next open owes the full
    // check, as after Core's own. The marker of a normal exit goes back only when this run ends.
    let clean_exit = root.join(crate::CLEAN_EXIT);
    let had_clean_exit = clean_exit.exists() && std::fs::remove_file(&clean_exit).is_ok();
    let mut store = match pending {
        Pending::Nothing => schema::open_checked(&database, crate::MODULES, OpenCheck::Structure)?,
        // The plan compared the whole structure with the predecessor's before it said so.
        _ => DatabaseConnection::open_existing(&database)?,
    };
    // The run needs the store alone; a process still leaving it (an exit's checkpoint, a check
    // being stopped) is waited for instead of failing the run.
    store.connection().busy_timeout(EXCLUSIVE_WAIT)?;
    store.connection().pragma_update(None, "locking_mode", "EXCLUSIVE")?;
    store.connection().pragma_update(None, "cache_size", crate::WRITER_CACHE_KIB)?;
    let mut report = Report { before_bytes: file_bytes(&database), ..Report::default() };
    // An older version run since (going back to it) writes outbox copies again, whatever is owed.
    let deleting = earlier.is_some_and(|owed| owed.stage == Stage::Delete)
        || delete::outbox_rows(store.connection())? > 0
        || streams::count(store.connection())? > 0;
    if deleting || earlier.is_some() {
        let runs = earlier.map_or(0, |owed| owed.runs).saturating_add(1);
        write_owed(root, if deleting { Stage::Delete } else { Stage::Rebuild }, runs)?;
        if deleting {
            let mut progress = Progress::new(survey(store.connection())?, step);
            delete::outbox(&mut store, &mut report, &mut progress)?;
            delete::transport_events(&mut store, &mut report, &mut progress)?;
            snapshots::clear(&mut store, &mut report, &mut progress)?;
            delete::raw_streams(&mut store, &mut report, &mut progress)?;
            // After the outbox: each event deleted checks the outbox copies that may point at it.
            streams::clear(&mut store, &mut report, &mut progress)?;
            report.stopped = progress.stopped;
            if !report.stopped {
                write_owed(root, Stage::Rebuild, runs)?;
            }
        }
        if !report.stopped {
            let (rebuild, reopened) = compact::rebuild(store, &database, free_space, step)?;
            store = reopened;
            match rebuild {
                None => report.stopped = true,
                Some(rebuild @ Rebuild::NoRoom { .. }) => {
                    report.rebuild = rebuild;
                    // Short of room is no failure: it does not count against the runs.
                    write_owed(root, Stage::Rebuild, 0)?;
                }
                Some(rebuild) => {
                    report.rebuild = rebuild;
                    remove_owed(root)?;
                }
            }
        }
    }
    store
        .connection()
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get::<_, i64>(0))?;
    drop(store);
    report.after_bytes = file_bytes(&database);
    if had_clean_exit {
        crate::write_clean_exit(root)?;
    }
    Ok(report)
}

fn file_bytes(database: &Path) -> u64 {
    let wal = PathBuf::from(format!("{}-wal", database.display()));
    [database, wal.as_path()]
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .sum()
}

/// What a run expects to delete. Counting the bytes exactly would read as much as deleting them,
/// so each part is counted in items and an item is taken at the size measured on a 7.2 GB store an
/// older version wrote: enough to move a progress bar evenly.
#[derive(Clone, Copy, Debug, Default)]
struct Survey {
    outbox_rows: u64,
    transport_events: u64,
    snapshots: u64,
    raw_streams: u64,
    streamed_deltas: u64,
}

const OUTBOX_ROW_BYTES: u64 = 1_200;
/// An older version wrote about nine `model.transport` events for each call it made.
const TRANSPORT_EVENTS_PER_CALL: u64 = 9;
const TRANSPORT_EVENT_BYTES: u64 = 88_000;
const SNAPSHOT_BYTES: u64 = 1_900_000;
const RAW_STREAM_BYTES: u64 = 500_000;
/// A streamed delta with its three events and its derived rows.
const STREAMED_DELTA_BYTES: u64 = 1_600;

fn survey(db: &rusqlite::Connection) -> StoreResult<Survey> {
    Ok(Survey {
        outbox_rows: delete::outbox_rows(db)?,
        transport_events: delete::calls(db)? * TRANSPORT_EVENTS_PER_CALL,
        snapshots: snapshots::count(db)?,
        raw_streams: delete::streams(db)?,
        streamed_deltas: streams::count(db)?,
    })
}

/// Reports the estimated bytes deleted as the parts go, and remembers whether the caller asked
/// the run to stop.
struct Progress<'a> {
    survey: Survey,
    total: u64,
    done: u64,
    stopped: bool,
    step: &'a mut dyn FnMut(Step) -> ControlFlow<()>,
}

impl<'a> Progress<'a> {
    fn new(survey: Survey, step: &'a mut dyn FnMut(Step) -> ControlFlow<()>) -> Self {
        let total = survey.outbox_rows * OUTBOX_ROW_BYTES
            + survey.transport_events * TRANSPORT_EVENT_BYTES
            + survey.snapshots * SNAPSHOT_BYTES
            + survey.raw_streams * RAW_STREAM_BYTES
            + survey.streamed_deltas * STREAMED_DELTA_BYTES;
        Self { survey, total, done: 0, stopped: false, step }
    }

    fn advance(&mut self, bytes: u64) {
        self.done = self.done.saturating_add(bytes).min(self.total);
        if (self.step)(Step::Deleting { done: self.done, total: self.total }).is_break() {
            self.stopped = true;
        }
    }

    fn outbox(&mut self, rows: u64) {
        self.advance(rows * OUTBOX_ROW_BYTES);
    }

    /// The events are read in ranges of their sequence, `part` of `whole`.
    fn transport_events(&mut self, part: u64, whole: u64) {
        let bytes = self.survey.transport_events * TRANSPORT_EVENT_BYTES;
        self.advance(
            u64::try_from(u128::from(bytes) * u128::from(part) / u128::from(whole.max(1)))
                .unwrap_or(bytes),
        );
    }

    fn snapshots(&mut self, snapshots: u64) {
        self.advance(snapshots * SNAPSHOT_BYTES);
    }

    fn raw_streams(&mut self, streams: u64) {
        self.advance(streams * RAW_STREAM_BYTES);
    }

    /// Each delta went as three events.
    fn streams(&mut self, events: u64) {
        self.advance(events / 3 * STREAMED_DELTA_BYTES);
    }
}
