use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{Arc, atomic::Ordering, mpsc},
    time::{Duration, Instant},
};

use aura_domain_common::SessionId;
use aura_workspace::{BufferStore, PathCatalog, SecurePathResolver};
use aura_workspace_domain::{
    FsSearchIncompleteReason, FsSearchPattern, FsSearchTextRequest, FsSearchTextResult,
    LogicalFileSnapshot,
};
use grep_regex::RegexMatcher;

use super::{
    budget::ByteBudget,
    cancel::{SearchCancel, SearchCompletion},
    candidates::SearchCandidate,
    classify::{ClassifiedWorkspace, classify_workspace},
    commit::{self, Collected, CommitState, RunStats},
    diagnostics::{StageDiagnosticsHandle, StageTimer, observe_high_water},
    disk::{DiskScanRequest, scan_disk},
    matcher::compile_matcher,
    overlay::{OverlayScanRequest, scan_overlay},
    policy::{
        HARD_DEADLINE, MAX_FILE_BYTES, MAX_PATTERN_BYTES, MAX_RESULTS, PROBE_BYTES,
        default_deadline,
    },
    pool::{self, FairPool},
    types::{EngineDiagnostics, FileOutcome, completion},
};
use crate::FsToolError;

/// Extra uncommitted dispatches allowed beyond the pool's own capacity, so a
/// slow early candidate cannot idle the workers while the ordered commit window
/// still bounds how many outcomes may pile up.
const COMMIT_WINDOW_HEADROOM: usize = 8;

pub(crate) fn execute(
    catalog: &PathCatalog,
    buffers: &Arc<BufferStore>,
    resolver: &SecurePathResolver,
    request: &FsSearchTextRequest,
    cancel: &SearchCancel,
    stages: &StageDiagnosticsHandle,
) -> Result<(FsSearchTextResult, EngineDiagnostics), FsToolError> {
    // Whatever this search does from here — success, early validation error or a
    // propagated failure — the handle the caller registered must end up
    // reporting the scan as over. That signal is what lets a connection whose
    // tasks are already gone still reclaim the work it started.
    let _completion = SearchCompletion::arm(cancel);
    let started = Instant::now();
    let (literal, pattern_text, case_sensitive) = match &request.pattern {
        FsSearchPattern::Literal { text, case_sensitive } => (true, text.as_str(), *case_sensitive),
        FsSearchPattern::Regex { pattern, case_sensitive } => {
            (false, pattern.as_str(), *case_sensitive)
        }
    };
    if pattern_text.is_empty() || pattern_text.len() > MAX_PATTERN_BYTES {
        return Err(FsToolError::InvalidRequest(
            "search pattern must be 1..=4096 UTF-8 bytes".into(),
        ));
    }
    if request.limit == 0 || request.limit > MAX_RESULTS {
        return Err(FsToolError::InvalidRequest("search limit must be 1..=100".into()));
    }
    let limit = usize::try_from(request.limit).unwrap_or(MAX_RESULTS as usize);
    let matcher = compile_matcher(literal, pattern_text, case_sensitive)?;
    if cancel.is_cancelled() {
        return Err(FsToolError::Cancelled);
    }
    let (mut entries, catalog_revision) = {
        let _catalog = StageTimer::start(&stages.catalog_micros);
        catalog.snapshot()?
    };
    // Folders the index did not enter are read from disk, within a budget, as candidates.
    let unindexed = catalog.read_unindexed(resolver, &request.root, true)?;
    entries.extend(unindexed.entries);
    let candidates = freeze_candidates_from(entries, request)?;
    stages
        .catalog_candidates
        .fetch_add(u64::try_from(candidates.len()).unwrap_or(u64::MAX), Ordering::Relaxed);
    // One snapshot of this session's writable buffers. Candidates outside the
    // set are scanned from disk without a per-file buffer probe.
    let buffer_paths = {
        let _probe = StageTimer::start(&stages.buffer_probe_micros);
        buffers
            .writable_search_paths(request.workspace_id, request.session_id)?
            .into_iter()
            .collect::<BTreeSet<_>>()
    };
    stages
        .buffer_snapshots
        .fetch_add(u64::try_from(buffer_paths.len()).unwrap_or(u64::MAX), Ordering::Relaxed);
    let budget = Arc::new(ByteBudget::new());
    let run = SearchRun {
        buffers: Arc::clone(buffers),
        resolver: resolver.clone(),
        session_id: request.session_id,
        matcher,
        budget,
        user_cancel: cancel.clone(),
        match_cap: limit,
        buffer_paths,
        stages: Arc::clone(stages),
    };
    let (collected, stats) = run_workers(&run, &candidates, started, limit)?;
    if cancel.is_cancelled() {
        return Err(FsToolError::Cancelled);
    }
    if !stats.is_balanced(candidates.len()) {
        // Accounting is an invariant, not a best effort: a mismatch means an
        // ordinal was counted twice or dropped, which must fail loudly instead
        // of surfacing as a plausible-looking statistic.
        return Err(FsToolError::Internal(format!(
            "search accounting attributed {} ordinals for {} candidates",
            stats.attributed(),
            candidates.len()
        )));
    }
    let (matches, reasons) = {
        let _commit = StageTimer::start(&stages.commit_micros);
        let mut reasons = stats.reasons.clone();
        let (matches, limit_reason) = collect_result(collected, limit);
        if let Some(reason) = limit_reason
            && !reasons.contains(&reason)
        {
            reasons.push(reason);
        }
        (matches, reasons)
    };
    if stats.matched_files > 0 {
        stages.matched_files.fetch_add(stats.matched_files, Ordering::Relaxed);
    }
    let diagnostics = EngineDiagnostics {
        workspace_id: request.workspace_id,
        session_id: request.session_id,
        elapsed_ms: started.elapsed().as_millis(),
        cancelled: false,
    };
    let skipped_scopes = crate::unindexed::skipped(resolver, &unindexed.unfinished);
    let mut completion = completion(reasons);
    if !skipped_scopes.is_empty() {
        crate::unindexed::partial(&mut completion);
    }
    Ok((
        FsSearchTextResult {
            skipped_scopes,
            catalog_revision,
            matches,
            completion,
            candidate_file_count: u64::try_from(candidates.len()).unwrap_or(u64::MAX),
            scanned_file_count: stats.scanned_file_count,
            scanned_bytes: stats.scanned_bytes,
            skipped_file_count: stats.skipped_file_count,
            // A workspace search reads the workspace index, so it names no external
            // target.
            external: None,
        },
        diagnostics,
    ))
}

fn freeze_candidates_from(
    entries: Vec<aura_workspace::CatalogEntry>,
    request: &FsSearchTextRequest,
) -> Result<Vec<SearchCandidate>, FsToolError> {
    super::candidates::freeze_candidates(
        entries,
        &request.root,
        request.path_glob.as_deref(),
        request.include_ignored,
        request.include_generated,
    )
}

struct SearchRun {
    buffers: Arc<BufferStore>,
    resolver: SecurePathResolver,
    session_id: SessionId,
    matcher: RegexMatcher,
    budget: Arc<ByteBudget>,
    user_cancel: SearchCancel,
    match_cap: usize,
    buffer_paths: BTreeSet<aura_domain_common::WorkspacePath>,
    stages: StageDiagnosticsHandle,
}

#[derive(Debug)]
enum ScanReply {
    Outcome(FileOutcome),
    Fatal(FsToolError),
}

enum PreparedSource {
    Buffer(LogicalFileSnapshot),
    Disk,
}

struct ReadyJob {
    candidate: SearchCandidate,
    reserved: u64,
    source: PreparedSource,
}

fn run_workers(
    run: &SearchRun,
    candidates: &[SearchCandidate],
    started: Instant,
    limit: usize,
) -> Result<(Vec<Collected>, RunStats), FsToolError> {
    let mut stats = RunStats::default();
    if candidates.is_empty() {
        return Ok((Vec::new(), stats));
    }
    let window =
        pool::MAX_ACTIVE.saturating_add(pool::QUEUE_BOUND).saturating_add(COMMIT_WINDOW_HEADROOM);
    let (tx, rx) = mpsc::sync_channel(window.max(1));
    let halt = SearchCancel::new();
    let mut next = 0_usize;
    let mut pending: Option<ReadyJob> = None;
    let mut in_flight = 0_usize;
    let mut finished: BTreeMap<usize, FileOutcome> = BTreeMap::new();
    let mut collected: Vec<Collected> = Vec::new();
    let mut commit = CommitState::new(window);
    let mut first_fatal = None;

    loop {
        if run.user_cancel.is_cancelled() {
            halt.cancel();
            // alse means real work outlived this bounded wait; it keeps its registration so nobody reports it as reclaimed (see SearchCancel::is_finished).
            let _reclaimed =
                drain_inflight(&rx, &mut in_flight, &mut finished, &mut first_fatal, started);
            release_pending(&run.budget, pending.take());
            return Err(first_fatal.unwrap_or(FsToolError::Cancelled));
        }
        let elapsed = started.elapsed();
        if elapsed >= HARD_DEADLINE || elapsed >= default_deadline() {
            halt.cancel();
            // alse means real work outlived this bounded wait; it keeps its registration so nobody reports it as reclaimed (see SearchCancel::is_finished).
            let _reclaimed =
                drain_inflight(&rx, &mut in_flight, &mut finished, &mut first_fatal, started);
            if let Some(error) = first_fatal {
                release_pending(&run.budget, pending.take());
                return Err(error);
            }
            let pending_ordinal = pending.as_ref().map(|job| job.candidate.ordinal);
            release_pending(&run.budget, pending.take());
            commit::finish_rest(commit::FinishRequest {
                candidates: candidates.len(),
                pending_ordinal,
                dispatched: next,
                limit,
                finished: &mut finished,
                collected: &mut collected,
                stats: &mut stats,
                commit: &mut commit,
                reason: FsSearchIncompleteReason::TimeBudget,
            });
            return Ok((collected, stats));
        }
        // Publish every ordinal whose predecessors are already committed,
        // folding their statistics and keeping only result-window matches.
        let committed_any =
            commit::ready(limit, &mut finished, &mut collected, &mut stats, &mut commit);
        if commit.stop_dispatch {
            halt.cancel();
            // A prepared-but-undispatched candidate only releases its
            // reservation here; it belongs to the unprocessed tail and is
            // attributed once by `finish_rest`, never published on its own.
            let pending_ordinal = pending.as_ref().map(|job| job.candidate.ordinal);
            release_pending(&run.budget, pending.take());
            // alse means real work outlived this bounded wait; it keeps its registration so nobody reports it as reclaimed (see SearchCancel::is_finished).
            let _reclaimed =
                drain_inflight(&rx, &mut in_flight, &mut finished, &mut first_fatal, started);
            if let Some(error) = first_fatal {
                return Err(error);
            }
            commit::finish_rest(commit::FinishRequest {
                candidates: candidates.len(),
                pending_ordinal,
                dispatched: next,
                limit,
                finished: &mut finished,
                collected: &mut collected,
                stats: &mut stats,
                commit: &mut commit,
                reason: FsSearchIncompleteReason::ResultLimit,
            });
            return Ok((collected, stats));
        }
        if commit.commit_next >= candidates.len() {
            break;
        }
        if let Some(error) = first_fatal.take() {
            halt.cancel();
            // alse means real work outlived this bounded wait; it keeps its registration so nobody reports it as reclaimed (see SearchCancel::is_finished).
            let _reclaimed =
                drain_inflight(&rx, &mut in_flight, &mut finished, &mut first_fatal, started);
            release_pending(&run.budget, pending.take());
            return Err(error);
        }
        let mut progressed = committed_any;
        let window = commit.window;
        while !commit.stop_dispatch
            && next.saturating_sub(commit.commit_next) < window
            && next < candidates.len()
        {
            if pending.is_none() {
                match prepare_job(run, &candidates[next]) {
                    Ok(Prepare::Ready(job)) => pending = Some(job),
                    Ok(Prepare::Skip(outcome)) => {
                        // A preparation skip is part of the ordered commit
                        // stream, never a hole the window has to wait on. Its
                        // ordinal is finished too, so later workers are not left
                        // waiting for a turn that can no longer arrive.
                        run.budget.finish_ordinal(outcome.ordinal);
                        commit::park(&mut finished, outcome);
                        next += 1;
                        progressed = true;
                        continue;
                    }
                    Err(error) => {
                        halt.cancel();
                        let _ = drain_inflight(
                            &rx,
                            &mut in_flight,
                            &mut finished,
                            &mut first_fatal,
                            started,
                        );
                        release_pending(&run.budget, pending.take());
                        return Err(error);
                    }
                }
            }
            let Some(job) = pending.take() else {
                break;
            };
            match submit_job(run, &tx, &halt, job) {
                Ok(()) => {
                    next += 1;
                    in_flight += 1;
                    progressed = true;
                }
                Err(failure) => {
                    let closed = failure.is_closed();
                    let job = failure.into_job();
                    if closed {
                        release_pending(&run.budget, job);
                        halt.cancel();
                        let _ = drain_inflight(
                            &rx,
                            &mut in_flight,
                            &mut finished,
                            &mut first_fatal,
                            started,
                        );
                        return Err(FsToolError::Internal("search pool is closed".into()));
                    }
                    // Keep the prepared job and its reservation; wait for
                    // capacity instead of dropping the file.
                    pending = job;
                    break;
                }
            }
        }
        observe_high_water(&run.stages.window_high_water, next.saturating_sub(commit.commit_next));
        if in_flight == 0 {
            if commit.commit_next >= candidates.len() {
                break;
            }
            if !progressed {
                // The pool is saturated by other sessions. Wait for capacity
                // instead of reporting the file as unavailable, and stay
                // responsive to cancellation and the deadline.
                std::thread::sleep(Duration::from_millis(5));
            }
            continue;
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(ScanReply::Outcome(outcome)) => {
                in_flight = in_flight.saturating_sub(1);
                finished.insert(outcome.ordinal, outcome);
            }
            Ok(ScanReply::Fatal(error)) => {
                in_flight = in_flight.saturating_sub(1);
                first_fatal = Some(error);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if in_flight > 0 {
                    return Err(FsToolError::Internal(
                        "search worker channel closed before reclaim".into(),
                    ));
                }
                break;
            }
        }
    }
    halt.cancel();
    let _reclaimed = drain_inflight(&rx, &mut in_flight, &mut finished, &mut first_fatal, started);
    if let Some(error) = first_fatal {
        release_pending(&run.budget, pending.take());
        return Err(error);
    }
    let pending_ordinal = pending.as_ref().map(|job| job.candidate.ordinal);
    release_pending(&run.budget, pending.take());
    commit::finish_rest(commit::FinishRequest {
        candidates: candidates.len(),
        pending_ordinal,
        dispatched: next,
        limit,
        finished: &mut finished,
        collected: &mut collected,
        stats: &mut stats,
        commit: &mut commit,
        reason: FsSearchIncompleteReason::ResultLimit,
    });
    Ok((collected, stats))
}

/// Assemble the wire result: the first `limit` matches in candidate order. The
/// `ResultLimit` reason is returned separately so the caller can merge it with
/// the per-file reasons without duplicating it.
fn collect_result(
    collected: Vec<Collected>,
    limit: usize,
) -> (Vec<aura_workspace_domain::FsSearchTextMatch>, Option<FsSearchIncompleteReason>) {
    let mut matches = Vec::new();
    for item in collected {
        if matches.len() >= limit {
            break;
        }
        let remaining = limit - matches.len();
        if item.matches.len() > remaining {
            matches.extend(item.matches.into_iter().take(remaining));
        } else {
            matches.extend(item.matches);
        }
    }
    matches.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line_index.cmp(&right.line_index))
            .then(left.column_character_offset.cmp(&right.column_character_offset))
            .then(
                (left.column_character_offset + left.match_character_length)
                    .cmp(&(right.column_character_offset + right.match_character_length)),
            )
    });
    matches.dedup_by(|left, right| {
        left.path == right.path
            && left.line_index == right.line_index
            && left.column_character_offset == right.column_character_offset
            && left.match_character_length == right.match_character_length
    });
    if matches.len() > limit {
        matches.truncate(limit);
    }
    let limited =
        (matches.len() >= limit && limit > 0).then_some(FsSearchIncompleteReason::ResultLimit);
    (matches, limited)
}

enum Prepare {
    Ready(ReadyJob),
    Skip(FileOutcome),
}

fn prepare_job(run: &SearchRun, candidate: &SearchCandidate) -> Result<Prepare, FsToolError> {
    if run.user_cancel.is_cancelled() {
        return Ok(Prepare::Skip(FileOutcome {
            ordinal: candidate.ordinal,
            path: candidate.path.clone(),
            matches: Vec::new(),
            scanned: false,
            skipped: true,
            bytes: 0,
            reasons: Vec::new(),
        }));
    }
    let source = match peek_source(run, candidate)? {
        Peek::Source(source) => source,
        Peek::Skip(outcome) => return Ok(Prepare::Skip(outcome)),
    };
    // Only an editor-buffer snapshot carries an authoritative logical size
    // before dispatch. A catalog byte length is a planning hint that may be
    // stale in either direction, so it never decides `FileTooLarge`: the
    // worker applies the limit to the securely opened handle.
    let logical = match &source {
        PreparedSource::Buffer(snapshot) => u64::try_from(snapshot.bytes.len()).unwrap_or(u64::MAX),
        PreparedSource::Disk => candidate.byte_length.unwrap_or(MAX_FILE_BYTES),
    };
    if logical > MAX_FILE_BYTES && matches!(source, PreparedSource::Buffer(_)) {
        return Ok(Prepare::Skip(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            0,
            vec![FsSearchIncompleteReason::FileTooLarge],
        )));
    }
    // Reserve from the planning hint so later candidates cannot spend bytes an
    // earlier one still needs. The reservation covers the probe pass *and* the
    // full scan of a file no larger than the hint, so an in-order scan never has
    // to grow its allowance in the middle of a read; the re-hash (and a retry)
    // grow in ordinal order instead.
    let need = logical.clamp(1, MAX_FILE_BYTES).saturating_add(PROBE_BYTES as u64);
    let Some(reserved) = run.budget.try_reserve(need) else {
        return Ok(Prepare::Skip(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            0,
            vec![FsSearchIncompleteReason::ScanByteBudget],
        )));
    };
    Ok(Prepare::Ready(ReadyJob { candidate: candidate.clone(), reserved, source }))
}

enum Peek {
    Source(PreparedSource),
    Skip(FileOutcome),
}

fn peek_source(run: &SearchRun, candidate: &SearchCandidate) -> Result<Peek, FsToolError> {
    if crate::read_scope::current().is_some() {
        let info = run.resolver.resolve_existing(&candidate.path)?;
        if !crate::read_scope::allows(&info.info) {
            return Ok(Peek::Skip(FileOutcome::skipped(
                candidate.ordinal,
                candidate.path.clone(),
                0,
                vec![FsSearchIncompleteReason::ScopeExcluded],
            )));
        }
    }
    if !run.buffer_paths.contains(&candidate.path) {
        // No writable editor buffer for this session: the candidate is a disk
        // scan, so skip the per-file buffer probe entirely.
        return Ok(Peek::Source(PreparedSource::Disk));
    }
    let _probe = StageTimer::start(&run.stages.buffer_probe_micros);
    run.stages.buffer_probes.fetch_add(1, Ordering::Relaxed);
    match run.buffers.writable_search_snapshot(&run.resolver, run.session_id, &candidate.path) {
        Ok(Some(snapshot)) => Ok(Peek::Source(PreparedSource::Buffer(snapshot))),
        Ok(None) => Ok(Peek::Source(PreparedSource::Disk)),
        Err(aura_workspace::WorkspaceError::NotFound { .. }) => {
            Ok(Peek::Source(PreparedSource::Disk))
        }
        Err(error) => match classify_workspace(error) {
            ClassifiedWorkspace::Temporary => Ok(Peek::Skip(FileOutcome::skipped(
                candidate.ordinal,
                candidate.path.clone(),
                0,
                vec![FsSearchIncompleteReason::FileTemporarilyUnavailable],
            ))),
            ClassifiedWorkspace::Changed => Ok(Peek::Skip(FileOutcome::skipped(
                candidate.ordinal,
                candidate.path.clone(),
                0,
                vec![FsSearchIncompleteReason::FileChangedDuringSearch],
            ))),
            ClassifiedWorkspace::Fatal(error) => Err(error),
        },
    }
}

fn submit_job(
    run: &SearchRun,
    tx: &mpsc::SyncSender<ScanReply>,
    halt: &SearchCancel,
    job: ReadyJob,
) -> Result<(), SubmitFailure> {
    let pool = FairPool::global();
    let buffers = Arc::clone(&run.buffers);
    let resolver = run.resolver.clone();
    let matcher = run.matcher.clone();
    let budget = Arc::clone(&run.budget);
    let cancel = SearchCancel::merge(&run.user_cancel, halt);
    let session = run.session_id;
    let match_cap = run.match_cap;
    let worker_tx = tx.clone();
    let stages = Arc::clone(&run.stages);
    let scope = crate::read_scope::current();
    // The factory runs only when the pool admits the job. The cell carries the
    // job back out on rejection, so no file or reservation is ever lost.
    let slot: Rc<RefCell<Option<ReadyJob>>> = Rc::new(RefCell::new(Some(job)));
    let taken = Rc::clone(&slot);
    // Admitted work is counted before submission, so there is no window in which
    // a dispatched file is neither running nor tracked. The guard travels into
    // the worker and settles when that work really ends — including on panic —
    // or is dropped with the factory when the pool refuses the job.
    let work_guard = cancel.admit_work();
    let outcome = pool.try_submit(pool::Submission::new(
        session,
        move || {
            let job = taken.borrow_mut().take().expect("admitted submission owns the job");
            Ok(Box::new(move || {
                let _scope = scope.as_ref().map(crate::FileReadScope::install);
                let _work = work_guard;
                let reply = worker_reply(|| {
                    scan_prepared(ScanPrepared {
                        buffers: &buffers,
                        resolver: &resolver,
                        session_id: session,
                        matcher: &matcher,
                        budget: &budget,
                        cancel: &cancel,
                        match_cap,
                        stages: &stages,
                        job,
                    })
                });
                let _ = worker_tx.send(reply);
            }) as Box<dyn FnOnce() + Send>)
        },
        Rc::clone(&slot),
    ));
    match outcome {
        Ok(()) => Ok(()),
        Err(cell) => {
            if pool.is_shutdown() {
                Err(SubmitFailure::Closed(cell))
            } else {
                Err(SubmitFailure::AtCapacity(cell))
            }
        }
    }
}

/// Why a prepared job could not be handed to the pool. Both variants carry the
/// job back to the coordinator so no file or reservation is lost.
enum SubmitFailure {
    AtCapacity(RejectedJob),
    Closed(RejectedJob),
}

impl SubmitFailure {
    fn into_job(self) -> Option<ReadyJob> {
        match self {
            SubmitFailure::AtCapacity(cell) | SubmitFailure::Closed(cell) => {
                cell.borrow_mut().take()
            }
        }
    }

    fn is_closed(&self) -> bool {
        matches!(self, SubmitFailure::Closed(_))
    }
}

type RejectedJob = Rc<RefCell<Option<ReadyJob>>>;

fn worker_reply(work: impl FnOnce() -> Result<FileOutcome, FsToolError>) -> ScanReply {
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(Ok(outcome)) => ScanReply::Outcome(outcome),
        Ok(Err(error)) => ScanReply::Fatal(error),
        Err(_) => ScanReply::Fatal(FsToolError::Internal("search worker panicked".into())),
    }
}

struct ScanPrepared<'a> {
    buffers: &'a BufferStore,
    resolver: &'a SecurePathResolver,
    session_id: SessionId,
    matcher: &'a RegexMatcher,
    budget: &'a Arc<ByteBudget>,
    cancel: &'a SearchCancel,
    match_cap: usize,
    stages: &'a StageDiagnosticsHandle,
    job: ReadyJob,
}

fn scan_prepared(input: ScanPrepared<'_>) -> Result<FileOutcome, FsToolError> {
    let ScanPrepared {
        buffers,
        resolver,
        session_id,
        matcher,
        budget,
        cancel,
        match_cap,
        stages,
        job,
    } = input;
    let ReadyJob { candidate, reserved, source } = job;
    // Finishing the ordinal is a scope responsibility, not a success-path step:
    // an early error (buffer scan failure, propagated I/O error) must still end
    // the file's turn exactly once, and a panic must too.
    let _ordinal = OrdinalFinish::arm(budget, candidate.ordinal);
    #[allow(clippy::let_and_return)] // the ordinal guard must drop before returning
    let outcome = match source {
        PreparedSource::Buffer(snapshot) => {
            let revision = snapshot.buffer_revision;
            let outcome = scan_overlay(OverlayScanRequest {
                snapshot,
                candidate: &candidate,
                matcher,
                budget,
                reserved,
                cancel,
                match_cap,
                stages,
            })?;
            confirm_buffer_revision(buffers, resolver, session_id, &candidate, revision, outcome)
        }
        PreparedSource::Disk => scan_disk(DiskScanRequest {
            resolver,
            candidate: &candidate,
            matcher,
            budget,
            reserved,
            cancel,
            match_cap,
            stages,
        }),
    };
    outcome
}

/// Ends one file ordinal when the scan leaves scope, whatever the reason.
struct OrdinalFinish<'a> {
    budget: &'a ByteBudget,
    ordinal: usize,
}

impl<'a> OrdinalFinish<'a> {
    fn arm(budget: &'a ByteBudget, ordinal: usize) -> Self {
        Self { budget, ordinal }
    }
}

impl Drop for OrdinalFinish<'_> {
    fn drop(&mut self) {
        self.budget.finish_ordinal(self.ordinal);
    }
}

fn confirm_buffer_revision(
    buffers: &BufferStore,
    resolver: &SecurePathResolver,
    session_id: SessionId,
    candidate: &SearchCandidate,
    revision: Option<aura_domain_common::Revision>,
    outcome: FileOutcome,
) -> Result<FileOutcome, FsToolError> {
    let Some(revision) = revision else {
        return Ok(outcome);
    };
    match buffers.buffer_revision_matches(resolver, session_id, &candidate.path, revision) {
        Ok(true) => Ok(outcome),
        Ok(false) => Ok(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            outcome.bytes,
            vec![FsSearchIncompleteReason::FileChangedDuringSearch],
        )),
        Err(error) => match classify_workspace(error) {
            ClassifiedWorkspace::Temporary => Ok(FileOutcome::skipped(
                candidate.ordinal,
                candidate.path.clone(),
                outcome.bytes,
                vec![FsSearchIncompleteReason::FileTemporarilyUnavailable],
            )),
            ClassifiedWorkspace::Changed => Ok(FileOutcome::skipped(
                candidate.ordinal,
                candidate.path.clone(),
                outcome.bytes,
                vec![FsSearchIncompleteReason::FileChangedDuringSearch],
            )),
            ClassifiedWorkspace::Fatal(error) => Err(error),
        },
    }
}

/// Reap reported work, bounded. Returns whether every dispatched worker reported
/// back.
///
/// alse means real work is still running: the coordinator must not present
/// that as a finished search. The work keeps its registration (the pool is still
/// inside synchronous I/O), so whoever owns the search is told about it instead
/// of being told it is gone.
fn drain_inflight(
    rx: &mpsc::Receiver<ScanReply>,
    in_flight: &mut usize,
    finished: &mut BTreeMap<usize, FileOutcome>,
    first_fatal: &mut Option<FsToolError>,
    started: Instant,
) -> bool {
    while *in_flight > 0 {
        if started.elapsed() >= HARD_DEADLINE + Duration::from_secs(2) {
            if first_fatal.is_none() {
                *first_fatal = Some(FsToolError::Internal("search workers did not reclaim".into()));
            }
            return false;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(ScanReply::Outcome(outcome)) => {
                finished.insert(outcome.ordinal, outcome);
                *in_flight = in_flight.saturating_sub(1);
            }
            Ok(ScanReply::Fatal(error)) => {
                *in_flight = in_flight.saturating_sub(1);
                if first_fatal.is_none() {
                    *first_fatal = Some(error);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // The channel is gone, so no further report can arrive for the
                // work still counted as in flight.
                return *in_flight == 0;
            }
        }
    }
    true
}

fn release_pending(budget: &ByteBudget, pending: Option<ReadyJob>) {
    if let Some(job) = pending {
        // The candidate was prepared but never handed to the pool: give its
        // reservation back and finish its ordinal, because nothing will run it.
        budget.release_attempt(0, job.reserved);
        budget.finish_ordinal(job.candidate.ordinal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::WorkspacePath;

    #[test]
    fn worker_panic_becomes_internal_fatal() {
        let reply = worker_reply(|| panic!("search worker panic probe"));
        match reply {
            ScanReply::Fatal(FsToolError::Internal(message)) => {
                assert!(message.contains("panicked"));
            }
            other => panic!("expected internal fatal, got {other:?}"),
        }
    }

    #[test]
    fn worker_error_stays_fatal_not_skipped() {
        let reply = worker_reply(|| {
            Err(FsToolError::Workspace(aura_workspace::WorkspaceError::RootIdentityChanged))
        });
        assert!(matches!(
            reply,
            ScanReply::Fatal(FsToolError::Workspace(
                aura_workspace::WorkspaceError::RootIdentityChanged
            ))
        ));
    }

    #[test]
    fn skipped_helper_keeps_reason() {
        let outcome = FileOutcome::skipped(
            0,
            WorkspacePath::parse("a.txt").expect("path"),
            4,
            vec![FsSearchIncompleteReason::FileTooLarge],
        );
        assert!(outcome.skipped);
        assert_eq!(outcome.bytes, 4);
    }

    #[test]
    fn run_stats_fold_matches_per_outcome_totals() {
        let outcomes = vec![
            FileOutcome {
                ordinal: 0,
                path: WorkspacePath::parse("a.txt").expect("path"),
                matches: vec![],
                scanned: true,
                skipped: false,
                bytes: 10,
                reasons: vec![],
            },
            FileOutcome::skipped(
                1,
                WorkspacePath::parse("b.txt").expect("path"),
                0,
                vec![FsSearchIncompleteReason::UnsupportedEncoding],
            ),
        ];
        let mut stats = RunStats::default();
        for outcome in &outcomes {
            stats.fold(outcome);
        }
        let mut direct_scanned = 0_u64;
        let mut direct_bytes = 0_u64;
        let mut direct_skipped = 0_u64;
        for outcome in &outcomes {
            direct_bytes += outcome.bytes;
            if outcome.scanned {
                direct_scanned += 1;
            }
            if outcome.skipped {
                direct_skipped += 1;
            }
        }
        assert_eq!(stats.scanned_file_count, direct_scanned);
        assert_eq!(stats.scanned_bytes, direct_bytes);
        assert_eq!(stats.skipped_file_count, direct_skipped);
        assert!(stats.is_balanced(outcomes.len()));
    }

    #[test]
    fn aggregate_attribution_counts_each_ordinal_once() {
        let mut stats = RunStats::default();
        stats.fold(&FileOutcome::skipped(
            0,
            WorkspacePath::parse("a.txt").expect("path"),
            0,
            vec![FsSearchIncompleteReason::FileTooLarge],
        ));
        stats.attribute(FsSearchIncompleteReason::ResultLimit, 3);
        assert_eq!(stats.skipped_file_count, 4, "four ordinals, one attribution each");
        assert!(stats.is_balanced(4));
        // The old tail accounting could add the undispatched range on top of
        // the ordinals it had already folded, which this balance check rejects.
        assert!(!stats.is_balanced(2));
    }
}
