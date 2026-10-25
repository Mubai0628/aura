//! Ordered commit stream for one search.
//!
//! A search dispatches candidates concurrently, but the result must be the same
//! as if the files had been scanned one by one in candidate order. This module
//! owns that ordering: outcomes are parked by ordinal, and only the contiguous
//! prefix starting at `commit_next` is folded into the statistics and the
//! result window. Every ordinal is attributed exactly once — either through its
//! own outcome or through one aggregate tail entry.
use std::collections::BTreeMap;

use aura_workspace_domain::{FsSearchIncompleteReason, FsSearchTextMatch};

use super::types::FileOutcome;

/// One ordinal that became part of the result window.
pub(crate) struct Collected {
    pub(crate) matches: Vec<FsSearchTextMatch>,
}

/// Running tallies for ordinals that were already committed. Committing folds
/// an outcome into these counters instead of keeping every `FileOutcome`
/// alive, so a search over a large catalog holds results, not candidates.
#[derive(Default)]
pub(crate) struct RunStats {
    pub(crate) scanned_file_count: u64,
    pub(crate) scanned_bytes: u64,
    pub(crate) skipped_file_count: u64,
    pub(crate) reasons: Vec<FsSearchIncompleteReason>,
    pub(crate) matched_files: u64,
    /// Ordinals already attributed to an outcome or to an aggregate.
    attributed_ordinals: u64,
}

impl RunStats {
    /// Fold one outcome into the statistics. Every file counts exactly once.
    pub(crate) fn fold(&mut self, outcome: &FileOutcome) {
        self.attributed_ordinals = self.attributed_ordinals.saturating_add(1);
        self.scanned_bytes = self.scanned_bytes.saturating_add(outcome.bytes);
        if outcome.scanned {
            self.scanned_file_count += 1;
        }
        if outcome.skipped {
            self.skipped_file_count += 1;
        }
        self.reasons.extend(outcome.reasons.iter().copied());
        if !outcome.matches.is_empty() {
            self.matched_files = self.matched_files.saturating_add(1);
        }
    }

    /// Attribute `count` ordinals that were never individually processed to one
    /// aggregate reason instead of allocating a per-file outcome.
    pub(crate) fn attribute(&mut self, reason: FsSearchIncompleteReason, count: usize) {
        if count == 0 {
            return;
        }
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        self.attributed_ordinals = self.attributed_ordinals.saturating_add(count);
        self.skipped_file_count = self.skipped_file_count.saturating_add(count);
        self.reasons.push(reason);
    }

    /// Attribute one ordinal whose worker was halted before it could say why it
    /// has no result.
    ///
    /// Such an outcome carries no reason of its own (it was stopped, not
    /// refused), so publishing it as-is would report a skip nobody can explain.
    /// It inherits the reason the batch stopped for, while the I/O it really
    /// performed is still counted.
    pub(crate) fn attribute_halted(&mut self, reason: FsSearchIncompleteReason, bytes: u64) {
        self.attributed_ordinals = self.attributed_ordinals.saturating_add(1);
        self.scanned_bytes = self.scanned_bytes.saturating_add(bytes);
        self.skipped_file_count += 1;
        self.reasons.push(reason);
    }

    /// Whether an outcome is a halt with nothing to say for itself.
    #[must_use]
    pub(crate) fn is_unexplained_halt(outcome: &FileOutcome) -> bool {
        outcome.skipped
            && !outcome.scanned
            && outcome.matches.is_empty()
            && outcome.reasons.is_empty()
    }

    /// Exactly one attribution per candidate.
    pub(crate) fn is_balanced(&self, candidates: usize) -> bool {
        self.attributed_ordinals == u64::try_from(candidates).unwrap_or(u64::MAX)
    }

    #[must_use]
    pub(crate) fn attributed(&self) -> u64 {
        self.attributed_ordinals
    }
}

pub(crate) struct CommitState {
    /// Number of uncommitted dispatched candidates allowed at once.
    pub(crate) window: usize,
    /// The next ordinal whose result may be published. Also the search's
    /// deterministic "turn" for budget growth.
    pub(crate) commit_next: usize,
    pub(crate) committed_matches: usize,
    pub(crate) stop_dispatch: bool,
}

impl CommitState {
    pub(crate) fn new(window: usize) -> Self {
        Self { window, commit_next: 0, committed_matches: 0, stop_dispatch: false }
    }
}

/// Park an outcome under its own ordinal. Nothing is published yet: publication
/// always waits for the contiguous prefix, so a fast later file can never
/// overtake a slower earlier one.
pub(crate) fn park(finished: &mut BTreeMap<usize, FileOutcome>, outcome: FileOutcome) {
    // Ordinals are unique per candidate, so an existing entry means the same
    // file was reported twice, which would double-count it.
    debug_assert!(
        !finished.contains_key(&outcome.ordinal),
        "ordinal {} was parked twice",
        outcome.ordinal
    );
    finished.insert(outcome.ordinal, outcome);
}

/// Publish every parked ordinal whose predecessors are already published.
/// Returns whether any ordinal was published.
pub(crate) fn ready(
    limit: usize,
    finished: &mut BTreeMap<usize, FileOutcome>,
    collected: &mut Vec<Collected>,
    stats: &mut RunStats,
    commit: &mut CommitState,
) -> bool {
    let mut any = false;
    while let Some(outcome) = finished.remove(&commit.commit_next) {
        any = true;
        publish(outcome, limit, collected, stats, commit);
    }
    any
}

/// Fold one ordinal into the statistics and, when it still fits the result
/// window, into the ordered match stream. Only the contiguous-prefix path may
/// call this; the ordinal check keeps that a hard invariant instead of a
/// convention.
fn publish(
    outcome: FileOutcome,
    limit: usize,
    collected: &mut Vec<Collected>,
    stats: &mut RunStats,
    commit: &mut CommitState,
) {
    debug_assert_eq!(
        outcome.ordinal, commit.commit_next,
        "outcome ordinal {} does not match the commit cursor {}",
        outcome.ordinal, commit.commit_next
    );
    stats.fold(&outcome);
    let remaining = limit.saturating_sub(commit.committed_matches);
    if remaining > 0 && !outcome.matches.is_empty() {
        let taken: Vec<_> = outcome.matches.into_iter().take(remaining).collect();
        commit.committed_matches = commit.committed_matches.saturating_add(taken.len());
        collected.push(Collected { matches: taken });
        if commit.committed_matches >= limit {
            commit.stop_dispatch = true;
        }
    }
    commit.commit_next += 1;
}

/// Everything needed to close a batch that stopped early.
pub(crate) struct FinishRequest<'a> {
    /// Candidates frozen for this search.
    pub(crate) candidates: usize,
    /// The prepared candidate that was never handed to the pool, if any.
    pub(crate) pending_ordinal: Option<usize>,
    /// Candidates actually handed to the pool.
    pub(crate) dispatched: usize,
    pub(crate) limit: usize,
    pub(crate) finished: &'a mut BTreeMap<usize, FileOutcome>,
    pub(crate) collected: &'a mut Vec<Collected>,
    pub(crate) stats: &'a mut RunStats,
    pub(crate) commit: &'a mut CommitState,
    pub(crate) reason: FsSearchIncompleteReason,
}

/// Finish a batch that stopped early. Every ordinal is accounted for exactly
/// once:
///
/// * ordinals below `commit_next` are already published;
/// * `commit_next..dispatched` is drained from `finished` (folding its real
///   state); an ordinal that was dispatched but never reported is attributed to
///   `reason` as aggregate, because its worker was halted;
/// * `dispatched..candidates` was never dispatched — including a `pending`
///   candidate that was prepared but not handed to the pool — and is counted as
///   one aggregate entry for `reason`.
///
/// `pending` therefore only contributes its released reservation here; it is
/// never inserted as its own outcome and never published separately.
pub(crate) fn finish_rest(request: FinishRequest<'_>) {
    let FinishRequest {
        candidates,
        pending_ordinal,
        dispatched,
        limit,
        finished,
        collected,
        stats,
        commit,
        reason,
    } = request;
    debug_assert!(
        pending_ordinal.is_none_or(|ordinal| ordinal >= commit.commit_next),
        "a pending candidate cannot sit below the commit cursor"
    );
    let mut unattributed = 0_usize;
    while commit.commit_next < dispatched.min(candidates) {
        match finished.remove(&commit.commit_next) {
            Some(outcome) if RunStats::is_unexplained_halt(&outcome) => {
                // The worker was stopped by this batch's own halt, so it has no
                // reason to report. Publishing it as-is would look like a skip
                // nobody can explain: it belongs to the batch reason.
                stats.attribute_halted(reason, outcome.bytes);
                commit.commit_next += 1;
            }
            Some(outcome) => publish(outcome, limit, collected, stats, commit),
            None => {
                unattributed += 1;
                commit.commit_next += 1;
            }
        }
    }
    stats.attribute(reason, unattributed);
    // Everything past the dispatch cursor is unprocessed, whether or not it had
    // been prepared.
    stats.attribute(reason, candidates.saturating_sub(dispatched));
    commit.commit_next = candidates;
}

#[cfg(test)]
mod tests {
    use aura_domain_common::WorkspacePath;

    use super::*;

    fn path(name: &str) -> WorkspacePath {
        WorkspacePath::parse(name).expect("path")
    }

    fn hit(ordinal: usize, byte_length: u64) -> FileOutcome {
        FileOutcome {
            ordinal,
            path: path(&format!("file{ordinal}.txt")),
            matches: vec![match_for(ordinal)],
            scanned: true,
            skipped: false,
            bytes: byte_length,
            reasons: Vec::new(),
        }
    }

    fn skip(ordinal: usize, reason: FsSearchIncompleteReason) -> FileOutcome {
        FileOutcome::skipped(ordinal, path(&format!("file{ordinal}.txt")), 0, vec![reason])
    }

    fn match_for(ordinal: usize) -> FsSearchTextMatch {
        FsSearchTextMatch {
            path: path(&format!("file{ordinal}.txt")),
            line_index: 0,
            column_character_offset: 0,
            match_character_length: 1,
            excerpt: "x".to_owned(),
            excerpt_start_character_offset: 0,
            excerpt_truncated_before: false,
            excerpt_truncated_after: false,
            source: aura_workspace_domain::FileStorageSource::Disk,
            logical_hash: aura_domain_common::ContentHash::blake3(b"x"),
            buffer_revision: None,
            ignored: false,
            generated: false,
        }
    }

    #[test]
    fn a_prepare_skip_cannot_overtake_an_in_flight_earlier_ordinal() {
        // Ordinal 1 is skipped during preparation while ordinal 0 is still being
        // scanned. The skip must wait for its turn: publishing it directly would
        // advance the cursor past 0 and lose ordinal 0's match for good.
        let mut finished = BTreeMap::new();
        let mut collected = Vec::new();
        let mut stats = RunStats::default();
        let mut commit = CommitState::new(4);
        let limit = 5;

        park(&mut finished, skip(1, FsSearchIncompleteReason::FileTooLarge));
        assert!(!ready(limit, &mut finished, &mut collected, &mut stats, &mut commit));
        assert_eq!(commit.commit_next, 0, "the skip must not advance the cursor");
        assert_eq!(collected.len(), 0);

        park(&mut finished, hit(0, 10));
        assert!(ready(limit, &mut finished, &mut collected, &mut stats, &mut commit));
        assert_eq!(commit.commit_next, 2, "both ordinals published in order");
        assert_eq!(collected.len(), 1, "ordinal 0's match survived the skip");
        assert_eq!(stats.scanned_file_count, 1);
        assert_eq!(stats.skipped_file_count, 1);
        assert!(stats.is_balanced(2));
    }

    #[test]
    fn a_limit_stop_keeps_earlier_in_flight_matches_and_counts_later_ones_once() {
        // Ordinal 0 is still in flight, ordinal 1 already filled the result
        // window, and a third candidate is prepared but never dispatched.
        let mut finished = BTreeMap::new();
        let mut collected = Vec::new();
        let mut stats = RunStats::default();
        let mut commit = CommitState::new(4);
        let limit = 1;

        park(&mut finished, hit(1, 10));
        // Ordinal 0 is not parked yet: publishing must wait for it.
        assert!(!ready(limit, &mut finished, &mut collected, &mut stats, &mut commit));
        assert_eq!(commit.commit_next, 0);

        park(&mut finished, hit(0, 20));
        assert!(ready(limit, &mut finished, &mut collected, &mut stats, &mut commit));
        assert!(commit.stop_dispatch, "the window filled");
        assert_eq!(commit.commit_next, 2);
        assert_eq!(stats.scanned_file_count, 2, "both scanned files are counted");

        // Two candidates were dispatched; the prepared-but-undispatched third
        // one belongs to the unprocessed tail and must be attributed once.
        finish_rest(FinishRequest {
            candidates: 5,
            pending_ordinal: Some(3),
            dispatched: 2,
            limit,
            finished: &mut finished,
            collected: &mut collected,
            stats: &mut stats,
            commit: &mut commit,
            reason: FsSearchIncompleteReason::ResultLimit,
        });
        assert_eq!(commit.commit_next, 5);
        assert_eq!(stats.skipped_file_count, 3, "ordinals 2..5 counted once each");
        assert!(stats.is_balanced(5), "attributed {}", stats.attributed());
    }

    #[test]
    fn dispatched_but_unreported_ordinals_are_attributed_once() {
        let mut finished = BTreeMap::new();
        let mut collected = Vec::new();
        let mut stats = RunStats::default();
        let mut commit = CommitState::new(4);
        park(&mut finished, hit(0, 5));
        // Ordinal 1 was dispatched and never reported back (worker halted).
        finish_rest(FinishRequest {
            candidates: 4,
            pending_ordinal: None,
            dispatched: 2,
            limit: 5,
            finished: &mut finished,
            collected: &mut collected,
            stats: &mut stats,
            commit: &mut commit,
            reason: FsSearchIncompleteReason::TimeBudget,
        });
        assert_eq!(stats.scanned_file_count, 1);
        assert_eq!(stats.skipped_file_count, 3, "ordinals 1..4 counted once each");
        assert!(stats.is_balanced(4));
        assert!(stats.reasons.contains(&FsSearchIncompleteReason::TimeBudget));
    }

    #[test]
    fn a_halted_worker_inherits_the_batch_reason_instead_of_a_silent_skip() {
        // A worker stopped by the batch halt reports `skipped` with no reason.
        // Publishing that as-is would leave a skip nobody can explain, so the
        // batch reason (here TimeBudget) must be attributed to it.
        let mut finished = BTreeMap::new();
        let mut collected = Vec::new();
        let mut stats = RunStats::default();
        let mut commit = CommitState::new(4);
        park(
            &mut finished,
            FileOutcome {
                ordinal: 0,
                path: path("halted.txt"),
                matches: Vec::new(),
                scanned: false,
                skipped: true,
                bytes: 128,
                reasons: Vec::new(),
            },
        );
        finish_rest(FinishRequest {
            candidates: 1,
            pending_ordinal: None,
            dispatched: 1,
            limit: 5,
            finished: &mut finished,
            collected: &mut collected,
            stats: &mut stats,
            commit: &mut commit,
            reason: FsSearchIncompleteReason::TimeBudget,
        });
        assert_eq!(stats.reasons, vec![FsSearchIncompleteReason::TimeBudget]);
        assert_eq!(stats.skipped_file_count, 1);
        assert_eq!(stats.scanned_bytes, 128, "real I/O is still counted");
        assert!(stats.is_balanced(1));
    }

    #[test]
    #[should_panic(expected = "does not match the commit cursor")]
    fn publishing_out_of_order_is_a_hard_invariant() {
        let mut collected = Vec::new();
        let mut stats = RunStats::default();
        let mut commit = CommitState::new(4);
        publish(hit(2, 1), 5, &mut collected, &mut stats, &mut commit);
    }

    #[test]
    #[should_panic(expected = "was parked twice")]
    fn parking_the_same_ordinal_twice_is_a_hard_invariant() {
        let mut finished = BTreeMap::new();
        park(&mut finished, hit(0, 1));
        park(&mut finished, hit(0, 1));
    }
}
