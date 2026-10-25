use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

/// Internal stage measurements for one search. Counters are cumulative across
/// the stages that run many times per search (probe, open, read, verify) and
/// snapshot-style for whole-search stages (catalog, merge).
///
/// These numbers are diagnostics for benchmarks and tests only. They are never
/// projected into `FsSearchTextResult`, the IPC payload, or a model request.
#[derive(Debug, Default)]
pub(crate) struct StageDiagnostics {
    pub(crate) catalog_micros: AtomicU64,
    pub(crate) buffer_probe_micros: AtomicU64,
    pub(crate) queue_wait_micros: AtomicU64,
    pub(crate) open_micros: AtomicU64,
    pub(crate) read_micros: AtomicU64,
    pub(crate) verify_micros: AtomicU64,
    pub(crate) commit_micros: AtomicU64,
    pub(crate) catalog_candidates: AtomicU64,
    pub(crate) buffer_probes: AtomicU64,
    pub(crate) buffer_snapshots: AtomicU64,
    pub(crate) open_calls: AtomicU64,
    pub(crate) read_calls: AtomicU64,
    pub(crate) scanned_bytes: AtomicU64,
    pub(crate) verify_calls: AtomicU64,
    pub(crate) verify_bytes: AtomicU64,
    /// Matched lines handed to the sink, their bytes, and the hits inside them.
    /// These separate "the matcher found something" from "the hit was projected",
    /// so a long line with many hits can be attributed instead of guessed.
    pub(crate) matched_lines: AtomicU64,
    pub(crate) matched_line_bytes: AtomicU64,
    pub(crate) matched_hits: AtomicU64,
    /// Bytes re-walked to turn a hit's byte offset into a character offset.
    pub(crate) char_offset_bytes: AtomicU64,

    pub(crate) matched_files: AtomicU64,
    pub(crate) window_high_water: AtomicU64,
    pub(crate) queue_high_water: AtomicU64,
    pub(crate) pool_active_high_water: AtomicU64,
}

impl StageDiagnostics {
    pub(crate) fn snapshot(&self) -> StageDiagnosticsSnapshot {
        StageDiagnosticsSnapshot {
            catalog_micros: self.catalog_micros.load(Ordering::Relaxed),
            buffer_probe_micros: self.buffer_probe_micros.load(Ordering::Relaxed),
            queue_wait_micros: self.queue_wait_micros.load(Ordering::Relaxed),
            open_micros: self.open_micros.load(Ordering::Relaxed),
            read_micros: self.read_micros.load(Ordering::Relaxed),
            verify_micros: self.verify_micros.load(Ordering::Relaxed),
            commit_micros: self.commit_micros.load(Ordering::Relaxed),
            catalog_candidates: self.catalog_candidates.load(Ordering::Relaxed),
            buffer_probes: self.buffer_probes.load(Ordering::Relaxed),
            buffer_snapshots: self.buffer_snapshots.load(Ordering::Relaxed),
            open_calls: self.open_calls.load(Ordering::Relaxed),
            read_calls: self.read_calls.load(Ordering::Relaxed),
            scanned_bytes: self.scanned_bytes.load(Ordering::Relaxed),
            verify_calls: self.verify_calls.load(Ordering::Relaxed),
            verify_bytes: self.verify_bytes.load(Ordering::Relaxed),
            matched_lines: self.matched_lines.load(Ordering::Relaxed),
            matched_line_bytes: self.matched_line_bytes.load(Ordering::Relaxed),
            matched_hits: self.matched_hits.load(Ordering::Relaxed),
            char_offset_bytes: self.char_offset_bytes.load(Ordering::Relaxed),

            matched_files: self.matched_files.load(Ordering::Relaxed),
            window_high_water: self.window_high_water.load(Ordering::Relaxed),
            queue_high_water: self.queue_high_water.load(Ordering::Relaxed),
            pool_active_high_water: self.pool_active_high_water.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StageDiagnosticsSnapshot {
    pub catalog_micros: u64,
    pub buffer_probe_micros: u64,
    pub queue_wait_micros: u64,
    pub open_micros: u64,
    pub read_micros: u64,
    pub verify_micros: u64,
    pub commit_micros: u64,
    pub catalog_candidates: u64,
    pub buffer_probes: u64,
    pub buffer_snapshots: u64,
    pub open_calls: u64,
    pub read_calls: u64,
    pub scanned_bytes: u64,
    pub verify_calls: u64,
    pub verify_bytes: u64,
    /// Matched lines, their bytes, and the hits found inside them.
    pub matched_lines: u64,
    pub matched_line_bytes: u64,
    pub matched_hits: u64,
    /// Bytes re-walked to convert hit byte offsets into character offsets.
    pub char_offset_bytes: u64,

    pub matched_files: u64,
    pub window_high_water: u64,
    pub queue_high_water: u64,
    pub pool_active_high_water: u64,
}

/// Times one stage of one file and adds the result to the search counters.
pub(crate) struct StageTimer<'a> {
    counter: &'a AtomicU64,
    started: Instant,
}

impl<'a> StageTimer<'a> {
    pub(crate) fn start(counter: &'a AtomicU64) -> Self {
        Self { counter, started: Instant::now() }
    }
}

impl Drop for StageTimer<'_> {
    fn drop(&mut self) {
        let micros = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.counter.fetch_add(micros, Ordering::Relaxed);
    }
}

pub(crate) fn observe_high_water(counter: &AtomicU64, value: usize) {
    let value = u64::try_from(value).unwrap_or(u64::MAX);
    counter.fetch_max(value, Ordering::Relaxed);
}

/// Cloneable handle passed into worker jobs.
pub(crate) type StageDiagnosticsHandle = Arc<StageDiagnostics>;
