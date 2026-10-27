//! A catalog's index build: whether it is still running, how far it got, which folders it could
//! not reach, and what waits for it.
//!
//! The walk runs on the workspace's own thread, outside in, so a build that has to stop early
//! still holds the top of the tree. Queries wait for it for a bounded time; listings read the disk
//! meanwhile; index updates that writes ask for are kept and applied when it ends, so a write
//! never waits for the index and never races the walk.
use std::{
    cell::RefCell,
    collections::BTreeSet,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use aura_domain_common::WorkspacePath;

use super::delta::CatalogChange;
use crate::WorkspaceError;

/// How long a query waits for a build before it answers that the index is still being built.
pub(crate) const QUERY_WAIT: Duration = Duration::from_secs(30);
/// Entries an index holds at most; a folder beyond it is recorded as itself and read from disk.
const MAX_ENTRIES: u64 = 200_000;
/// Memory an index may use at most, or a quarter of the memory free when it starts if less.
const MAX_INDEX_BYTES: u64 = 400 * 1024 * 1024;
/// What one entry costs in memory, counted generously.
const BYTES_PER_ENTRY: u64 = 2 * 1024;
/// How long a build runs at full speed before it paces itself by the machine's load.
const FULL_SPEED: Duration = Duration::from_millis(2_500);
/// Folders listed between two looks at the machine's load.
const LOAD_CHECK_EVERY: u64 = 64;
/// Above this processor busy share a build yields, a short while at a time.
const BUSY_PERCENT: u8 = 70;
const YIELD: Duration = Duration::from_millis(50);
const MAX_YIELD: Duration = Duration::from_secs(2);
/// Below this much free memory, or a tenth of all memory, a build goes no deeper.
const MIN_FREE_MEMORY: u64 = 512 * 1024 * 1024;
/// A walk that has listed fewer entries than this never stops for memory: a small workspace's
/// index costs little, and stopping it would only leave its lookups to read the disk.
const MEMORY_GUARD_FROM: u64 = 50_000;
/// How often an index left incomplete for want of memory looks whether memory has come back.
const RETRY_EVERY: Duration = Duration::from_secs(30);
/// Folders watched at most: each costs a handle and a 16 KB buffer, a quarter of the budget.
pub(crate) const MAX_WATCHED_DIRECTORIES: usize = (MAX_INDEX_BYTES / 4 / (16 * 1024)) as usize;
/// How often a running build reports its progress.
const PROGRESS_EVERY: Duration = Duration::from_millis(500);
/// Whole walks running at once across all workspaces; the rest wait their turn, so a start with
/// many workspaces does not read them all at the same moment.
const MAX_CONCURRENT_WALKS: usize = 2;
static RUNNING_WALKS: Mutex<usize> = Mutex::new(0);
static WALK_TURN: Condvar = Condvar::new();

/// One whole walk's turn, given back when it ends.
struct Turn;

impl Turn {
    /// Waits for a turn; a workspace closing meanwhile takes one at once, and its walk stops at
    /// its first folder.
    fn take(stop: &std::sync::atomic::AtomicBool) -> Self {
        let Ok(mut running) = RUNNING_WALKS.lock() else { return Self };
        while *running >= MAX_CONCURRENT_WALKS && !stop.load(Ordering::Acquire) {
            match WALK_TURN.wait_timeout(running, Duration::from_millis(50)) {
                Ok((next, _)) => running = next,
                Err(_) => return Self,
            }
        }
        *running += 1;
        Self
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        if let Ok(mut running) = RUNNING_WALKS.lock() {
            *running = running.saturating_sub(1);
        }
        WALK_TURN.notify_one();
    }
}

/// Where a catalog's build stands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogBuildPhase {
    Building,
    Ready,
    /// The build stopped before the whole tree: at the entry cap, or with memory low.
    Incomplete,
    Failed {
        reason: String,
    },
}

/// A build's phase and progress, as the file views show them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogBuildStatus {
    pub phase: CatalogBuildPhase,
    /// Entries the current or last walk listed.
    pub listed_entries: u64,
    /// Folders the last walk recorded but did not enter.
    pub unindexed_directories: u64,
    /// The last walk stopped going deeper because memory ran low, not at the entry cap.
    pub memory_low: bool,
    /// A whole walk runs now; an index that was usable answers meanwhile.
    pub walking: bool,
}

pub(crate) struct CatalogBuild {
    state: Mutex<BuildState>,
    settled: Condvar,
    progress: Arc<BuildProgress>,
    deferred: Mutex<Vec<(WorkspacePath, CatalogChange)>>,
    report: Mutex<Option<super::CatalogReconcileReport>>,
    /// When an index stopped for memory last looked whether memory has come back.
    last_retry: Mutex<Option<Instant>>,
}

struct BuildState {
    phase: CatalogBuildPhase,
    /// A whole walk is running: the first build, the check of a saved index, or a rebuild.
    walking: bool,
}

/// What one walk reached, shared between the walk's thread and the readers.
#[derive(Default)]
pub(crate) struct BuildProgress {
    listed: AtomicU64,
    unindexed: Mutex<BTreeSet<WorkspacePath>>,
    /// The walk stopped going deeper because memory ran low.
    memory_low: AtomicBool,
    /// A smaller entry cap than the memory budget gives, for a test; 0 for none.
    cap: AtomicU64,
    /// A lower entry count than [`MEMORY_GUARD_FROM`] from which memory is looked at, for a
    /// test; 0 for none.
    guard_from: AtomicU64,
}

impl BuildProgress {
    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn cap_at(&self, entries: u64) {
        self.cap.store(entries, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn guard_memory_from(&self, entries: u64) {
        self.guard_from.store(entries, Ordering::Release);
    }

    fn memory_guard_from(&self) -> u64 {
        match self.guard_from.load(Ordering::Acquire) {
            0 => MEMORY_GUARD_FROM,
            entries => entries,
        }
    }
}

#[cfg(test)]
thread_local! {
    static PRETEND_MEMORY_LOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Makes this thread's walks and retry checks see memory as low, or not, for a test.
#[cfg(test)]
pub(crate) fn pretend_memory_low(low: bool) {
    PRETEND_MEMORY_LOW.with(|pretend| pretend.set(low));
}

/// Whether free memory is below where a walk goes no deeper: a floor, or a tenth of all memory.
fn memory_low_now() -> bool {
    #[cfg(test)]
    if PRETEND_MEMORY_LOW.with(std::cell::Cell::get) {
        return true;
    }
    aura_windows_platform::physical_memory()
        .is_ok_and(|memory| memory.available < MIN_FREE_MEMORY.max(memory.total / 10))
}

/// Whether memory has come back well above where a walk stopped: twice that much free.
fn memory_recovered() -> bool {
    #[cfg(test)]
    if PRETEND_MEMORY_LOW.with(std::cell::Cell::get) {
        return false;
    }
    aura_windows_platform::physical_memory()
        .is_ok_and(|memory| memory.available >= 2 * MIN_FREE_MEMORY.max(memory.total / 10))
}

impl CatalogBuild {
    /// A catalog whose index is usable as loaded.
    pub(crate) fn ready() -> Self {
        Self {
            state: Mutex::new(BuildState { phase: CatalogBuildPhase::Ready, walking: false }),
            settled: Condvar::new(),
            progress: Arc::default(),
            deferred: Mutex::new(Vec::new()),
            report: Mutex::new(None),
            last_retry: Mutex::new(None),
        }
    }

    fn state(&self) -> Result<std::sync::MutexGuard<'_, BuildState>, WorkspaceError> {
        self.state.lock().map_err(|_| WorkspaceError::LockPoisoned("catalog build"))
    }

    pub(crate) fn progress(&self) -> Arc<BuildProgress> {
        Arc::clone(&self.progress)
    }

    /// What the last finished walk found, compared with the index it started from.
    pub(crate) fn last_report(&self) -> Option<super::CatalogReconcileReport> {
        self.report.lock().ok().and_then(|report| *report)
    }

    pub(crate) fn record(&self, report: super::CatalogReconcileReport) {
        if let Ok(mut last) = self.report.lock() {
            *last = Some(report);
        }
    }

    /// A whole walk starts: the first build of an index that holds nothing yet when `fresh`, a
    /// check or rebuild of a usable one otherwise.
    pub(crate) fn begin(&self, fresh: bool) -> Result<(), WorkspaceError> {
        let mut state = self.state()?;
        state.walking = true;
        if fresh {
            state.phase = CatalogBuildPhase::Building;
        }
        Ok(())
    }

    /// Marks a requested walk as running, unless one is; says whether it did. A failed index
    /// holds nothing usable and builds anew; a usable one answers meanwhile.
    pub(crate) fn begin_requested(&self) -> Result<bool, WorkspaceError> {
        let mut state = self.state()?;
        if state.walking {
            return Ok(false);
        }
        state.walking = true;
        if matches!(state.phase, CatalogBuildPhase::Failed { .. }) {
            state.phase = CatalogBuildPhase::Building;
        }
        Ok(true)
    }

    /// Whether an index left incomplete for want of memory is to be walked again now: memory
    /// has come back, no walk runs, and it last looked a while ago.
    pub(crate) fn retry_due(&self) -> bool {
        self.retry_due_at(Instant::now(), RETRY_EVERY)
    }

    pub(crate) fn retry_due_at(&self, now: Instant, every: Duration) -> bool {
        let Ok(state) = self.state() else { return false };
        if state.walking
            || state.phase != CatalogBuildPhase::Incomplete
            || !self.progress.memory_low.load(Ordering::Acquire)
        {
            return false;
        }
        drop(state);
        let Ok(mut last) = self.last_retry.lock() else { return false };
        if last.is_some_and(|at| now.saturating_duration_since(at) < every) {
            return false;
        }
        *last = Some(now);
        memory_recovered()
    }

    /// Ends a whole walk. A first build that failed is a failure; any other walk that failed
    /// leaves the index as it was. Otherwise the phase follows from what the walk could not enter.
    pub(crate) fn finish(&self, failure: Option<String>) -> Result<(), WorkspaceError> {
        let unindexed = self.unindexed_count()?;
        let mut state = self.state()?;
        let fresh = state.phase == CatalogBuildPhase::Building;
        state.walking = false;
        state.phase = match failure {
            Some(reason) if fresh => CatalogBuildPhase::Failed { reason },
            Some(_) => state.phase.clone(),
            None if unindexed > 0 => CatalogBuildPhase::Incomplete,
            None => CatalogBuildPhase::Ready,
        };
        self.settled.notify_all();
        Ok(())
    }

    pub(crate) fn status(&self) -> Result<CatalogBuildStatus, WorkspaceError> {
        let (phase, walking) = {
            let state = self.state()?;
            (state.phase.clone(), state.walking)
        };
        Ok(CatalogBuildStatus {
            phase,
            listed_entries: self.progress.listed.load(Ordering::Acquire),
            unindexed_directories: self.unindexed_count()?,
            memory_low: self.progress.memory_low.load(Ordering::Acquire),
            walking,
        })
    }

    pub(crate) fn is_building(&self) -> Result<bool, WorkspaceError> {
        Ok(self.state()?.phase == CatalogBuildPhase::Building)
    }

    /// Waits up to `limit` for a first build to end; one still running then is answered with
    /// how far it got. A check of a usable index is not waited for.
    pub(crate) fn wait(&self, limit: Duration) -> Result<(), WorkspaceError> {
        self.wait_while(limit, |state| state.phase == CatalogBuildPhase::Building)
    }

    /// Waits up to `limit` for any whole walk to end.
    pub(crate) fn wait_walked(&self, limit: Duration) -> Result<(), WorkspaceError> {
        self.wait_while(limit, |state| state.walking)
    }

    fn wait_while(
        &self,
        limit: Duration,
        running: impl Fn(&BuildState) -> bool,
    ) -> Result<(), WorkspaceError> {
        let state = self.state()?;
        let (state, _) = self
            .settled
            .wait_timeout_while(state, limit, |state| running(state))
            .map_err(|_| WorkspaceError::LockPoisoned("catalog build"))?;
        if running(&state) {
            return Err(WorkspaceError::IndexBuilding {
                listed: self.progress.listed.load(Ordering::Acquire),
            });
        }
        Ok(())
    }

    /// Keeps an index update for after the running build; `false` when no build runs.
    pub(crate) fn defer(
        &self,
        changes: &[(WorkspacePath, CatalogChange)],
    ) -> Result<bool, WorkspaceError> {
        let state = self.state()?;
        if !state.walking {
            return Ok(false);
        }
        self.deferred
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("catalog build"))?
            .extend_from_slice(changes);
        Ok(true)
    }

    pub(crate) fn take_deferred(
        &self,
    ) -> Result<Vec<(WorkspacePath, CatalogChange)>, WorkspaceError> {
        Ok(std::mem::take(
            &mut *self
                .deferred
                .lock()
                .map_err(|_| WorkspaceError::LockPoisoned("catalog build"))?,
        ))
    }

    /// Whether the last walk recorded `directory`, or a folder above it, without entering it.
    pub(crate) fn unindexed(&self, directory: &WorkspacePath) -> Result<bool, WorkspaceError> {
        let unindexed = self
            .progress
            .unindexed
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("catalog build"))?;
        if unindexed.is_empty() {
            return Ok(false);
        }
        let mut current = Some(directory.as_str());
        while let Some(path) = current {
            if WorkspacePath::parse(path).is_ok_and(|path| unindexed.contains(&path)) {
                return Ok(true);
            }
            current = path.rsplit_once('/').map(|(parent, _)| parent);
        }
        Ok(false)
    }

    /// The unindexed folders a lookup of `directory` would have entered: those under it (or
    /// directly in it, when not `recursive`), or the one it is itself inside.
    pub(crate) fn unindexed_within(
        &self,
        directory: &WorkspacePath,
        recursive: bool,
    ) -> Result<Vec<WorkspacePath>, WorkspaceError> {
        if self.unindexed(directory)? {
            let unindexed = self
                .progress
                .unindexed
                .lock()
                .map_err(|_| WorkspaceError::LockPoisoned("catalog build"))?;
            let mut current = Some(directory.as_str());
            while let Some(path) = current {
                if let Ok(path) = WorkspacePath::parse(path)
                    && unindexed.contains(&path)
                {
                    return Ok(vec![path]);
                }
                current = path.rsplit_once('/').map(|(parent, _)| parent);
            }
            return Ok(vec![directory.clone()]);
        }
        let unindexed = self
            .progress
            .unindexed
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("catalog build"))?;
        let prefix = if directory.is_root() { String::new() } else { format!("{directory}/") };
        Ok(unindexed
            .iter()
            .filter(|path| {
                let Some(rest) = path.as_str().strip_prefix(&prefix) else { return false };
                !rest.is_empty() && (recursive || !rest.contains('/'))
            })
            .cloned()
            .collect())
    }

    fn unindexed_count(&self) -> Result<u64, WorkspaceError> {
        Ok(self
            .progress
            .unindexed
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("catalog build"))?
            .len() as u64)
    }
}

/// The walk running on this thread, if any. A listing outside a build walk (a bounded update)
/// is neither counted nor capped.
struct Walk {
    progress: Arc<BuildProgress>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    started: Instant,
    directories: u64,
    max_entries: u64,
    cpu: Option<aura_windows_platform::CpuLoad>,
    stopped: bool,
    /// Memory has been looked at since the walk passed its memory guard.
    memory_checked: bool,
    reported: Instant,
    report: Box<dyn FnMut()>,
}

thread_local! {
    static WALK: RefCell<Option<Walk>> = const { RefCell::new(None) };
}

/// The stop flag of the walk running on this thread, if any: a whole index write the walk makes
/// stops between its jobs when the workspace closes.
pub(crate) fn walk_stop() -> Option<Arc<std::sync::atomic::AtomicBool>> {
    WALK.with(|current| current.borrow().as_ref().map(|walk| Arc::clone(&walk.stop)))
}

/// Runs `walk` as a build walk: its listings are counted toward `progress`, it is capped and
/// paced, and `report` is called with progress now and then.
pub(crate) fn with_walk<R>(
    progress: Arc<BuildProgress>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    report: impl FnMut() + 'static,
    walk: impl FnOnce() -> R,
) -> R {
    let _turn = Turn::take(&stop);
    // Each walk is counted and capped on its own.
    progress.listed.store(0, Ordering::Release);
    progress.memory_low.store(false, Ordering::Release);
    if let Ok(mut unindexed) = progress.unindexed.lock() {
        unindexed.clear();
    }
    let budget = aura_windows_platform::physical_memory()
        .map_or(MAX_INDEX_BYTES, |memory| (memory.available / 4).min(MAX_INDEX_BYTES));
    let max_entries = match progress.cap.load(Ordering::Acquire) {
        0 => (budget / BYTES_PER_ENTRY).min(MAX_ENTRIES),
        cap => cap,
    };
    let now = Instant::now();
    WALK.with(|current| {
        *current.borrow_mut() = Some(Walk {
            progress,
            stop,
            started: now,
            directories: 0,
            max_entries,
            cpu: aura_windows_platform::CpuLoad::start().ok(),
            stopped: false,
            memory_checked: false,
            reported: now,
            report: Box::new(report),
        });
    });
    let result = walk();
    WALK.with(|current| current.borrow_mut().take());
    result
}

/// Counts one folder's listing in the running build walk and paces the walk: after the first
/// stretch it yields while the processors are busy, and once it has listed many entries it stops
/// going deeper when memory runs low.
pub(crate) fn note_listed(children: usize) {
    WALK.with(|current| {
        let mut current = current.borrow_mut();
        let Some(walk) = current.as_mut() else { return };
        let listed =
            walk.progress.listed.fetch_add(children as u64, Ordering::AcqRel) + children as u64;
        walk.directories += 1;
        if walk.reported.elapsed() >= PROGRESS_EVERY {
            walk.reported = Instant::now();
            (walk.report)();
        }
        let periodic = walk.directories % LOAD_CHECK_EVERY == 0;
        // Past the guard, memory is looked at right away and then with the machine's load.
        if listed >= walk.progress.memory_guard_from()
            && (periodic || !walk.memory_checked)
            && !walk.stopped
        {
            walk.memory_checked = true;
            if memory_low_now() {
                walk.stopped = true;
                walk.progress.memory_low.store(true, Ordering::Release);
            }
        }
        if !periodic {
            return;
        }
        if walk.started.elapsed() < FULL_SPEED {
            return;
        }
        let Some(cpu) = walk.cpu.as_mut() else { return };
        let mut yielded = Duration::ZERO;
        while yielded < MAX_YIELD && cpu.busy_percent().is_ok_and(|busy| busy > BUSY_PERCENT) {
            std::thread::sleep(YIELD);
            yielded += YIELD;
        }
    });
}

/// Whether the running build walk may enter `directory`. Past the entry cap, or with memory
/// low, it may not, and the folder is recorded as not indexed.
pub(crate) fn may_enter(directory: &WorkspacePath) -> bool {
    WALK.with(|current| {
        let current = current.borrow();
        let Some(walk) = current.as_ref() else { return true };
        if !walk.stopped && walk.progress.listed.load(Ordering::Acquire) < walk.max_entries {
            return true;
        }
        if let Ok(mut unindexed) = walk.progress.unindexed.lock() {
            unindexed.insert(directory.clone());
        }
        false
    })
}

/// Whether the running build walk has been asked to stop: the workspace is closing, so the walk
/// ends without publishing anything.
pub(crate) fn stop_requested() -> bool {
    WALK.with(|current| {
        current.borrow().as_ref().is_some_and(|walk| walk.stop.load(Ordering::Acquire))
    })
}
