//! The run-information index: a lightweight, read-only collection of the start,
//! resource and end facts that **real executors** publish about tools they are
//! actually running.
//!
//! WHAT THIS DELIBERATELY IS NOT
//! * It is not a scheduler. It holds no admission decision, no permit and no lock
//!   over a resource. Nothing here can start, stop, queue or refuse a tool.
//! * It is not the tool state machine. The durable Attempt/Result state stays with
//!   its real owner; this index cannot adjudicate `Unknown` and never replays.
//! * It does not release file protection. A's common admission service is asked
//!   nothing by this module and depends on nothing in it. See
//!   `publishing_never_touches_admission_or_protection` and
//!   `the_index_has_no_admission_surface` for the executable form of that.
//! * It is not `aura-core::activity::AgentActivities`. That registry is the Web
//!   permission/cancel authority; expanding it would have merged observer and
//!   authority. This is a separate owner with a separate identity type.
//!
//! WHY THE PUBLISH PATH IS SO SHORT
//! A publisher is a thread that is about to do real work. The publication path may
//! therefore not re-open a path, resolve an identity, wait on the journal, or take
//! a business lock. Everything it needs is handed to it by the owner that already
//! holds the verified facts, and the only synchronisation it takes is this module's
//! own mutex over plain in-memory indexes. The tests
//! `publishing_does_not_open_a_path_or_wait_on_a_business_lock` and
//! `publishing_a_large_burst_stays_within_the_fixed_budget` measure exactly that.
//!
//! WHY AN EMPTY TABLE IS NOT "THE HOST HAS NO WRITER"
//! Coverage is tracked separately from the rows and is observable on a path that
//! does not go through the same queue that overflowed: the counters are bumped on
//! the publish path itself, before any queue or cap can drop the fact. A reader
//! that sees zero rows with `Complete` coverage is seeing a real zero; a reader
//! that sees `Unavailable` is seeing a broken index, and the two are different
//! values (`ActivityCoverage::unavailable` vs `ActivityCoverage::complete(0, 0)`).
mod projection;
mod resource_index;
pub use projection::{
    ActivityRelevance, ActivitySnapshot, ActivitySummary, ActivitySummaryBudget,
    ActivitySummaryRow, ActivityVisibility, targets_related,
};

use aura_activity_domain::*;
use aura_domain_common::{Revision, SessionId, UtcTimestamp, WorkspaceId};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, Mutex, MutexGuard},
};

/// One entry of the minimal owner/attachment table: who really publishes what,
/// and under which identity.
///
/// It is a table rather than a trait so that the coverage report can be read as
/// data, and so the negative test can assert the exact set of publishers instead
/// of trusting a list in a document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityPublisherRow {
    pub kind: ActivityKind,
    /// The real owner module that calls [`ActivityIndex::publish`].
    pub real_owner: &'static str,
    /// The identity that owner reuses, verbatim.
    pub reused_identity: &'static str,
    /// Whether this owner may carry an Agent identity at all.
    pub may_carry_agent_identity: bool,
    pub source_file: &'static str,
}

/// The complete publisher table. Six real executors, no synthetic ones.
///
/// R1: every row here names a call that really exists. The table previously carried a
/// **reservation** row pointing at `agent_execution/reservation.rs` with no `publish` call in that
/// file at all, and the Main Fs row still pointed at `routes/tool.rs`, which R1 removed. A row that
/// claims coverage without a call is worse than a missing row, because coverage read as data would
/// report an executor that does not publish. The reservation's real contribution is now the
/// **attempt ordinal** the dispatch publisher carries, which is stated in the row below rather than
/// as an owner of its own.
pub const ACTIVITY_PUBLISHERS: &[ActivityPublisherRow] = &[
    ActivityPublisherRow {
        kind: ActivityKind::AgentTool,
        real_owner: "aura-core::agent_activity::execution::ObservedBackend; the actual \
                     file/Web/task worker begins observation after admission and owns the lease",
        reused_identity: "ToolCallId + the reserved SecurityOperationId + the reservation's real \
                          attempt ordinal; real AgentRunId",
        may_carry_agent_identity: true,
        source_file: "crates/aura-core/src/agent_execution/dispatch.rs",
    },
    ActivityPublisherRow {
        kind: ActivityKind::MainFs,
        real_owner: "aura-core::broker::dispatch_files (the ordinary fs.*/buffer-save entry)",
        reused_identity: "SecurityOperationId + the request's own CommandInvocationId when it has \
                          one; no AgentRunId",
        may_carry_agent_identity: false,
        source_file: "crates/aura-core/src/broker/files.rs",
    },
    ActivityPublisherRow {
        kind: ActivityKind::BufferCommit,
        real_owner: "aura-workspace::buffer (the real save window: Start publishes, terminal ends)",
        reused_identity: "EditorReservationId + buffer revision; the store's already-verified \
                          FileIdentity; no AgentRunId",
        may_carry_agent_identity: false,
        source_file: "crates/aura-workspace/src/buffer.rs",
    },
    ActivityPublisherRow {
        kind: ActivityKind::P04Publication,
        real_owner: "aura-core::p04 (real publication commit point)",
        reused_identity: "the actual CorrelationId + optional SecurityOperationId; no AgentRunId",
        may_carry_agent_identity: false,
        source_file: "crates/aura-core/src/p04.rs",
    },
    ActivityPublisherRow {
        kind: ActivityKind::ShellForeground,
        real_owner: "aura-core::command_execution::CorePort's ShellExecutionObserver, \
                     handed to the actual native or managed execution owner",
        reused_identity: "ToolCallId + SecurityOperationId + real attempt ordinal; the call's own Agent identity",
        may_carry_agent_identity: true,
        source_file: "crates/aura-core/src/agent_execution/dispatch.rs",
    },
    ActivityPublisherRow {
        kind: ActivityKind::ShellManagedBackground,
        real_owner: "aura-core::command_execution (managed background owner)",
        reused_identity: "the background's actual SecurityOperationId + attempt ordinal; no AgentRunId",
        may_carry_agent_identity: false,
        source_file: "crates/aura-core/src/command_execution/port.rs",
    },
];

/// Why the index refused or dropped a fact. Kept as a bounded counter set so the
/// reason is observable without retaining the dropped payload.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActivityDropCounters {
    pub capacity_dropped: u64,
    pub unavailable_dropped: u64,
    pub rejected: u64,
}

struct IndexState {
    /// `ActivityId -> record`. One row per real activity.
    records: HashMap<ActivityId, Arc<ActivityRecord>>,
    resources: resource_index::ResourceIndex,
    /// Resource -> the set of activities touching it. Repeated reads of one
    /// resource must stay separate rows, so this is a set and never a single
    /// value; a map that stored one id would silently overwrite the other read.
    by_resource: HashMap<ActivityResourceKey, BTreeSet<ActivityId>>,
    /// Session -> activities, for owner-scope filtering.
    by_session: BTreeMap<SessionId, BTreeSet<ActivityId>>,
    /// Workspace -> activities. This is a *filter*, never a hiding rule: a
    /// resource lookup ignores it on purpose.
    by_workspace: BTreeMap<WorkspaceId, BTreeSet<ActivityId>>,
    /// Tool group -> activities, so one group's problems cannot hide another's.
    by_group: BTreeMap<ActivityToolGroup, BTreeSet<ActivityId>>,
    /// Unknown-write-set activities, so a reader can always ask for the honest
    /// "we cannot prove this Shell's write set" set.
    unknown_write_set: BTreeSet<ActivityId>,
    /// R3: directory object -> the activities whose target is inside that subtree.
    ///
    /// WHY THIS IS SEPARATE FROM `by_resource`
    /// `by_resource` is exact-key association: it answers "who named this same object". A directory
    /// write must additionally answer "who named something **under** this directory", which is a
    /// different question and cannot be answered by key equality. Without this index that question
    /// was answered by scanning every row, and the only relation recognised was exact enum-key
    /// equality - so a write to a file inside a directory was invisible to a query about the
    /// directory. This index is bounded by [`MAX_ACTIVITY_DIRECTORY_INDEX`] distinct directory
    /// objects; past the bound the oldest directory is dropped and the query honestly falls back to
    /// a scan rather than silently missing rows.
    by_directory: BTreeMap<(u32, u64), BTreeSet<ActivityId>>,
    /// The directory keys in first-recorded order, so the bound above is enforced deterministically.
    directory_order: std::collections::VecDeque<(u32, u64)>,
    pending_ends: std::sync::mpsc::Receiver<(ActivityId, u32)>,
    revision: Revision,
    counters: ActivityDropCounters,
    /// Set while the index is reporting itself unavailable.
    unavailable: bool,
    capacity: usize,
}

/// How many distinct directory objects keep a descendant index (R3).
///
/// Beyond this the oldest directory stops maintaining its own set, and a descendant query for it
/// falls back to a real scan of the rows. That is a bounded, honest degradation: the answer stays
/// correct, only the fast path is lost, and no row is hidden.
pub const MAX_ACTIVITY_DIRECTORY_INDEX: usize = 1024;

/// A physical-resource key derived from A's already-verified facts.
///
/// The key is built by copying facts the publishing owner already resolved; it is
/// never built by re-opening or re-resolving a path here.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ActivityResourceKey {
    VerifiedObject { volume: String, file_id: String },
    VerifiedEntry { volume: String, file_id: String, pending: Vec<String>, folded: bool },
    Object { volume_serial: u32, file_index: u64 },
    Entry { parent_volume_serial: u32, parent_file_index: u64, leaf: String, folded: bool },
    Subtree { volume_serial: u32, file_index: u64 },
    Unproven { hint: String },
}

impl ActivityResourceKey {
    #[must_use]
    pub fn verified(identity: &aura_domain_common::FileIdentity) -> Self {
        Self::VerifiedObject {
            volume: identity.volume_serial_number.clone(),
            file_id: identity.file_id.clone(),
        }
    }

    /// Derives the key from an already-verified target. It performs no I/O.
    #[must_use]
    pub fn of(target: &ActivityResourceTarget) -> Self {
        match target {
            ActivityResourceTarget::VerifiedObject { identity, .. } => Self::verified(identity),
            ActivityResourceTarget::VerifiedEntry { parent, case_insensitive, .. } => {
                Self::VerifiedEntry {
                    volume: parent.volume_serial_number.clone(),
                    file_id: parent.file_id.clone(),
                    pending: match target.name_keys() {
                        Some(keys) => keys,
                        None => {
                            return Self::Unproven { hint: "entry comparison unavailable".into() };
                        }
                    },
                    folded: *case_insensitive,
                }
            }
            ActivityResourceTarget::ExistingObject { volume_serial, file_index, .. } => {
                Self::Object { volume_serial: *volume_serial, file_index: *file_index }
            }
            ActivityResourceTarget::DirectoryEntry {
                parent_volume_serial,
                parent_file_index,
                case_insensitive,
                ..
            } => Self::Entry {
                parent_volume_serial: *parent_volume_serial,
                parent_file_index: *parent_file_index,
                leaf: match target.name_keys().and_then(|keys| keys.into_iter().next()) {
                    Some(key) => key,
                    None => return Self::Unproven { hint: "entry comparison unavailable".into() },
                },
                folded: *case_insensitive,
            },
            ActivityResourceTarget::Subtree { volume_serial, file_index, .. } => {
                Self::Subtree { volume_serial: *volume_serial, file_index: *file_index }
            }
            ActivityResourceTarget::Unproven { hint } => Self::Unproven { hint: hint.clone() },
        }
    }
}

/// The index. One per Core runtime instance.
///
/// It is `Clone` over an `Arc` so an executor can hand a handle to the owner that
/// publishes on its behalf without a second index existing.
#[derive(Clone)]
pub struct ActivityIndex(Arc<Mutex<IndexState>>, Arc<PublicationOutbox>);

struct PublicationOutbox {
    ends: std::sync::mpsc::SyncSender<(ActivityId, u32)>,
    counters: AtomicDropCounters,
    reset: std::sync::atomic::AtomicBool,
    disabled: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
struct AtomicDropCounters {
    capacity_dropped: std::sync::atomic::AtomicU64,
    unavailable_dropped: std::sync::atomic::AtomicU64,
    rejected: std::sync::atomic::AtomicU64,
}
impl AtomicDropCounters {
    fn read(&self) -> ActivityDropCounters {
        use std::sync::atomic::Ordering::Acquire;
        ActivityDropCounters {
            capacity_dropped: self.capacity_dropped.load(Acquire),
            unavailable_dropped: self.unavailable_dropped.load(Acquire),
            rejected: self.rejected.load(Acquire),
        }
    }
    fn observe(&self, outcome: ActivityPublishOutcome) {
        let counter = match outcome {
            ActivityPublishOutcome::CapacityExhausted => &self.capacity_dropped,
            ActivityPublishOutcome::Unavailable => &self.unavailable_dropped,
            ActivityPublishOutcome::Rejected => &self.rejected,
            _ => return,
        };
        let _ = counter.fetch_update(
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
            |value| Some(value.saturating_add(1)),
        );
    }
}

impl Default for ActivityIndex {
    fn default() -> Self {
        Self::new(DEFAULT_ACTIVITY_CAPACITY)
    }
}

/// Row capacity of one index. Sized so a busy runtime still holds every real
/// activity, while a runaway publisher cannot grow memory without bound.
pub const DEFAULT_ACTIVITY_CAPACITY: usize = 512;

impl ActivityIndex {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (ends, pending_ends) = std::sync::mpsc::sync_channel(DEFAULT_ACTIVITY_CAPACITY * 2);
        Self(
            Arc::new(Mutex::new(IndexState {
                records: HashMap::new(),
                resources: resource_index::ResourceIndex::default(),
                by_resource: HashMap::new(),
                by_session: BTreeMap::new(),
                by_workspace: BTreeMap::new(),
                by_group: BTreeMap::new(),
                unknown_write_set: BTreeSet::new(),
                by_directory: BTreeMap::new(),
                directory_order: std::collections::VecDeque::new(),
                revision: Revision::INITIAL,
                counters: ActivityDropCounters::default(),
                unavailable: false,
                capacity: capacity.clamp(1, DEFAULT_ACTIVITY_CAPACITY),
                pending_ends,
            })),
            Arc::new(PublicationOutbox {
                ends,
                counters: AtomicDropCounters::default(),
                reset: std::sync::atomic::AtomicBool::new(false),
                disabled: std::sync::atomic::AtomicBool::new(false),
            }),
        )
    }

    fn lock(&self) -> Option<MutexGuard<'_, IndexState>> {
        let mut state = self.0.try_lock().ok()?;
        if self.1.reset.swap(false, std::sync::atomic::Ordering::AcqRel) {
            state.records.clear();
            state.resources = resource_index::ResourceIndex::default();
            state.by_resource.clear();
            state.by_session.clear();
            state.by_workspace.clear();
            state.by_group.clear();
            state.unknown_write_set.clear();
            state.by_directory.clear();
            state.directory_order.clear();
            state.revision = advance(state.revision);
        }
        // Ends are bounded, nonblocking notifications. Drain before exposing any active row.
        for _ in 0..DEFAULT_ACTIVITY_CAPACITY * 2 {
            let Ok((id, generation)) = state.pending_ends.try_recv() else { break };
            end_record(&mut state, id, generation);
        }
        state.unavailable = self.1.disabled.load(std::sync::atomic::Ordering::Acquire);
        Some(state)
    }

    /// Takes the index's own mutex **even if it was poisoned** (R5).
    ///
    /// WHY THIS IS A SEPARATE ACCESSOR
    /// A poisoned mutex is a permanent state: every later `lock()` fails for the life of the
    /// process. That is the right answer for reading - an index whose invariant a panic may have
    /// broken has no business being trusted as a picture of what is running - but it is the wrong
    /// answer for the two operations that are allowed to **restore** it. `mark_available` exists
    /// precisely to say "the condition that broke this is over", and it could never do its job
    /// through `lock()`: the poisoning it was written to clear made it a no-op forever.
    ///
    /// The recovered state is never *read* through this path; it is only marked. A caller that
    /// clears the mark is asserting that the cause is gone, which is what a restart of the index's
    /// storage really is.
    fn lock_including_poisoned(&self) -> Option<MutexGuard<'_, IndexState>> {
        self.0.try_lock().ok()
    }

    /// Records one activity a real executor is starting.
    ///
    /// This never fails a tool. Every non-recorded outcome is reported to the
    /// caller so its own coverage fact can be updated, and the executor proceeds
    /// unchanged.
    pub fn publish(&self, mut record: ActivityRecord) -> ActivityPublishOutcome {
        if !record.has_no_extra_facts() {
            self.count_rejected();
            return ActivityPublishOutcome::Rejected;
        }
        let Some(mut state) = self.lock() else {
            self.count_unavailable();
            return ActivityPublishOutcome::Unavailable;
        };
        if state.unavailable {
            state.counters.unavailable_dropped =
                state.counters.unavailable_dropped.saturating_add(1);
            return ActivityPublishOutcome::Unavailable;
        }
        if state.records.contains_key(&record.id) {
            // A repeated publish of the identical fact is safe and idempotent.
            return ActivityPublishOutcome::Duplicate;
        }
        if state.records.len() >= state.capacity {
            state.counters.capacity_dropped = state.counters.capacity_dropped.saturating_add(1);
            return ActivityPublishOutcome::CapacityExhausted;
        }
        let id = record.id;
        state.by_session.entry(record.owner.session_id).or_default().insert(id);
        state.by_workspace.entry(record.owner.workspace_id).or_default().insert(id);
        state.by_group.entry(record.tool_group).or_default().insert(id);
        for resource in &record.resources {
            let key = ActivityResourceKey::of(&resource.target);
            state.by_resource.entry(key).or_default().insert(id);
            state.resources.insert(id, &resource.target);
            if resource.relation.is_unknown_write_set() {
                state.unknown_write_set.insert(id);
            }
            index_directory_membership(&mut state, id, &resource.target);
        }
        state.revision = advance(state.revision);
        record.published_at_revision = state.revision;
        state.records.insert(id, Arc::new(record));
        ActivityPublishOutcome::Recorded
    }

    /// Removes one activity, but only for the exact identity that ended.
    ///
    /// A late `end` from an older generation names an id that is not in the table
    /// (or a generation that no longer matches), so it cannot delete the newer
    /// generation's row. A repeated `end` finds nothing and is equally safe.
    ///
    /// R3: every index entry this row created is reclaimed, and an entry that becomes empty is
    /// **removed** rather than left behind. That is what keeps memory from growing with the
    /// history of sessions the process has already finished with.
    pub fn end(&self, id: ActivityId, generation: u32) -> ActivityEndOutcome {
        if let Some(mut state) = self.lock() {
            return end_record(&mut state, id, generation);
        }
        // Never park an executor behind an observer. Retain the exact end for the next reader.
        if self.1.ends.try_send((id, generation)).is_err() {
            self.1.reset.store(true, std::sync::atomic::Ordering::Release);
            self.count_unavailable();
        }
        ActivityEndOutcome::IndexUnavailable
    }

    /// Marks the index unavailable. Tools keep running; coverage starts saying so
    /// on this independent path rather than through a queue that may already have
    /// dropped the fact that would have explained it.
    pub fn mark_unavailable(&self) {
        self.1.disabled.store(true, std::sync::atomic::Ordering::Release);
        if let Some(mut state) = self.lock() {
            state.revision = advance(state.revision);
        }
    }

    /// Clears the unavailable mark, e.g. after a restart of the index's storage.
    ///
    /// R5: this is the one operation that reaches the state **through a poisoned mutex**, because
    /// it is the one operation whose whole purpose is recovery. Through the ordinary `lock()` it
    /// could never run after the very event it exists to repair, so the index stayed unavailable
    /// for the life of the process.
    pub fn mark_available(&self) {
        self.1.disabled.store(false, std::sync::atomic::Ordering::Release);
        if let Some(mut state) = self.lock_including_poisoned() {
            state.unavailable = false;
            state.revision = advance(state.revision);
        }
    }

    /// Fills the index up to its capacity, for the capacity counterexample.
    #[doc(hidden)]
    pub fn capacity(&self) -> usize {
        self.lock().map_or(0, |state| state.capacity)
    }

    fn count_rejected(&self) {
        self.1.counters.observe(ActivityPublishOutcome::Rejected);
    }

    fn count_unavailable(&self) {
        self.1.counters.observe(ActivityPublishOutcome::Unavailable);
    }

    /// An upstream information publisher missed a fact. Tools continue; coverage is partial.
    pub fn note_missed_publication(&self) {
        self.count_unavailable();
    }

    /// The candidate activity ids a resource query really needs to consider (R3).
    ///
    /// WHY THIS EXISTS
    /// A precise question - "who is writing this file" - used to be answered by snapshotting every
    /// row in the index and filtering linearly, so the cost of the query grew with the whole
    /// runtime's activity rather than with its answer. The index already keeps the association in
    /// `by_resource`; this is the entry point that actually uses it.
    ///
    /// WHAT IT RETURNS
    /// The union of two index lookups: the exact resource key, and every directory object whose
    /// subtree contains the queried target. The second is what makes "a directory write finds the
    /// affected children" a real lookup rather than a scan, and it is why a child named by its
    /// verified parent is associated with a query about that parent.
    ///
    /// `None` means "use the full scan": the index is unavailable, or the query names an `Unproven`
    /// target whose only honest association is the hint string itself. It never means "empty".
    #[must_use]
    pub fn resource_candidates(
        &self,
        target: &ActivityResourceTarget,
    ) -> Option<BTreeSet<ActivityId>> {
        let state = self.lock()?;
        if state.unavailable {
            return None;
        }
        let mut candidates = state.resources.candidates(target, &state.by_resource)?;
        // Legacy numeric callers retain the parent-membership candidate behavior. Production
        // full identities are represented by the ancestry/prefix index, without this fallback.
        if !matches!(
            target,
            ActivityResourceTarget::VerifiedObject { .. }
                | ActivityResourceTarget::VerifiedEntry { .. }
        ) {
            if let ActivityResourceTarget::Subtree { volume_serial, file_index, .. } = target
                && let Some(inside) = state.by_directory.get(&(*volume_serial, *file_index))
            {
                candidates.extend(inside.iter().copied());
            }
            for directory in directory_keys_of(target) {
                if let Some(outside) = state.by_directory.get(&directory) {
                    candidates.extend(outside.iter().copied());
                }
            }
        }
        Some(candidates)
    }

    /// Takes one fixed snapshot of the index and projects it for one reader.
    ///
    /// Every reader - the precise query, a page of one, or the model summary -
    /// goes through this one projection, so they all describe the same picture.
    ///
    /// A caller that needs several readers to agree must **hold** the returned
    /// [`ActivitySnapshot`] and call [`ActivitySnapshot::query`] on it. Calling
    /// this method twice takes two samples, which is the correct behaviour for two
    /// different model steps and the wrong behaviour inside one.
    /// Test seam holds the exact state needed by actual publish/end/query paths.
    #[doc(hidden)]
    pub fn with_publication_state_held_for_test<T>(&self, work: impl FnOnce() -> T) -> T {
        let _held = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        work()
    }

    #[must_use]
    pub fn snapshot(&self, view: &ActivityVisibility) -> ActivitySnapshot {
        self.snapshot_with_seams(view, &|record| view.scope_matches(record), &|| {}, None)
    }

    /// [`Self::snapshot`] with an observation between its two phases (R5 evidence).
    ///
    /// It exists so a directed test can park a reader **at the instant the identity guard has been
    /// released and the row materialisation has not started** - the gap that only exists if the guard
    /// really was dropped, which is precisely the fix under test. A timing-based assertion cannot
    /// establish this: an earlier version of the evidence passed against a deliberately
    /// re-serialised implementation, so it proved nothing.
    ///
    /// `between` runs with **no index guard held**. A publisher that completes while a reader is
    /// parked here therefore proves the reader is not holding the lock across its whole projection;
    /// under the pre-fix shape the guard was held for the entire body, so the publisher would block
    /// until the reader returned.
    #[doc(hidden)]
    #[must_use]
    pub fn snapshot_with_seams_for_test(
        &self,
        view: &ActivityVisibility,
        scope: &(dyn Fn(&ActivityRecord) -> bool + Sync),
        between: &(dyn Fn() + Sync),
    ) -> ActivitySnapshot {
        self.snapshot_with_seams(view, scope, between, None)
    }

    fn snapshot_with_seams(
        &self,
        view: &ActivityVisibility,
        scope: &(dyn Fn(&ActivityRecord) -> bool + Sync),
        between: &(dyn Fn() + Sync),
        query: Option<&ActivityQuery>,
    ) -> ActivitySnapshot {
        let sampled_at = UtcTimestamp::now();
        // Only immutable Arc references are frozen under the index guard. Filtering, projection,
        // callbacks and rendering happen outside; no second read can mix revisions or owners.
        let Some((records, mut counters, revision, unavailable)) = self.lock().map(|state| {
            let (records, unsupported) = resource_index::select_records(&state, query);
            (records, state.counters, state.revision, state.unavailable || unsupported)
        }) else {
            let mut snapshot = ActivitySnapshot::unavailable(view, sampled_at);
            let losses = self.1.counters.read();
            snapshot.header.coverage.capacity_dropped = losses.capacity_dropped;
            snapshot.header.coverage.unavailable_dropped = losses.unavailable_dropped;
            snapshot.header.coverage.rejected = losses.rejected;
            return snapshot;
        };
        let losses = self.1.counters.read();
        counters.capacity_dropped =
            counters.capacity_dropped.saturating_add(losses.capacity_dropped);
        counters.unavailable_dropped =
            counters.unavailable_dropped.saturating_add(losses.unavailable_dropped);
        counters.rejected = counters.rejected.saturating_add(losses.rejected);
        between();
        let mut total_in_scope = 0;
        let mut visibility_filtered = 0;
        let mut rows = Vec::new();
        let mut captured = Vec::new();
        if !unavailable {
            for record in records {
                if !scope(&record) {
                    continue;
                }
                total_in_scope += 1;
                if !view.may_see(&record) {
                    visibility_filtered += 1;
                    continue;
                }
                rows.push(view.project(&record));
                captured.push(record);
            }
        }
        rows.sort_by(compare_rows);
        ActivitySnapshot {
            header: ActivitySnapshotHeader {
                sampled_at,
                runtime_instance_id: view.runtime_instance_id,
                revision,
                coverage: ActivityCoverage {
                    state: if unavailable {
                        ActivityCoverageState::Unavailable
                    } else if counters.capacity_dropped > 0
                        || counters.unavailable_dropped > 0
                        || counters.rejected > 0
                        || visibility_filtered > 0
                    {
                        ActivityCoverageState::Partial
                    } else {
                        ActivityCoverageState::Complete
                    },
                    total_in_scope,
                    visibility_filtered,
                    capacity_dropped: counters.capacity_dropped,
                    unavailable_dropped: counters.unavailable_dropped,
                    rejected: counters.rejected,
                    omitted_by_limit: 0,
                    omission_reasons: {
                        let mut reasons = omission_reasons(counters, visibility_filtered);
                        if unavailable
                            && !reasons.contains(&ActivityOmissionReason::IndexUnavailable)
                        {
                            reasons.push(ActivityOmissionReason::IndexUnavailable);
                        }
                        reasons
                    },
                    truncated: false,
                },
                stale: false,
            },
            rows,
            captured,
        }
    }
}

impl ActivityIndex {
    #[must_use]
    pub fn snapshot_query(
        &self,
        view: &ActivityVisibility,
        query: &ActivityQuery,
    ) -> ActivitySnapshot {
        self.snapshot_with_seams(
            view,
            &|record| {
                view.scope_matches(record)
                    && match query {
                        ActivityQuery::Session => record.owner.session_id == view.session_id,
                        ActivityQuery::Group { group } => record.tool_group == *group,
                        ActivityQuery::UnknownWriteSet => record
                            .resources
                            .iter()
                            .any(|resource| resource.relation.is_unknown_write_set()),
                        _ => true,
                    }
            },
            &|| {},
            Some(query),
        )
    }

    /// Whether the index currently reports itself unavailable.
    #[must_use]
    pub fn is_unavailable(&self) -> bool {
        self.lock().is_none_or(|state| state.unavailable)
    }

    /// How many rows the index currently holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().map_or(0, |state| state.records.len())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A deterministic fingerprint of the current rows. Two samples compare equal
    /// exactly when the index content is unchanged, which is how "the retry reused
    /// the frozen picture" is checkable.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let Some(state) = self.lock() else {
            return "unavailable".to_owned();
        };
        let mut out = String::with_capacity(state.records.len() * 40);
        let mut records = state.records.iter().collect::<Vec<_>>();
        records.sort_by_key(|(id, _)| **id);
        for (id, record) in records {
            out.push_str(&id.to_string());
            out.push(':');
            out.push_str(record.kind.as_str());
            out.push(':');
            out.push_str(&record.started_at.to_string());
            out.push('\n');
        }
        out
    }

    /// The fixed snapshot's current revision, so a later reader can tell whether
    /// the picture it holds is still the current one.
    #[must_use]
    pub fn revision(&self) -> Revision {
        self.lock().map_or(Revision::INITIAL, |state| state.revision)
    }

    /// How many per-key index entries are currently held (R3).
    ///
    /// It exists so "an ended activity reclaims its entries" is checkable rather than asserted. A
    /// map left holding empty sets grows with every session, workspace, group and resource the
    /// process has ever seen, which is the leak R3 names.
    ///
    /// Returns `(by_resource, by_directory)`.
    #[must_use]
    pub fn index_entries(&self) -> (usize, usize) {
        self.lock().map_or((0, 0), |state| (state.by_resource.len(), state.by_directory.len()))
    }
}

/// Advances the index revision, saturating instead of wrapping.
///
/// `Revision` has no `next`; the index owns its own monotonic step so a reader
/// can compare two samples without a second source of truth for "now".
fn end_record(state: &mut IndexState, id: ActivityId, generation: u32) -> ActivityEndOutcome {
    let Some(record) = state.records.get(&id) else {
        return ActivityEndOutcome::NotCurrent;
    };
    if !record.matches_end(id, generation) {
        return ActivityEndOutcome::GenerationMismatch;
    }
    let record = state.records.remove(&id).expect("checked");
    reclaim(&mut state.by_session, record.owner.session_id, id);
    reclaim(&mut state.by_workspace, record.owner.workspace_id, id);
    reclaim(&mut state.by_group, record.tool_group, id);
    for resource in &record.resources {
        state.resources.remove(id, &resource.target);
        let key = ActivityResourceKey::of(&resource.target);
        if let Some(set) = state.by_resource.get_mut(&key) {
            set.remove(&id);
            if set.is_empty() {
                state.by_resource.remove(&key);
            }
        }
        for directory in directory_keys_of(&resource.target) {
            if let Some(set) = state.by_directory.get_mut(&directory) {
                set.remove(&id);
                if set.is_empty() {
                    state.by_directory.remove(&directory);
                    state.directory_order.retain(|key| *key != directory);
                }
            }
        }
    }
    state.unknown_write_set.remove(&id);
    state.revision = advance(state.revision);
    ActivityEndOutcome::Ended
}

fn advance(revision: Revision) -> Revision {
    Revision::new(revision.value().saturating_add(1))
}

/// Removes one id from a per-key set and drops the key when it becomes empty (R3).
///
/// A key left behind with an empty set is a small leak that grows with every session, workspace
/// and tool group the process has ever served, so emptiness is reclaimed here rather than left for
/// a later sweep that does not exist.
fn reclaim<K: Ord + Copy>(map: &mut BTreeMap<K, BTreeSet<ActivityId>>, key: K, id: ActivityId) {
    if let Some(set) = map.get_mut(&key) {
        set.remove(&id);
        if set.is_empty() {
            map.remove(&key);
        }
    }
}

/// The directory objects whose subtree contains this target (R3).
///
/// A directory target covers itself; a child named by its verified parent covers that parent. So a
/// query about a directory finds every activity that named something inside it, which exact enum-key
/// equality could never report.
fn directory_keys_of(target: &ActivityResourceTarget) -> Vec<(u32, u64)> {
    match target {
        ActivityResourceTarget::Subtree { volume_serial, file_index, .. } => {
            vec![(*volume_serial, *file_index)]
        }
        ActivityResourceTarget::DirectoryEntry {
            parent_volume_serial, parent_file_index, ..
        } => {
            vec![(*parent_volume_serial, *parent_file_index)]
        }
        ActivityResourceTarget::VerifiedObject { .. }
        | ActivityResourceTarget::VerifiedEntry { .. }
        | ActivityResourceTarget::ExistingObject { .. }
        | ActivityResourceTarget::Unproven { .. } => Vec::new(),
    }
}

/// Records the subtree memberships one published target creates (R3), inside the bound.
fn index_directory_membership(
    state: &mut IndexState,
    id: ActivityId,
    target: &ActivityResourceTarget,
) {
    for directory in directory_keys_of(target) {
        if !state.by_directory.contains_key(&directory) {
            // A new directory key: enforce the bound before admitting it, so the index cannot grow
            // with the history of every directory the process has ever written inside.
            while state.by_directory.len() >= MAX_ACTIVITY_DIRECTORY_INDEX {
                let Some(oldest) = state.directory_order.pop_front() else {
                    break;
                };
                state.by_directory.remove(&oldest);
            }
            state.directory_order.push_back(directory);
        }
        state.by_directory.entry(directory).or_default().insert(id);
    }
}

fn omission_reasons(
    counters: ActivityDropCounters,
    visibility_filtered: u64,
) -> Vec<ActivityOmissionReason> {
    let mut reasons = Vec::new();
    if counters.capacity_dropped > 0 {
        reasons.push(ActivityOmissionReason::CapacityDropped);
    }
    if counters.unavailable_dropped > 0 {
        reasons.push(ActivityOmissionReason::IndexUnavailable);
    }
    if counters.rejected > 0 {
        reasons.push(ActivityOmissionReason::RejectedFact);
    }
    if visibility_filtered > 0 {
        reasons.push(ActivityOmissionReason::VisibilityFiltered);
    }
    reasons
}

/// What happened to an `end`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivityEndOutcome {
    /// The exact activity was removed.
    Ended,
    /// The named identity is not the current one. Nothing was removed.
    NotCurrent,
    /// The identity exists but belongs to another generation.
    GenerationMismatch,
    /// The index could not answer. Nothing was removed.
    IndexUnavailable,
}

/// The observer the publisher uses to report a fact it could not record.
///
/// It exists so that "the index dropped something" is observable on a path that
/// does not go through the index's own queue. A publisher that sees a drop
/// increments this and continues; the tool is unaffected either way.
///
/// R5: THE OBSERVER'S OWN FAILURE MUST NOT BECOME SILENT DATA LOSS.
/// The counters live behind a plain mutex, so a panic while it is held poisoned it - and
/// `observe`/`counters` both went through `lock()`, which returns `Err` for a poisoned mutex. The
/// effect was that the observer stopped counting **permanently and invisibly**: coverage would then
/// report a clean `Complete` for a runtime that had really been dropping facts, which is worse than
/// reporting the drop. A mutex whose entire content is three `u64` counters has no invariant a panic
/// can half-break, so a poisoned lock is recovered rather than abandoned. The tool path is unaffected
/// either way: this is still a bounded in-memory increment with no I/O and no blocking wait.
#[derive(Clone, Default)]
pub struct ActivityPublicationObserver(Arc<AtomicDropCounters>);

/// A cloneable handle to one process's publication observer.
///
/// It exists so a publisher that is not the Agent — the Shell backend, Main's file
/// service — reports into the *same* drop counters as the Agent, instead of each
/// owner keeping a private tally that no reader could combine.
pub type ActivityObserverHandle = ActivityPublicationObserver;

impl ActivityPublicationObserver {
    #[must_use]
    pub fn counters(&self) -> ActivityDropCounters {
        self.0.read()
    }

    /// Counting is atomic: neither a reader nor a failed observer can hold a lock
    /// needed by a tool publication.
    pub fn observe(&self, outcome: ActivityPublishOutcome) {
        self.0.observe(outcome);
    }
}

#[cfg(test)]
mod tests;
