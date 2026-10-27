//! The one owner of Agent Store access: bounded execution, real transactions and
//! per-dependency health.
//!
//! WHY THIS EXISTS
//! The Agent run, its status and its failure record all read and write the same authoritative
//! V2 store, and they used to do it by taking `Mutex<V2Store>` and running SQLite **on the
//! reactor thread that also has to deliver the heartbeat**. Three things follow:
//!
//! 1. **Where the wait happens.** Every step runs as one bounded section on the shared
//!    [`SyncBoundary`], so a caller waiting for a permit or for a transaction is not occupying
//!    a Tokio worker. The section owns the permit; the caller's wait and the transaction's
//!    real lifetime are separate facts.
//! 2. **Who owns the result.** An admitted section hands its real result to a settlement sink
//!    that runs on the thread which really did the work, so a caller whose own wait ended -
//!    timeout, cancel, disconnect - cannot lose it. A **refusal** is the only answer that means
//!    "it never ran", and it is the only one reported that way.
//! 3. **Which failures are shared.** The V2 store is one SQLite database: a store-level failure
//!    is a fact about the dependency, and every dependent operation is told so within bounds. A
//!    revision conflict or a stale guard is a fact about *this* operation. The two are
//!    classified by type ([`classify`]), never by parsing a message.
//!
//! READS AND WRITES
//! Writes go through the one write authority (`BrokerJournal`'s `V2Store`), unchanged: one
//! connection, one `unit_of_work`, one commit. Reads use a **read-only connection to the same
//! database** when one can be opened, so a status or projection read is not parked behind a
//! write transaction holding the write connection - exactly the isolation one run's status
//! needs from another run's write. The read connection cannot write (`query_only`), so it is
//! not a second SQL authority; it is the same data, read-only. When it cannot be opened the
//! read falls back to the write connection and that fact is recorded, never hidden.
use crate::{
    blocking_pool::BlockingAdmission,
    sync_boundary::{SyncBoundary, SyncSection, SyncSubject},
};
use aura_domain_common::{AgentRunId, UtcTimestamp};
use aura_persistence_core::{Connection, PersistenceCoreError, SqliteFailureClass};
use aura_persistence_v2::V2Store;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

mod credential_scope;
pub(crate) mod health;

/// Default-off, acceptance-only external control of the Agent hold points (S04-E).
///
/// Compiled only with `--features acceptance-barrier`, so the default image has no reader for the
/// control document and no arrival on the tool-dispatch path. See the module for what it may hold.
#[cfg(feature = "acceptance-barrier")]
pub(crate) mod acceptance_hold;

pub(crate) use health::{StoreDependency, StoreHealthSnapshot};

/// The V2 store. One dependency today; named so a second one cannot be conflated with it.
const JOURNAL: StoreDependency = StoreDependency::Journal;

/// Which access an observation came from.
///
/// It matters for clearing: a write that really succeeded proves the database serves both
/// reads and writes, so it clears either fact. A read that succeeded only proves reads work -
/// in WAL a reader is served while the writer is blocked - so it must not clear a write-side
/// unavailability and hide it from the next dependent write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StoreAccess {
    Read,
    Write,
}

/// The identity of one Store step: which run (when there is one) and which operation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StoreStep {
    run: Option<AgentRunId>,
    operation: &'static str,
}

impl StoreStep {
    #[must_use]
    pub(crate) const fn operation(operation: &'static str) -> Self {
        Self { run: None, operation }
    }

    #[must_use]
    pub(crate) const fn run(run: AgentRunId, operation: &'static str) -> Self {
        Self { run: Some(run), operation }
    }

    #[must_use]
    pub(crate) const fn run_id(self) -> Option<AgentRunId> {
        self.run
    }

    /// What a hold point or a span is matched against.
    #[must_use]
    pub(crate) fn identity(self) -> String {
        match self.run {
            Some(run) => format!("run={run}/{}", self.operation),
            None => self.operation.to_owned(),
        }
    }
}

/// A retained result: what really happened to one admitted Store write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReceiptState {
    /// The work was admitted and has not answered yet. It may still commit.
    InFlight,
    /// The work really ran to a terminal answer, written by the thread that ran it.
    Settled { committed: bool, code: Option<&'static str> },
    /// No worker was free inside the budget: the work provably never started.
    NeverStarted,
}

/// One admitted write's own record, kept after its caller stopped waiting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoreReceipt {
    pub(crate) id: u64,
    pub(crate) operation: &'static str,
    pub(crate) run: Option<AgentRunId>,
    pub(crate) state: ReceiptState,
}

/// How many receipts are kept. Bounded, oldest first: a long run cannot grow it.
const MAX_RECEIPTS: usize = 256;

#[derive(Default)]
struct Receipts {
    next: u64,
    entries: BTreeMap<u64, StoreReceipt>,
}

impl Receipts {
    fn open(&mut self, step: StoreStep) -> u64 {
        self.next = self.next.wrapping_add(1);
        let id = self.next;
        if self.entries.len() >= MAX_RECEIPTS
            && let Some(oldest) = self.entries.keys().next().copied()
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(
            id,
            StoreReceipt {
                id,
                operation: step.operation,
                run: step.run,
                state: ReceiptState::InFlight,
            },
        );
        id
    }

    fn settle(&mut self, id: u64, state: ReceiptState) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.state = state;
        }
    }
}

/// What the caller was told, with the failure classes kept apart.
#[derive(Debug)]
pub(crate) enum StoreError {
    /// No store worker was free inside the budget: the operation **did not run**.
    Refused { operation: &'static str, waited_ms: u64 },
    /// The operation started and did not answer inside the budget. Its result is retained.
    Unknown { operation: &'static str, waited_ms: u64, receipt: Option<u64> },
    /// The operation ran and the store answered with this real error.
    Failed { operation: &'static str, error: PersistenceCoreError },
    /// The shared dependency could not serve it. Every dependent operation is told the same.
    Unavailable {
        operation: &'static str,
        snapshot: StoreHealthSnapshot,
        last_known: Option<ProjectionFact>,
    },
}

impl StoreError {
    /// The typed code a caller reads instead of parsing a message.
    #[must_use]
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Refused { .. } => "agent.store_refused",
            Self::Unknown { .. } => "agent.store_unknown",
            Self::Failed { .. } => "agent.store_failed",
            Self::Unavailable { .. } => "agent.store_unavailable",
        }
    }

    /// The operation this failure belongs to.
    #[allow(dead_code)] // read by a diagnostic or a regression, never by a decision
    #[must_use]
    pub(crate) const fn operation(&self) -> &'static str {
        match self {
            Self::Refused { operation, .. }
            | Self::Unknown { operation, .. }
            | Self::Failed { operation, .. }
            | Self::Unavailable { operation, .. } => operation,
        }
    }

    /// Whether the operation provably never ran.
    #[must_use]
    pub(crate) const fn not_started(&self) -> bool {
        matches!(self, Self::Refused { .. })
    }

    /// The receipt a caller can query for the original result, when one exists.
    #[must_use]
    pub(crate) const fn receipt(&self) -> Option<u64> {
        match self {
            Self::Unknown { receipt, .. } => *receipt,
            _ => None,
        }
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused { operation, waited_ms } => write!(
                formatter,
                "agent.store_refused: {operation} did not run; no store worker was free within \
                 {waited_ms} ms"
            ),
            Self::Unknown { operation, waited_ms, receipt } => {
                write!(
                    formatter,
                    "agent.store_unknown: {operation} started and did not answer within \
                     {waited_ms} ms; its result is retained"
                )?;
                match receipt {
                    Some(receipt) => write!(formatter, " as receipt {receipt}"),
                    None => Ok(()),
                }
            }
            Self::Failed { operation, error } => {
                write!(formatter, "agent.store_failed: {operation}: {error}")
            }
            Self::Unavailable { operation, snapshot, last_known } => {
                write!(
                    formatter,
                    "agent.store_unavailable: {operation} needs {} which is unavailable ({})",
                    snapshot.dependency.as_str(),
                    snapshot.detail_text()
                )?;
                if let Some(fact) = last_known {
                    write!(
                        formatter,
                        "; last known projection revision {} observed at {}",
                        fact.revision, fact.observed_at
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for StoreError {}

/// The ownership verdict, computed from the journal's own rows.
///
/// The chain is the schema's own: `sessions(workspace_id)` → `turn_tasks(session_id,
/// state)` → `agent_runs(turn_task_id)`. Reading them together is what makes "these
/// four ids belong together" a real question rather than four independent existence
/// checks — four ids can each exist and still name two different tasks.
///
/// # The verdicts, and why they are six and not two
///
/// * the session is absent → `Unknown`;
/// * the session belongs to another workspace → `Mismatched`;
/// * the turn task is absent, or belongs to another session → `Unknown` /
///   `Mismatched` respectively;
/// * the turn task has ended → `TaskEnded` (real ownership, finished work — a stale
///   request rather than a forged one);
/// * the agent run is absent, or belongs to another turn task → `Unknown` /
///   `Mismatched`;
/// * any read error → `Unavailable`, which is never permission.
fn verify_scope_ownership_in(
    connection: &aura_persistence_core::Connection,
    scope: &aura_tool_contract::ToolRegistryScopeV1,
) -> aura_persistence::tool_entities::ScopeOwnershipV1 {
    use aura_persistence::tool_entities::ScopeOwnershipV1;
    use aura_persistence_core::OptionalExtension;

    let (workspace_id, session_id, turn_task_id, agent_run_id) = match scope {
        aura_tool_contract::ToolRegistryScopeV1::Workspace { workspace_id } => {
            // A workspace scope names no task, so ownership reduces to whether the
            // workspace itself exists. There is no liveness to answer.
            return match connection
                .query_row(
                    "SELECT 1 FROM workspaces WHERE id=?1",
                    [workspace_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
            {
                Ok(Some(_)) => ScopeOwnershipV1::Owned,
                Ok(None) => ScopeOwnershipV1::Unknown,
                Err(_) => ScopeOwnershipV1::Unavailable,
            };
        }
        aura_tool_contract::ToolRegistryScopeV1::Session {
            workspace_id,
            session_id,
            turn_task_id,
            agent_run_id,
        } => (*workspace_id, *session_id, *turn_task_id, *agent_run_id),
        // A child agent's run id is its own field, and it is that run whose ownership
        // the scope asserts.
        aura_tool_contract::ToolRegistryScopeV1::ChildAgent {
            workspace_id,
            session_id,
            turn_task_id,
            child_agent_run_id,
        } => (*workspace_id, *session_id, *turn_task_id, *child_agent_run_id),
    };

    // The session must exist and must belong to the named workspace. Both facts come
    // from one row, so a session that exists under a different workspace is a
    // mismatch rather than a pass.
    let session_workspace = match connection
        .query_row(
            "SELECT workspace_id FROM sessions WHERE id=?1",
            [session_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()
    {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return ScopeOwnershipV1::Unknown,
        Err(_) => return ScopeOwnershipV1::Unavailable,
    };
    if session_workspace != workspace_id.to_string() {
        return ScopeOwnershipV1::Mismatched;
    }

    // The turn task must exist, belong to this session, and say whether it is live.
    let turn = match connection
        .query_row(
            "SELECT session_id, state FROM turn_tasks WHERE id=?1",
            [turn_task_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
    {
        Ok(Some(turn)) => turn,
        Ok(None) => return ScopeOwnershipV1::Unknown,
        Err(_) => return ScopeOwnershipV1::Unavailable,
    };
    if turn.0 != session_id.to_string() {
        return ScopeOwnershipV1::Mismatched;
    }
    // A terminated turn task is real, finished work. That is deliberately distinct
    // from a forged scope: the ownership was genuine and the request is stale.
    if turn_task_has_ended(&turn.1) {
        return ScopeOwnershipV1::TaskEnded;
    }

    // The agent run must exist and belong to this turn task.
    let run_turn = match connection
        .query_row(
            "SELECT turn_task_id FROM agent_runs WHERE id=?1",
            [agent_run_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()
    {
        Ok(Some(turn_task)) => turn_task,
        Ok(None) => return ScopeOwnershipV1::Unknown,
        Err(_) => return ScopeOwnershipV1::Unavailable,
    };
    if run_turn != turn_task_id.to_string() {
        return ScopeOwnershipV1::Mismatched;
    }

    ScopeOwnershipV1::Owned
}

/// Whether a turn task's recorded state means the work has finished.
///
/// Matched on the schema's own vocabulary rather than a substring test, so an
/// unknown future state is treated as **live** rather than silently completed — the
/// safe direction, because treating live work as ended would refuse a valid call.
fn turn_task_has_ended(state: &str) -> bool {
    matches!(state, "completed" | "cancelled" | "ended" | "failed" | "terminated")
}

/// What a display may show as stale, and never what a dispatch may act on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProjectionFact {
    pub(crate) revision: u64,
    pub(crate) observed_at: UtcTimestamp,
    /// Where the fact came from. A successful projection read is the only source.
    pub(crate) source: &'static str,
}

/// Counters, read by regressions and the acceptance driver only.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct StoreCounters {
    pub(crate) refused: u64,
    pub(crate) unknown: u64,
    pub(crate) completed: u64,
    pub(crate) failed: u64,
    pub(crate) unavailable: u64,
    /// Reads really served by the read-only connection.
    pub(crate) reads_on_reader: u64,
    /// Reads served by the write connection because no reader could be opened.
    pub(crate) reads_on_writer: u64,
    /// Writes that really committed.
    pub(crate) writes_committed: u64,
}

#[derive(Clone, Copy, Default)]
struct Counters {
    refused: u64,
    unknown: u64,
    completed: u64,
    failed: u64,
    unavailable: u64,
    reads_on_reader: u64,
    reads_on_writer: u64,
    writes_committed: u64,
}

/// WHY THE CLASSIFICATION IS BY TYPE
/// A database-level failure is the dependency failing to serve the operation - a busy writer, a
/// permission change, a damaged file - and every dependent operation is honestly degraded.
/// `Conflict`, `Invalid`, `Json` and `Create` are answers *about this operation*: a revision
/// that moved, a guard that no longer holds, a request that is not valid. Turning one of those
/// into a process-wide fact is what once made one run's stale guard refuse every other
/// session's status.
#[must_use]
fn shared_failure(error: &PersistenceCoreError) -> Option<(&'static str, String)> {
    match error {
        PersistenceCoreError::Sqlite(failure) => {
            let code = failure
                .sqlite_error_code()
                .and_then(SqliteFailureClass::of)
                .map_or("sqlite.operation_failed", SqliteFailureClass::as_str);
            Some((code, error.to_string()))
        }
        _ => None,
    }
}

#[must_use]
fn classify(operation: &'static str, error: PersistenceCoreError) -> StoreError {
    match shared_failure(&error) {
        Some(_) => StoreError::Unavailable {
            operation,
            snapshot: StoreHealthSnapshot {
                dependency: JOURNAL,
                available: false,
                code: None,
                detail: Some(error.to_string()),
                observed_at: None,
                revision: 0,
                failures: 0,
                cleared: 0,
            },
            last_known: None,
        },
        None => StoreError::Failed { operation, error },
    }
}

/// What one bounded critical section's work really answered.
///
/// The distinction is by **type**, never by a message: the error is downcast back to
/// [`PersistenceCoreError`] when it is one, so the store's own classification still applies, and
/// anything else is the caller's own operation-level answer and is returned untouched.
enum SectionAnswer {
    Store(PersistenceCoreError),
    Owned(anyhow::Error),
}

impl SectionAnswer {
    /// Classifies the section's answer by the error's own type.
    #[must_use]
    fn of(error: anyhow::Error) -> Self {
        match error.downcast::<PersistenceCoreError>() {
            Ok(store) => Self::Store(store),
            Err(other) => Self::Owned(other),
        }
    }
}

/// The one Store owner.
pub(crate) struct AgentStore {
    journal: Arc<crate::broker::BrokerJournal>,
    /// A real read-only connection to the same database, when one could be opened.
    reader: Option<Arc<Mutex<Connection>>>,
    /// Why there is no reader, as a fact rather than a silence.
    reader_absent: Mutex<Option<String>>,
    boundary: SyncBoundary,
    health: health::StoreHealth,
    diagnostics: health::RunDiagnostics,
    receipts: Mutex<Receipts>,
    projections: Mutex<BTreeMap<AgentRunId, ProjectionFact>>,
    counters: Arc<Mutex<Counters>>,
    /// Regression/acceptance hold point, default off. It parks the **real work on the blocking
    /// thread inside the target production call**, matched by run/operation identity.
    ///
    /// Debug-only, matching the persistence barrier it binds to: a release build has no hold point
    /// to park at and carries no such field.
    #[cfg(debug_assertions)]
    hold: Mutex<Option<aura_persistence::barrier::Barrier>>,
}

/// The label a store hold matches on.
const HOLD_LABEL: &str = "agent_store";

impl AgentStore {
    /// The one owner. The reader is opened here, once, and its absence is recorded.
    #[must_use]
    pub(crate) fn new(journal: Arc<crate::broker::BrokerJournal>, boundary: SyncBoundary) -> Self {
        let (reader, absent) = match journal.database_path() {
            Ok(path) => match aura_persistence_core::open_reader(&path) {
                Ok(connection) => (Some(Arc::new(Mutex::new(connection))), None),
                Err(error) => (None, Some(format!("open_reader: {error}"))),
            },
            Err(error) => (None, Some(format!("database_path: {error}"))),
        };
        Self {
            journal,
            reader,
            reader_absent: Mutex::new(absent),
            boundary,
            health: health::StoreHealth::new(),
            diagnostics: health::RunDiagnostics::default(),
            receipts: Mutex::new(Receipts::default()),
            projections: Mutex::new(BTreeMap::new()),
            counters: Arc::new(Mutex::new(Counters::default())),
            #[cfg(debug_assertions)]
            hold: Mutex::new(None),
        }
    }

    /// The same journal authority the rest of the Core holds. Never a second authority.
    #[must_use]
    pub(crate) fn journal(&self) -> &Arc<crate::broker::BrokerJournal> {
        &self.journal
    }

    #[allow(dead_code)] // the dependency fact a display or a regression reads
    #[must_use]
    pub(crate) fn health(&self, dependency: StoreDependency) -> StoreHealthSnapshot {
        self.health.snapshot(dependency)
    }

    #[must_use]
    pub(crate) fn diagnostics(&self) -> &health::RunDiagnostics {
        &self.diagnostics
    }

    /// The retained result of a write whose caller stopped waiting for it.
    #[must_use]
    pub(crate) fn receipt(&self, id: u64) -> Option<StoreReceipt> {
        self.receipts.lock().ok().and_then(|receipts| receipts.entries.get(&id).cloned())
    }

    /// Every retained receipt, for a regression that has to observe one from outside the caller.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn receipts(&self) -> Vec<StoreReceipt> {
        self.receipts
            .lock()
            .map(|receipts| receipts.entries.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Regression-only: run every bounded section on the calling thread.
    ///
    /// This is exactly the behaviour that existed before the boundary existed, injected so the R1
    /// isolation regression can show that the property it measures really comes from the boundary
    /// and not from the shape of the call sites. It is never reachable from a product path.
    #[cfg(test)]
    pub(crate) fn force_inline_for_test(&self, inline: bool) {
        self.boundary.force_inline_for_test(inline);
    }

    /// Why reads are not on their own connection, when they are not.
    #[allow(dead_code)] // a startup fact a regression and a diagnostic read
    #[must_use]
    pub(crate) fn reader_absent(&self) -> Option<String> {
        self.reader_absent.lock().ok().and_then(|absent| absent.clone())
    }

    /// Verifies a scope's task ownership against the **authoritative** agent store,
    /// on a connection the caller already holds.
    ///
    /// The scoped tool read used to check that a session and a turn
    /// task existed in the **persistence** database, which is not where task ownership
    /// lives: the journal owns `sessions`, `turn_tasks` and `agent_runs`, and the two
    /// databases are different files. The check therefore answered a different question
    /// than the one that matters, and the real owner could not satisfy it at all.
    ///
    /// # Why this takes a connection instead of using [`Self::read`]
    ///
    /// Its production caller runs **inside** the Agent Store's critical section, which
    /// already holds the journal lock. Going through the asynchronous `read` entry from
    /// there would hold that lock across an await and wait on the same bounded capacity
    /// it is already inside — a lock-order and capacity hazard. Taking the caller's own
    /// connection keeps the authority and the lock order unchanged.
    ///
    /// It **writes nothing** and creates no second task record: it answers about
    /// ownership that already exists.
    ///
    /// # What it establishes, and what it does not
    ///
    /// It establishes that the scope's ids really belong together and whether the turn
    /// task is still live. It is **not** a permission decision: a live task says nothing
    /// about whether a tool may run, which is read separately from the coordinator's
    /// enablement state.
    pub(crate) fn verify_scope_ownership_on(
        &self,
        connection: &aura_persistence_core::Connection,
        scope: &aura_tool_contract::ToolRegistryScopeV1,
    ) -> aura_persistence::tool_entities::ScopeOwnershipV1 {
        verify_scope_ownership_in(connection, scope)
    }

    #[must_use]
    pub(crate) fn last_projection(&self, run: AgentRunId) -> Option<ProjectionFact> {
        self.projections.lock().ok().and_then(|facts| facts.get(&run).copied())
    }

    /// One successful projection read, as the display fact it is.
    pub(crate) fn remember_projection(&self, run: AgentRunId, revision: u64) {
        if let Ok(mut facts) = self.projections.lock() {
            facts.insert(
                run,
                ProjectionFact {
                    revision,
                    observed_at: UtcTimestamp::now(),
                    source: "agent.status",
                },
            );
        }
    }

    #[allow(dead_code)] // measured by the regression tests and the acceptance driver
    #[must_use]
    pub(crate) fn counters(&self) -> StoreCounters {
        let counters = self.counters.lock().map(|counters| *counters).unwrap_or_default();
        StoreCounters {
            refused: counters.refused,
            unknown: counters.unknown,
            completed: counters.completed,
            failed: counters.failed,
            unavailable: counters.unavailable,
            reads_on_reader: counters.reads_on_reader,
            reads_on_writer: counters.reads_on_writer,
            writes_committed: counters.writes_committed,
        }
    }

    /// Arms one hold inside the real store work, matching on the step identity.
    ///
    /// Default off, and reached only by a regression or an acceptance driver: with no hold
    /// armed this is one uncontended lock and the work runs untouched.
    #[allow(dead_code)] // armed by the regression tests; no product path calls it
    #[must_use]
    #[cfg(debug_assertions)]
    pub(crate) fn arm_hold(
        &self,
        select: &str,
        step_millis: u64,
    ) -> aura_persistence::barrier::Barrier {
        let barrier = aura_persistence::barrier::Barrier::arm_selected(
            aura_persistence::barrier::Point::WriterExecution,
            Some(HOLD_LABEL),
            Some(select),
            step_millis,
        );
        if let Ok(mut hold) = self.hold.lock() {
            *hold = Some(barrier.clone());
        }
        barrier
    }

    #[cfg(debug_assertions)]
    fn hold_slot(&self) -> Option<aura_persistence::barrier::Barrier> {
        self.hold.lock().ok().and_then(|hold| hold.clone())
    }

    /// One bounded read of the authoritative store.
    pub(crate) async fn read<T, F>(
        self: &Arc<Self>,
        step: StoreStep,
        work: F,
    ) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, PersistenceCoreError> + Send + 'static,
    {
        let reader = self.reader.clone();
        let journal = Arc::clone(&self.journal);
        let identity = step.identity();
        #[cfg(debug_assertions)]
        let hold = self.hold_slot();
        let counters = Arc::clone(&self.counters);
        let on_reader = reader.is_some();
        let work = move || -> Result<T, PersistenceCoreError> {
            #[cfg(debug_assertions)]
            let _permit = hold.and_then(|barrier| {
                barrier.arrive_identified(
                    aura_persistence::barrier::Point::WriterExecution,
                    HOLD_LABEL,
                    Some(&identity),
                )
            });
            let value = match &reader {
                Some(reader) => {
                    let guard = reader
                        .lock()
                        .map_err(|_| PersistenceCoreError::Invalid("agent.store_reader_lock"))?;
                    work(&guard)?
                }
                None => {
                    let store = journal.lock()?;
                    work(store.database.connection())?
                }
            };
            if let Ok(mut counters) = counters.lock() {
                if on_reader {
                    counters.reads_on_reader = counters.reads_on_reader.saturating_add(1);
                } else {
                    counters.reads_on_writer = counters.reads_on_writer.saturating_add(1);
                }
            }
            Ok(value)
        };
        self.section(step, SyncSection::JournalRead, StoreAccess::Read, work, None).await
    }

    /// One bounded transaction of the authoritative store.
    pub(crate) async fn write<T, F>(
        self: &Arc<Self>,
        step: StoreStep,
        work: F,
    ) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&mut V2Store) -> Result<T, PersistenceCoreError> + Send + 'static,
    {
        self.write_retained(step, work).await.map(|(value, _receipt)| value)
    }

    /// The same write, with a receipt a caller can query after its own wait ended.
    pub(crate) async fn write_retained<T, F>(
        self: &Arc<Self>,
        step: StoreStep,
        work: F,
    ) -> Result<(T, Option<u64>), StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&mut V2Store) -> Result<T, PersistenceCoreError> + Send + 'static,
    {
        let receipt = self.receipts.lock().ok().map(|mut receipts| receipts.open(step));
        let journal = Arc::clone(&self.journal);
        let identity = step.identity();
        #[cfg(debug_assertions)]
        let hold = self.hold_slot();
        let work = move || -> Result<T, PersistenceCoreError> {
            #[cfg(debug_assertions)]
            let _permit = hold.and_then(|barrier| {
                barrier.arrive_identified(
                    aura_persistence::barrier::Point::WriterExecution,
                    HOLD_LABEL,
                    Some(&identity),
                )
            });
            let mut store = journal.lock()?;
            work(&mut store)
        };
        self.section(step, SyncSection::AgentStore, StoreAccess::Write, work, receipt)
            .await
            .map(|value| (value, receipt))
    }

    /// One bounded section that owns a **whole synchronous critical section the caller names**.
    ///
    /// WHY THIS EXISTS
    /// Several Agent steps are not "one SQL statement". They are a section that takes more than
    /// one lock in a fixed order - the session permission authority first, then the journal - and
    /// that must stay one indivisible section: splitting the authority read from the commit turns
    /// the recorded revision into a snapshot that can already be stale. Those sections cannot
    /// await inside (the authority barrier hands the callback a `&mut` view while it holds the
    /// lock), so the only correct move is to run the section **whole** on this bounded owner
    /// instead of on the reactor worker that happened to call it.
    ///
    /// WHAT IT KEEPS
    /// * the same pool, the same admission and the same budget as every other Agent step: this is
    ///   not a second pool, and it is not an unbounded `spawn_blocking`;
    /// * the caller's own answer, by type. A `PersistenceCoreError` keeps its class (a damaged
    ///   database is still a dependency fact, an operation-level answer stays with its
    ///   operation); any other error is handed back unchanged rather than flattened into a store
    ///   error and parsed out of a message;
    /// * the refusal/unknown distinction. `agent.store_refused` means the section provably did not
    ///   run; `agent.store_unknown` means it started and its result is owned by the section.
    pub(crate) async fn run_critical<T, F>(
        self: &Arc<Self>,
        step: StoreStep,
        access: StoreAccess,
        work: F,
    ) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    {
        let operation = step.operation;
        let receipt = self.receipts.lock().ok().map(|mut receipts| receipts.open(step));
        #[cfg(debug_assertions)]
        let hold = self.hold_slot();
        let identity = step.identity();
        let settle_owner = Arc::clone(self);
        let section = match access {
            StoreAccess::Read => SyncSection::JournalRead,
            StoreAccess::Write => SyncSection::AgentStore,
        };
        let admission = self
            .boundary
            .run_settled(
                section,
                SyncSubject::new().request(step.identity()).method(operation),
                move || -> Result<T, SectionAnswer> {
                    #[cfg(debug_assertions)]
                    let _permit = hold.and_then(|barrier| {
                        barrier.arrive_identified(
                            aura_persistence::barrier::Point::WriterExecution,
                            HOLD_LABEL,
                            Some(&identity),
                        )
                    });
                    work().map_err(SectionAnswer::of)
                },
                move |result: &Result<T, SectionAnswer>| match result {
                    Ok(_) => settle_owner.settle_success(access, receipt),
                    Err(SectionAnswer::Store(error)) => {
                        settle_owner.settle_failure(access, receipt, error);
                    }
                    Err(SectionAnswer::Owned(_)) => settle_owner.settle_owned(receipt),
                },
            )
            .await;
        match admission {
            BlockingAdmission::Completed(Ok(value)) => Ok(value),
            BlockingAdmission::Completed(Err(SectionAnswer::Store(error))) => {
                Err(anyhow::Error::new(self.failure(operation, step, error)))
            }
            BlockingAdmission::Completed(Err(SectionAnswer::Owned(error))) => Err(error),
            BlockingAdmission::Refused { waited_ms } => {
                if let Some(receipt) = receipt
                    && let Ok(mut receipts) = self.receipts.lock()
                {
                    receipts.settle(receipt, ReceiptState::NeverStarted);
                }
                self.bump(|counters| counters.refused = counters.refused.saturating_add(1));
                Err(anyhow::Error::new(StoreError::Refused { operation, waited_ms }))
            }
            BlockingAdmission::Unknown { waited_ms } => {
                self.bump(|counters| counters.unknown = counters.unknown.saturating_add(1));
                Err(anyhow::Error::new(StoreError::Unknown { operation, waited_ms, receipt }))
            }
        }
    }

    /// The one bounded section: admitted work, an owned permit and an owned result.
    async fn section<T, F>(
        self: &Arc<Self>,
        step: StoreStep,
        section: SyncSection,
        access: StoreAccess,
        work: F,
        receipt: Option<u64>,
    ) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, PersistenceCoreError> + Send + 'static,
    {
        let operation = step.operation;
        let store = Arc::clone(self);
        let admission = self
            .boundary
            .run_settled(
                section,
                SyncSubject::new().request(step.identity()).method(operation),
                work,
                move |result: &Result<T, PersistenceCoreError>| {
                    store.settle(operation, access, receipt, result);
                },
            )
            .await;
        match admission {
            BlockingAdmission::Completed(Ok(value)) => Ok(value),
            BlockingAdmission::Completed(Err(error)) => Err(self.failure(operation, step, error)),
            BlockingAdmission::Refused { waited_ms } => {
                if let Some(receipt) = receipt
                    && let Ok(mut receipts) = self.receipts.lock()
                {
                    receipts.settle(receipt, ReceiptState::NeverStarted);
                }
                self.bump(|counters| counters.refused = counters.refused.saturating_add(1));
                Err(StoreError::Refused { operation, waited_ms })
            }
            BlockingAdmission::Unknown { waited_ms } => {
                self.bump(|counters| counters.unknown = counters.unknown.saturating_add(1));
                Err(StoreError::Unknown { operation, waited_ms, receipt })
            }
        }
    }

    /// The real result, recorded by the thread that really ran the work.
    fn settle<T>(
        self: &Arc<Self>,
        operation: &'static str,
        access: StoreAccess,
        receipt: Option<u64>,
        result: &Result<T, PersistenceCoreError>,
    ) {
        let _ = operation;
        match result {
            Ok(_) => self.settle_success(access, receipt),
            Err(error) => self.settle_failure(access, receipt, error),
        }
    }

    /// A section whose work really finished and committed.
    fn settle_success(&self, access: StoreAccess, receipt: Option<u64>) {
        self.health.observe_success(JOURNAL, access);
        self.bump(|counters| {
            counters.completed = counters.completed.saturating_add(1);
            if access == StoreAccess::Write {
                counters.writes_committed = counters.writes_committed.saturating_add(1);
            }
        });
        if let Some(receipt) = receipt
            && let Ok(mut receipts) = self.receipts.lock()
        {
            receipts.settle(receipt, ReceiptState::Settled { committed: true, code: None });
        }
    }

    /// A section whose work really finished and answered with a store error.
    fn settle_failure(
        &self,
        access: StoreAccess,
        receipt: Option<u64>,
        error: &PersistenceCoreError,
    ) {
        let shared = shared_failure(error);
        match &shared {
            Some((code, detail)) => {
                self.health.observe_unavailable(JOURNAL, access, code, detail.clone());
                self.bump(|counters| {
                    counters.unavailable = counters.unavailable.saturating_add(1);
                });
            }
            None => {
                self.bump(|counters| counters.failed = counters.failed.saturating_add(1));
            }
        }
        if let Some(receipt) = receipt
            && let Ok(mut receipts) = self.receipts.lock()
        {
            receipts.settle(
                receipt,
                ReceiptState::Settled {
                    committed: false,
                    code: shared.as_ref().map(|(code, _)| *code),
                },
            );
        }
    }

    /// A section whose work really finished and answered with the caller's own operation error.
    fn settle_owned(&self, receipt: Option<u64>) {
        self.bump(|counters| counters.failed = counters.failed.saturating_add(1));
        if let Some(receipt) = receipt
            && let Ok(mut receipts) = self.receipts.lock()
        {
            receipts.settle(receipt, ReceiptState::Settled { committed: false, code: None });
        }
    }

    /// The typed answer for a real store error of one operation.
    fn failure(
        &self,
        operation: &'static str,
        step: StoreStep,
        error: PersistenceCoreError,
    ) -> StoreError {
        match classify(operation, error) {
            StoreError::Unavailable { operation, .. } => StoreError::Unavailable {
                operation,
                snapshot: self.health.snapshot(JOURNAL),
                last_known: step.run_id().and_then(|run| self.last_projection(run)),
            },
            other => other,
        }
    }

    fn bump(&self, update: impl FnOnce(&mut Counters)) {
        if let Ok(mut counters) = self.counters.lock() {
            update(&mut counters);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_classification_is_by_type_and_never_by_message() {
        let corrupt = classify(
            "agent.step",
            PersistenceCoreError::Sqlite(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
                Some("database disk image is malformed".into()),
            )),
        );
        match corrupt {
            StoreError::Unavailable { snapshot, .. } => {
                assert_eq!(snapshot.code, None, "the snapshot is refilled from live health");
            }
            other => panic!("a damaged store is a dependency fact: {other}"),
        }
        assert_eq!(
            shared_failure(&PersistenceCoreError::Sqlite(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                Some("database is locked".into()),
            )))
            .map(|(code, _)| code),
            Some("sqlite.busy")
        );
        for local in [
            PersistenceCoreError::Conflict,
            PersistenceCoreError::Invalid("agent.mode_switch_requires_idle"),
        ] {
            assert!(
                shared_failure(&local).is_none(),
                "an operation-level answer must not become a dependency fact"
            );
            assert!(matches!(classify("agent.step", local), StoreError::Failed { .. }));
        }
    }

    #[test]
    fn a_write_success_clears_a_write_fault_and_a_read_success_does_not_hide_it() {
        let health = health::StoreHealth::new();
        health.observe_unavailable(JOURNAL, StoreAccess::Write, "sqlite.busy", "locked".into());
        assert!(!health.snapshot(JOURNAL).available);
        // In WAL a reader is served while the writer is blocked: that must not clear the fact.
        health.observe_success(JOURNAL, StoreAccess::Read);
        let still_failed = health.snapshot(JOURNAL);
        assert!(!still_failed.available, "{still_failed:?}");
        assert_eq!(still_failed.cleared, 0);
        // A write that really committed does clear it, with the text that produced it.
        health.observe_success(JOURNAL, StoreAccess::Write);
        let cleared = health.snapshot(JOURNAL);
        assert!(cleared.available);
        assert_eq!(cleared.cleared, 1);
        assert_eq!(cleared.code, None, "a recovered dependency is not described by its old error");
        assert!(cleared.revision > still_failed.revision);
    }
}
