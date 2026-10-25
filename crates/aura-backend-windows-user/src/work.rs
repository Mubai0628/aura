//! Bounded launch-work counters.
//!
//! The stage timings say *where* a launch spends its time; these counts say *why*
//! the expensive stage is expensive. They are deliberately counts: no path, no
//! script text, no credential, no nonce and no directory content ever enters a
//! record, so the same line stays safe to journal beside the timings.
//!
//! Every field is a plain saturating counter. Nothing here collects per-object
//! state, so a large tree costs one increment per object rather than one entry.

use serde::Serialize;

/// What one ACL grant pass over a single root actually touched.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct GrantWork {
    /// Objects opened and inspected, including the root itself.
    pub objects_scanned: u64,
    /// Directories among the scanned objects, whose children were enumerated.
    pub directories_scanned: u64,
    /// Objects whose ACL was actually rewritten (captured, planned, applied).
    pub acl_rewrites: u64,
    /// Objects deliberately left alone: reparse points and multiply-linked files.
    pub objects_skipped: u64,
    /// Durable intent/scope records written, each flushed before the next object.
    pub journal_records: u64,
    /// Why the pass stopped early, if it did. A stopped pass still reports every count
    /// above: the work it did before stopping is real work that happened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<crate::file_lease::GrantStop>,
}

impl GrantWork {
    /// Fold another pass into this one. Saturating: a counter that could overflow
    /// must not wrap into a small number that reads like a fast launch.
    pub fn add(&mut self, other: Self) {
        self.objects_scanned = self.objects_scanned.saturating_add(other.objects_scanned);
        self.directories_scanned =
            self.directories_scanned.saturating_add(other.directories_scanned);
        self.acl_rewrites = self.acl_rewrites.saturating_add(other.acl_rewrites);
        self.objects_skipped = self.objects_skipped.saturating_add(other.objects_skipped);
        self.journal_records = self.journal_records.saturating_add(other.journal_records);
    }
}

/// What one ancestor-metadata grant pass touched. Ancestor chains are bounded by
/// the platform's own limit, so these numbers stay small by construction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AncestorWork {
    /// Ancestors pinned against rename, whether or not they needed a rewrite.
    pub ancestors_pinned: u64,
    /// Ancestors whose metadata ACL was rewritten.
    pub acl_rewrites: u64,
    /// Durable records written for those rewrites.
    pub journal_records: u64,
}

/// What opening the runtime's lock-and-lease-recovery block actually cost.
///
/// Journals that already settled are served from the trusted released index
/// instead of being read and parsed again; `parsed` plus `indexed` therefore
/// stays equal to `journals`, and `bytes` covers only the content this open
/// really read. Counts only: no journal name, path or hash is a workload field.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RecoveryWork {
    /// Journal files found in the lease directory.
    pub journals: u64,
    /// Journals whose content was read, hashed and parsed in full this time.
    pub parsed: u64,
    /// Released journals served from the trusted index (identity, size and change
    /// generation re-checked under a writer-excluding handle, content not re-read).
    pub indexed: u64,
    /// Journals whose change generation was checked this open. This is the real
    /// verification cost of a warm open, and it is deliberately not folded into
    /// `bytes`: those reads did not happen.
    pub witnessed: u64,
    /// Journal content bytes read and hashed on this open.
    pub bytes: u64,
    /// Journal records parsed on this open.
    pub records: u64,
    /// Wall time of the whole recovery block.
    pub ms: u64,
}

/// Per-launch bounded workload counters, journaled once per launch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct LaunchWorkV1 {
    /// Workspace/scratch roots handed to the ACL walk.
    pub roots_granted: u64,
    pub objects_scanned: u64,
    pub directories_scanned: u64,
    pub acl_rewrites: u64,
    pub objects_skipped: u64,
    pub journal_records: u64,
    /// Roots whose ancestor metadata chain was granted.
    pub ancestor_roots_granted: u64,
    pub ancestors_pinned: u64,
    pub ancestor_acl_rewrites: u64,
    /// Installation/resource re-verifications performed for this launch.
    pub installation_checks: u64,
    /// True when the runtime's lock-plus-lease-recovery block was already open and
    /// therefore skipped: the only cache this path has.
    pub runtime_reused: bool,
    /// Resource units holding a live lease at the moment of allocation.
    pub active_resources: u64,
    /// Of those, the ones still owning a live background tree.
    pub background_resources: u64,
    /// Bytes fed to a content hash on this path. Expected to be tiny: if this ever
    /// dominates, the launch is hashing something it should be reusing.
    pub hashed_bytes: u64,
    /// What opening the lock-and-lease-recovery block cost this launch. All zero
    /// when the runtime was reused: the recovery belongs to the launch that
    /// opened the block, not to every launch that follows it.
    pub recovery_ms: u64,
    pub recovery_journals: u64,
    pub recovery_bytes: u64,
    /// Journal records parsed on this open (indexed journals are not re-parsed).
    pub recovery_records: u64,
    /// Released journals served from the trusted index instead of a full parse.
    pub recovery_indexed: u64,
    /// Of those, the ones whose change generation was actually checked. This is
    /// the work a warm open really does, and it is why `recovery_bytes` alone
    /// cannot be read as "the recovery did nothing".
    pub recovery_index_checks: u64,
    /// How long this launch waited for its share of the shared gate.
    ///
    /// The four durations here are the four budgets that used to be confused with one
    /// another. Reading them together is what tells "the machine was busy elsewhere"
    /// (queue) from "the access-control work is the cost" (preparation) from "the
    /// process and handshake are the cost" (startup) — and the window is how much of
    /// the short launch ticket was left when Execute went out, which is the number
    /// that was zero for the two CMD launches.
    pub queue_ms: u64,
    pub preparation_ms: u64,
    pub startup_ms: u64,
    pub launch_window_ms: u64,
}

impl LaunchWorkV1 {
    /// One root's grant pass.
    pub fn record_grant(&mut self, work: GrantWork) {
        self.roots_granted = self.roots_granted.saturating_add(1);
        self.objects_scanned = self.objects_scanned.saturating_add(work.objects_scanned);
        self.directories_scanned =
            self.directories_scanned.saturating_add(work.directories_scanned);
        self.acl_rewrites = self.acl_rewrites.saturating_add(work.acl_rewrites);
        self.objects_skipped = self.objects_skipped.saturating_add(work.objects_skipped);
        self.journal_records = self.journal_records.saturating_add(work.journal_records);
    }

    /// One grant pass that may have failed or stopped early.
    ///
    /// A pass that returned an error still did work, and that work is real: the objects it
    /// captured, journaled and rewrote are exactly what a recovery has to undo. Losing the
    /// count because the pass ended badly would report a launch that touched nothing when
    /// it touched thousands.
    pub fn record_grant_attempt(&mut self, work: &crate::file_lease::GrantOutcome) {
        self.record_grant(work.work);
    }

    /// One root's ancestor-metadata pass.
    pub fn record_ancestors(&mut self, work: AncestorWork) {
        self.ancestor_roots_granted = self.ancestor_roots_granted.saturating_add(1);
        self.ancestors_pinned = self.ancestors_pinned.saturating_add(work.ancestors_pinned);
        self.ancestor_acl_rewrites = self.ancestor_acl_rewrites.saturating_add(work.acl_rewrites);
        self.journal_records = self.journal_records.saturating_add(work.journal_records);
    }

    pub fn record_installation_check(&mut self) {
        self.installation_checks = self.installation_checks.saturating_add(1);
    }

    pub fn record_hashed_bytes(&mut self, bytes: u64) {
        self.hashed_bytes = self.hashed_bytes.saturating_add(bytes);
    }

    /// The recovery block's own cost, reported by the launch that opened it.
    pub fn record_recovery(&mut self, work: RecoveryWork) {
        self.recovery_ms = self.recovery_ms.saturating_add(work.ms);
        self.recovery_journals = self.recovery_journals.saturating_add(work.journals);
        self.recovery_bytes = self.recovery_bytes.saturating_add(work.bytes);
        self.recovery_records = self.recovery_records.saturating_add(work.records);
        self.recovery_indexed = self.recovery_indexed.saturating_add(work.indexed);
        self.recovery_index_checks = self.recovery_index_checks.saturating_add(work.witnessed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The counters add up, and they saturate instead of wrapping.
    #[test]
    fn counters_fold_together_and_saturate() {
        let mut work = LaunchWorkV1::default();
        work.record_grant(GrantWork {
            objects_scanned: 10,
            directories_scanned: 3,
            acl_rewrites: 8,
            objects_skipped: 1,
            journal_records: 9,
            stopped: None,
        });
        work.record_grant(GrantWork { objects_scanned: u64::MAX, ..GrantWork::default() });
        work.record_ancestors(AncestorWork {
            ancestors_pinned: 4,
            acl_rewrites: 2,
            journal_records: 2,
        });
        assert_eq!(work.roots_granted, 2);
        assert_eq!(work.objects_scanned, u64::MAX, "a saturated count never wraps small");
        assert_eq!(work.directories_scanned, 3);
        assert_eq!(work.acl_rewrites, 8);
        assert_eq!(work.objects_skipped, 1);
        assert_eq!(work.ancestor_roots_granted, 1);
        assert_eq!(work.ancestors_pinned, 4);
        assert_eq!(work.ancestor_acl_rewrites, 2);
        assert_eq!(work.journal_records, 11);
    }

    /// The wire shape is flat counts and one boolean. A record that grew a path or a
    /// free-text field would be a place for script content to leak into the journal.
    #[test]
    fn a_launch_work_record_carries_counts_only() {
        let mut work =
            LaunchWorkV1 { runtime_reused: true, active_resources: 2, ..Default::default() };
        work.record_hashed_bytes(31);
        let wire = serde_json::to_value(work).expect("wire");
        let object = wire.as_object().expect("object");
        for (key, value) in object {
            assert!(
                value.is_u64() || value.is_boolean(),
                "`{key}` must be a count or a flag, not {value}"
            );
        }
        assert_eq!(object["runtime_reused"], serde_json::json!(true));
        assert_eq!(object["active_resources"], serde_json::json!(2));
        assert_eq!(object["hashed_bytes"], serde_json::json!(31));
        assert_eq!(object.len(), 24, "a new field is a new promise about what the record explains");
    }

    /// Recovery cost is recorded once, by the launch that opened the block.
    #[test]
    fn recovery_work_is_recorded_as_counts() {
        let mut work = LaunchWorkV1::default();
        assert_eq!(work.recovery_journals, 0, "a reused runtime recovered nothing");
        work.record_recovery(RecoveryWork {
            journals: 4,
            parsed: 1,
            indexed: 3,
            witnessed: 3,
            bytes: 900,
            records: 40,
            ms: 12,
        });
        let wire = serde_json::to_value(work).expect("wire");
        assert_eq!(wire["recovery_ms"], serde_json::json!(12));
        assert_eq!(wire["recovery_journals"], serde_json::json!(4));
        assert_eq!(wire["recovery_bytes"], serde_json::json!(900));
        assert_eq!(wire["recovery_records"], serde_json::json!(40));
        assert_eq!(wire["recovery_indexed"], serde_json::json!(3));
        assert_eq!(wire["recovery_index_checks"], serde_json::json!(3));
    }

    /// The four budgets are reported as separate spans.
    ///
    /// They used to be one number — a thirty-second preparation window that a launch
    /// was refused for having spent — so a reader has to be able to see, per launch,
    /// which of the four actually took the time.
    #[test]
    fn the_four_launch_budgets_are_reported_separately() {
        let work = LaunchWorkV1 {
            queue_ms: 11,
            preparation_ms: 22,
            startup_ms: 33,
            launch_window_ms: 44,
            ..Default::default()
        };
        let wire = serde_json::to_value(work).expect("wire");
        assert_eq!(wire["queue_ms"], serde_json::json!(11));
        assert_eq!(wire["preparation_ms"], serde_json::json!(22));
        assert_eq!(wire["startup_ms"], serde_json::json!(33));
        assert_eq!(wire["launch_window_ms"], serde_json::json!(44));
    }
}
