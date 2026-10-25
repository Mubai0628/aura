//! Incremental catalog updates.
//!
//! The ordinary path for both an Aura mutation and an external watcher event is a
//! bounded local update: only the paths that actually changed are re-read, only
//! the affected entries are replaced, and only the touched partition summaries are
//! recomputed. A full [`PathCatalog::rebuild`] is reserved for first indexing, a
//! proven lost watcher event, or a change that cannot be described locally, and it
//! records its reason so the cost is auditable rather than implicit.
//!
//! Three scopes are distinguished, because they cost very different amounts:
//!
//! - [`CatalogChange::Changed`] re-reads exactly one path (a file's content, or a
//!   directory's own metadata) and never descends into it;
//! - [`CatalogChange::DirectoryListed`] re-lists one directory's direct children —
//!   the bounded answer to "something in here changed" when the caller cannot name
//!   the child;
//! - [`CatalogChange::DirectoryRescanned`] re-reads a whole subtree, and is only
//!   used when the caller knows that the subtree itself is new or replaced.
//!
//! Nothing is published until the durable delta has committed: a failed write
//! leaves the in-memory view exactly as it was.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use aura_domain_common::{CorrelationId, Revision, WorkspacePath};
use aura_persistence::{
    CatalogCommitOutcome, CatalogCommitRefusal, CatalogDeltaRecord, CatalogPartitionRecord,
};
use aura_workspace_domain::{CatalogEntryKind, CatalogIndexMode, CatalogPartitionSummary};

use super::scan::{
    CatalogEntry, CatalogOptions, CatalogState, PartitionStats, PathCatalog, PendingEntry,
    hydrate_entry, inherited_ignores, is_text_candidate, list_directory_once, scan_directory,
    scan_directory_entries,
};
use crate::{SecurePathResolver, WorkspaceError};

/// What happened to one path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogChange {
    /// Exactly this path changed. A file is re-read (metadata and content); a
    /// directory has its own metadata refreshed and nothing below it is touched.
    Changed,
    /// The path and, for a directory, its whole subtree are gone.
    Removed,
    /// The caller observed that this directory's own listing changed but cannot
    /// name the child, so its direct children are re-listed and nothing below them
    /// is read.
    DirectoryListed,
    /// The caller knows this directory's whole subtree is new or replaced, so the
    /// subtree is re-read.
    DirectoryRescanned,
}

/// A bounded update request. Paths are workspace-relative and already resolved
/// by the caller; this module never widens scope by itself.
#[derive(Clone, Debug)]
pub struct CatalogUpdate {
    pub changes: Vec<(WorkspacePath, CatalogChange)>,
    /// Which bounded scope produced this delta. Recorded with the durable event
    /// so a recovery is never mistaken for a routine update.
    pub reason: String,
    /// How many raw inputs (watcher events, or the one write) the changes were
    /// merged from. Reported as a count and never used to decide anything.
    pub inputs: usize,
    /// True when this update exists to write text the index is missing, not to react to
    /// a change in the file.
    ///
    /// A repair re-reads a path whose text row was dropped, and the file on disk is
    /// usually exactly what the catalog already records — so the ordinary "nothing about
    /// this file changed" answer is right about the file and wrong about the work: the
    /// missing text still has to be written. This flag says so explicitly instead of
    /// leaving the decision to a comparison that cannot see the difference.
    pub writes_missing_text: bool,
    /// Paths whose re-derivation mark this update settles. Empty for an ordinary
    /// change notification: a mark is cleared only by the committed write of that
    /// path's text or removal, never ahead of it.
    pub repaired_paths: Vec<WorkspacePath>,
}

impl CatalogUpdate {
    /// One bounded update with its inputs unknown to the caller.
    #[must_use]
    pub fn new(changes: Vec<(WorkspacePath, CatalogChange)>, reason: impl Into<String>) -> Self {
        Self {
            changes,
            reason: reason.into(),
            inputs: 0,
            writes_missing_text: false,
            repaired_paths: Vec::new(),
        }
    }
}

/// Why one path was read again instead of being settled from its recorded facts.
///
/// A bounded reason code, never a path or file content. It exists so "the batch was
/// slow" can be answered with what the batch actually did, in a production log as
/// well as a test.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogReadReason {
    /// The index has never recorded this path, so there is nothing to compare with.
    NoRecordedEntry,
    /// The file has never been read successfully, so nothing records its bytes.
    NoRecordedDigest,
    /// The partition does not index text, so there is nothing to compare.
    PartitionWithoutText,
    /// The recorded entry does not hold the text facts a full-text partition keeps.
    MissingRecordedText,
    /// The partition changed how much of a file it keeps.
    PartitionModeChanged,
    /// A change notification described a text file, so its bytes have to be read
    /// again: a listing cannot prove that what is on disk is what the index holds.
    ContentNotProvenById,
}

impl CatalogReadReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::NoRecordedEntry => "no_recorded_entry",
            Self::NoRecordedDigest => "no_recorded_digest",
            Self::PartitionWithoutText => "partition_without_text",
            Self::MissingRecordedText => "missing_recorded_text",
            Self::PartitionModeChanged => "partition_mode_changed",
            Self::ContentNotProvenById => "content_not_proven_by_id",
        }
    }
}

/// Why one update could not settle a path without writing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CatalogUpdateReasons {
    pub read: BTreeMap<&'static str, usize>,
}

impl CatalogUpdateReasons {
    fn record(&mut self, reason: CatalogReadReason) {
        *self.read.entry(reason.as_str()).or_default() += 1;
    }
}

/// What one update did, as bounded counts.
///
/// Every field is a count, a duration or a small identifier. Nothing here names a
/// path or carries file content, so the record is safe to keep beside ordinary
/// runtime logs. It is the difference between "the filesystem was slow" and "the
/// index re-read and rewrote a directory that had not changed": `no_op` is how many
/// of the `read` paths were already described exactly by their durable row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogUpdateReport {
    /// Raw inputs the update was merged from. Zero when the caller did not say.
    pub inputs: usize,
    /// Distinct paths the update described after merging.
    pub described: usize,
    /// Distinct paths that were re-listed and re-read.
    pub read: usize,
    /// Of those, paths whose complete observable facts still matched.
    pub no_op: usize,
    /// Duplicate descriptions of one path inside a single update.
    pub duplicates: usize,
    /// Entries written to the durable calendar.
    pub upserted: usize,
    pub removed: usize,
    /// Partition summaries whose facts changed.
    pub partitions: usize,
    /// Paths dropped from the watch set with a removal.
    pub watch_directories_removed: usize,
    pub watch_directories_added: usize,
    /// Whether the update produced any durable write at all.
    pub persisted: bool,
    pub revision: Revision,
    /// The published revision the plan was formed against. It equals `revision` unless
    /// another update committed while this one was reading and forced a re-plan.
    pub catalog_view_revision: Revision,
    /// How many times this update discarded a plan and planned again because the
    /// published view moved under it. Zero in the ordinary case; a non-zero count is
    /// how "another delta won the race" becomes visible instead of silent.
    pub replans: usize,
    pub elapsed_ms: u64,
    /// How many of `read` paths were read for each bounded reason. Counts only.
    pub reasons: BTreeMap<&'static str, usize>,
}

impl CatalogUpdateReport {
    /// A report for an update that described nothing at all.
    fn unchanged(
        inputs: usize,
        described: usize,
        duplicates: usize,
        revision: Revision,
        reasons: &CatalogUpdateReasons,
    ) -> Self {
        Self {
            inputs,
            described,
            read: 0,
            no_op: 0,
            duplicates,
            upserted: 0,
            removed: 0,
            partitions: 0,
            watch_directories_removed: 0,
            watch_directories_added: 0,
            persisted: false,
            revision,
            catalog_view_revision: revision,
            replans: 0,
            elapsed_ms: 0,
            reasons: reasons.read.clone(),
        }
    }

    fn log(&self, database_path: &Path, reason: &str) {
        crate::diagnostics::append_workspace_log(
            database_path,
            "workspace.catalog.update",
            serde_json::json!({
                "scope": reason,
                "inputs": self.inputs,
                "described": self.described,
                "read": self.read,
                "no_op": self.no_op,
                "duplicates": self.duplicates,
                "upserted": self.upserted,
                "removed": self.removed,
                "partitions": self.partitions,
                "watch_directories_removed": self.watch_directories_removed,
                "watch_directories_added": self.watch_directories_added,
                "persisted": self.persisted,
                "catalog_revision": self.revision.value(),
                "catalog_view_revision": self.catalog_view_revision.value(),
                "replans": self.replans,
                "elapsed_ms": self.elapsed_ms,
                "read_reasons": self.reasons,
            }),
        );
    }
}

/// The changes one update will make, worked out before anything is written.
#[derive(Default)]
struct CatalogPlan {
    /// Entries whose recorded facts no longer describe the file on disk.
    upserts: BTreeMap<WorkspacePath, CatalogEntry>,
    /// Indexed text of those entries, taken out of their record.
    indexed: BTreeMap<WorkspacePath, Option<String>>,
    removed: BTreeSet<WorkspacePath>,
    /// Paths a fresh listing produced. Anything inside an authoritative scope that
    /// is missing here is gone from disk.
    reproduced: BTreeSet<WorkspacePath>,
    /// Scopes whose whole subtree is authoritative.
    subtree_scopes: Vec<WorkspacePath>,
    /// Scopes whose direct children are authoritative.
    child_scopes: Vec<WorkspacePath>,
    /// Paths whose own existence was re-checked.
    checked: BTreeSet<WorkspacePath>,
    watch_directories: Vec<std::path::PathBuf>,
    /// The partition summaries the published view would have, and which of them
    /// describe something other than what is already stored.
    partitions: Vec<CatalogPartitionSummary>,
    changed_partitions: Vec<CatalogPartitionRecord>,
    removed_partitions: Vec<WorkspacePath>,
    /// True when every described path has to be written, because the index is missing
    /// text for it rather than because the file changed.
    writes_missing_text: bool,
    /// Paths this update re-read, and how many of them turned out unchanged.
    read: usize,
    no_op: usize,
    duplicates: usize,
    reasons: CatalogUpdateReasons,
}

/// The published view a plan was formed against.
///
/// Planning, listing and reading happen *outside* the catalog's state lock, so the
/// facts a plan is built from have to be held by the plan rather than read again from
/// a lock later. This is a copy of the published view and nothing else: it carries no
/// durability claim, and every path it describes is re-validated against the live view
/// before anything is published.
#[derive(Clone)]
pub(crate) struct CatalogView {
    pub(crate) entries: BTreeMap<WorkspacePath, CatalogEntry>,
    pub(crate) partitions: Vec<CatalogPartitionSummary>,
    #[allow(dead_code)]
    pub(crate) indexed_paths: BTreeSet<WorkspacePath>,
    #[allow(dead_code)]
    pub(crate) text_rows_known: bool,
    pub(crate) revision: Revision,
}

impl CatalogView {
    /// Takes the current published view. Holds the read lock for exactly this copy.
    pub(crate) fn of(state: &CatalogState) -> Self {
        Self {
            entries: state.entries.clone(),
            partitions: state.partitions.clone(),
            indexed_paths: state.indexed_paths.clone(),
            text_rows_known: state.text_rows_known,
            revision: state.revision,
        }
    }
}

impl PathCatalog {
    /// Applies a bounded catalog delta and returns the new revision.
    ///
    /// A single-path update re-reads only that path. A directory listing re-reads
    /// only that directory's direct children. A subtree rescan re-reads only that
    /// subtree. The durable delta is written first; only a committed write changes
    /// the published view, so a failed write cannot publish changed memory under
    /// the old revision.
    ///
    /// Planning and file reads work on a copy of the published view ([`CatalogView`])
    /// without holding the state lock or the commit coordination. The commit itself is
    /// three bounded steps: validate the plan against the live view, write the durable
    /// delta **conditioned on** the revision the plan was derived from, and swap the
    /// published view in memory. No reader waits on the commit coordination or on the
    /// storage reply — a query takes only the state lock, which is now held for the swap
    /// and nothing else.
    pub fn apply_changes(
        &self,
        resolver: &SecurePathResolver,
        update: CatalogUpdate,
        options: &CatalogOptions,
        correlation_id: CorrelationId,
    ) -> Result<Revision, WorkspaceError> {
        if resolver.workspace_id() != self.workspace_id {
            return Err(WorkspaceError::ScopeUnavailable {
                path: WorkspacePath::parse(WorkspacePath::ROOT).expect("root path"),
            });
        }
        let started = std::time::Instant::now();
        resolver.verify_root_identity()?;
        let inputs = update.inputs;
        let reason = update.reason;
        let writes_missing_text = update.writes_missing_text;
        let repaired_paths = update.repaired_paths;
        let mut changes = update.changes;
        let described = changes.len();
        changes.sort_by(|left, right| left.0.cmp(&right.0));
        changes.dedup_by(|left, right| left.0 == right.0);
        let duplicates = described.saturating_sub(changes.len());
        let mut replans = 0_usize;
        for attempt in 0..MAX_PLAN_ATTEMPTS {
            // 1. Take the published view. The lock is held for this copy only.
            let view = {
                let state =
                    self.state.read().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
                CatalogView::of(&state)
            };
            if changes.is_empty() {
                // Nothing was described, so the view is the answer. Publishing an
                // unchanged report is the one thing a no-op update does.
                return self
                    .publish_unchanged(view, inputs, described, duplicates, started, &reason);
            }
            // 2. Plan and read with no lock held: this is the traversal, the hydrate
            //    and the hashing, and it must not occupy anybody else's window.
            let plan =
                self.plan_against(&view, resolver, options, &changes, writes_missing_text)?;
            // 3. Publish. The commit coordination serialises commits of this catalog;
            //    readers are unaffected because they never take it, and the state lock is
            //    held only for the in-memory swap that follows the durable write.
            match self.publish_plan(
                &view,
                plan,
                resolver,
                options,
                &changes,
                inputs,
                described,
                duplicates,
                writes_missing_text,
                repaired_paths.clone(),
                &reason,
                correlation_id,
                started,
                replans,
            )? {
                PublishOutcome::Published(revision) => return Ok(revision),
                PublishOutcome::Superseded => {
                    // Another update committed while this one was reading. Its facts are
                    // newer than the ones this plan was built from, so the plan is
                    // discarded in full and the same changes are planned again against
                    // the view that update published.
                    replans = replans.saturating_add(1);
                    if attempt + 1 == MAX_PLAN_ATTEMPTS {
                        return Err(WorkspaceError::CatalogPlanSuperseded { replans });
                    }
                }
                PublishOutcome::RevisionMoved => {
                    // The **durable** revision moved ahead of this instance's published
                    // view: something committed outside this catalog, or this catalog's own
                    // write landed without its swap. Re-planning from the stale view would
                    // derive the next plan from facts that no longer exist, so the
                    // committed view is restored first and the plan is then re-derived
                    // against it.
                    self.restore_committed_view()?;
                    replans = replans.saturating_add(1);
                    if attempt + 1 == MAX_PLAN_ATTEMPTS {
                        return Err(WorkspaceError::CatalogPlanSuperseded { replans });
                    }
                }
                PublishOutcome::InstanceLost(reason) => {
                    // This instance no longer owns this workspace's index, or the workspace
                    // is gone: re-reading cannot change that, so the loss is reported
                    // instead of retried.
                    crate::diagnostics::append_workspace_log(
                        self.persistence.database_path(),
                        "workspace.catalog.commit_refused",
                        serde_json::json!({
                            "reason": reason,
                            "replans": replans,
                            "published_revision": view.revision.value(),
                        }),
                    );
                    return Err(WorkspaceError::CatalogPlanSuperseded { replans });
                }
            }
        }
        Err(WorkspaceError::CatalogPlanSuperseded { replans })
    }

    /// Plans one attempt against a view, filling in every field the report needs.
    #[allow(clippy::too_many_arguments)]
    fn plan_against(
        &self,
        view: &CatalogView,
        resolver: &SecurePathResolver,
        options: &CatalogOptions,
        changes: &[(WorkspacePath, CatalogChange)],
        writes_missing_text: bool,
    ) -> Result<CatalogPlan, WorkspaceError> {
        // The repair flag is part of the plan *before* the plan is worked out: it is what
        // tells the per-path comparison that this update exists to write text the index is
        // missing, so "the recorded facts still match the file" is the wrong answer here.
        // Setting it after planning made every repair a no-op — the path was read, found
        // unchanged, and left without the text row its mark said it still owed.
        let mut plan = plan_changes(view, resolver, options, changes, writes_missing_text)?;
        plan.writes_missing_text = writes_missing_text;
        plan.partitions = candidate_partitions(view, &plan, options);
        let already_stored = view
            .partitions
            .iter()
            .map(|partition| (partition.partition_path.clone(), partition.facts()))
            .collect::<BTreeMap<_, _>>();
        plan.changed_partitions = plan
            .partitions
            .iter()
            .filter(|partition| {
                already_stored.get(&partition.partition_path) != Some(&partition.facts())
            })
            .map(|partition| CatalogPartitionRecord {
                partition_path: partition.partition_path.clone(),
                file_count: partition.file_count,
                indexable_text_bytes: partition.indexable_text_bytes,
                index_mode: partition.index_mode,
            })
            .collect();
        let current_partitions = plan
            .partitions
            .iter()
            .map(|partition| partition.partition_path.clone())
            .collect::<BTreeSet<_>>();
        plan.removed_partitions = already_stored
            .keys()
            .filter(|path| !current_partitions.contains(*path))
            .cloned()
            .collect();
        Ok(plan)
    }

    /// Publishes a report for an update that described nothing.
    fn publish_unchanged(
        &self,
        view: CatalogView,
        inputs: usize,
        described: usize,
        duplicates: usize,
        started: std::time::Instant,
        reason: &str,
    ) -> Result<Revision, WorkspaceError> {
        let mut state =
            self.state.write().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
        let revision = state.revision;
        let mut report = CatalogUpdateReport::unchanged(
            inputs,
            described,
            duplicates,
            revision,
            &CatalogUpdateReasons::default(),
        );
        report.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        report.catalog_view_revision = view.revision;
        report.log(self.persistence.database_path(), reason);
        state.last_update = Some(report);
        Ok(revision)
    }

    /// Validates a plan, commits it durably under the revision it was derived from, and
    /// publishes the result in memory.
    ///
    /// The order is the protocol:
    ///
    /// 1. take the commit coordination of this catalog — commits are serialised, readers
    ///    are not;
    /// 2. compare the plan's view with the **live** published view, and discard the plan if
    ///    the view moved;
    /// 3. write the durable delta **guarded** by the revision the plan came from and by the
    ///    instance that claimed this workspace's index, both checked inside the writing
    ///    transaction, so an overtaken or replaced writer cannot commit at all;
    /// 4. swap the published view in memory, under a short write lock.
    ///
    /// Nothing here holds the state lock while storage is called, so a delayed write
    /// cannot delay a query; the durable guard is what keeps the delayed write from
    /// landing after the facts it was derived from have been replaced.
    #[allow(clippy::too_many_arguments)]
    fn publish_plan(
        &self,
        view: &CatalogView,
        plan: CatalogPlan,
        resolver: &SecurePathResolver,
        options: &CatalogOptions,
        changes: &[(WorkspacePath, CatalogChange)],
        inputs: usize,
        described: usize,
        duplicates: usize,
        writes_missing_text: bool,
        repaired_paths: Vec<WorkspacePath>,
        reason: &str,
        correlation_id: CorrelationId,
        started: std::time::Instant,
        replans: usize,
    ) -> Result<PublishOutcome, WorkspaceError> {
        // (1) One commit of this catalog at a time. Queries never take this.
        let _commit = self.enter_commit()?;
        // (2) Optimistic concurrency: the plan was built from `view`, so the live view has
        //     to still be that view when the plan is published. A revision that moved means
        //     the facts moved with it, and those facts are newer than the ones this plan
        //     was derived from. A *verification* is not enough here - a path could have been
        //     replaced and replaced back - so the whole plan is discarded and re-derived.
        if self.published_revision()? != view.revision {
            return Ok(PublishOutcome::Superseded);
        }
        let report = CatalogUpdateReport {
            inputs,
            described,
            read: plan.read,
            no_op: plan.no_op,
            duplicates,
            upserted: plan.upserts.len(),
            removed: plan.removed.len(),
            partitions: plan.changed_partitions.len() + plan.removed_partitions.len(),
            watch_directories_added: plan.watch_directories.len(),
            watch_directories_removed: 0,
            persisted: !(plan.upserts.is_empty()
                && plan.removed.is_empty()
                && plan.changed_partitions.is_empty()
                && plan.removed_partitions.is_empty()),
            revision: view.revision,
            catalog_view_revision: view.revision,
            replans,
            elapsed_ms: 0,
            reasons: plan.reasons.read.clone(),
        };
        let CatalogPlan {
            upserts,
            indexed,
            removed,
            partitions,
            changed_partitions,
            removed_partitions,
            watch_directories,
            ..
        } = plan;
        // A batch that described nothing but paths the index already records is not
        // a content change. Advancing the revision here is what invalidated every
        // directory cursor in the workspace for an access-control notification that
        // only changed ACLs, and what made the status query queue behind a large
        // write. Nothing durable has to change, so nothing is written.
        // Which paths the durable index will hold text for, decided from the same
        // values the records are built from.
        let indexed_text = upserts
            .values()
            .map(|entry| {
                (entry.path.clone(), indexed.get(&entry.path).cloned().flatten().is_some())
            })
            .collect::<Vec<_>>();
        let revision = if report.persisted {
            let delta = CatalogDeltaRecord {
                upserts: upserts
                    .values()
                    .map(|entry| entry.to_record(indexed.get(&entry.path).cloned().flatten()))
                    .collect(),
                removed_paths: removed.iter().cloned().collect(),
                removed_partitions,
                partitions: changed_partitions,
                repaired_paths,
                reason: reason.to_owned(),
            };
            // (3) The durable write decides whether this update happened at all, and it is
            //     conditioned on the very facts the plan was derived from: the workspace's
            //     durable catalog revision and the instance that claimed its index. The
            //     condition is evaluated inside the transaction that writes, so no writer
            //     can slip between the check and the modification. Until it returns, the
            //     published view is untouched.
            match self.persistence.apply_workspace_catalog_delta_guarded(
                self.workspace_id,
                self.commit_position(view.revision),
                delta,
                correlation_id,
            )? {
                CatalogCommitOutcome::Applied(revision) => revision,
                CatalogCommitOutcome::Refused(CatalogCommitRefusal::RevisionMoved { .. }) => {
                    return Ok(PublishOutcome::RevisionMoved);
                }
                CatalogCommitOutcome::Refused(CatalogCommitRefusal::InstanceReplaced {
                    ..
                }) => {
                    return Ok(PublishOutcome::InstanceLost("instance_replaced"));
                }
                CatalogCommitOutcome::Refused(CatalogCommitRefusal::WorkspaceMissing) => {
                    return Ok(PublishOutcome::InstanceLost("workspace_missing"));
                }
            }
        } else {
            view.revision
        };
        // (4) The swap. It is short by construction: every file has been read, every
        //     record built and the durable write has already returned. Readers see either
        //     the previous view or this one.
        let mut state =
            self.state.write().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
        if state.revision != view.revision {
            // Only reachable if a commit took the state lock while this one held the commit
            // coordination, which the coordination exists to prevent. The durable write has
            // already happened, so the published view has to be restored from it rather
            // than left describing neither the old nor the new facts.
            drop(state);
            self.restore_committed_view()?;
            return Ok(PublishOutcome::RevisionMoved);
        }
        if report.persisted {
            for (path, has_text) in indexed_text {
                if has_text {
                    state.indexed_paths.insert(path);
                } else {
                    state.indexed_paths.remove(&path);
                }
            }
            for path in &removed {
                state.indexed_paths.remove(path);
            }
            state.text_rows_known = true;
        }
        for path in &removed {
            state.entries.remove(path);
        }
        for (path, entry) in upserts {
            state.entries.insert(path, entry);
        }
        // A removed subtree leaves the watch set as well. The set is derived from the
        // index, so a registration for a path the index no longer holds is stale by
        // definition — and because it was only ever extended, every temporary directory
        // the workspace had ever seen stayed registered for the life of the process.
        let before_prune = state.watch_directories.len();
        prune_watch_directories(resolver.canonical_root(), &mut state.watch_directories, &removed);
        let pruned = state.watch_directories.len();
        state.watch_directories.extend(watch_directories);
        state.watch_directories.sort();
        state.watch_directories.dedup();
        state.partitions = partitions;
        state.revision = revision;
        let report = CatalogUpdateReport {
            watch_directories_removed: before_prune.saturating_sub(pruned),
            revision,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            ..report
        };
        let _ = (writes_missing_text, options, changes);
        report.log(self.persistence.database_path(), reason);
        state.last_update = Some(report);
        Ok(PublishOutcome::Published(revision))
    }
}

/// How many times one update may be re-planned before it admits that another writer is
/// winning. Every attempt is a full, correct plan; the bound only stops a pathological
/// contention loop, and reaching it is reported rather than silently dropped.
const MAX_PLAN_ATTEMPTS: usize = 4;

/// What publishing a plan did.
enum PublishOutcome {
    Published(Revision),
    /// The published view moved while this plan was being formed, so the plan was
    /// discarded without writing anything. Re-deriving it against the view that won is
    /// both possible and useful: the winner is this same catalog.
    Superseded,
    /// The **durable** revision moved ahead of the published view. Nothing was written;
    /// the committed view has to be restored before another plan is derived.
    RevisionMoved,
    /// This instance no longer owns the workspace's index (a reopen claimed it, or the
    /// workspace is gone). Nothing was written and nothing will be: re-reading cannot make
    /// this instance the owner.
    InstanceLost(&'static str),
}

/// The comparable facts of one partition summary. Two summaries with equal facts
/// describe the same durable row, so the row does not have to be rewritten.
trait PartitionFacts {
    fn facts(&self) -> (u64, u64, CatalogIndexMode);
}

impl PartitionFacts for CatalogPartitionSummary {
    fn facts(&self) -> (u64, u64, CatalogIndexMode) {
        (self.file_count, self.indexable_text_bytes, self.index_mode)
    }
}

/// Works out every change the update implies without touching the published view.
#[allow(clippy::too_many_arguments)]
fn plan_changes(
    state: &CatalogView,
    resolver: &SecurePathResolver,
    options: &CatalogOptions,
    changes: &[(WorkspacePath, CatalogChange)],
    writes_missing_text: bool,
) -> Result<CatalogPlan, WorkspaceError> {
    let mut plan = CatalogPlan { writes_missing_text, ..CatalogPlan::default() };
    let mut pending = Vec::new();
    let mut stats = BTreeMap::new();
    for (path, change) in changes {
        // A removal is an observation, not an instruction: the watcher and the write
        // path describe the same directory from two directions, so a removal can arrive
        // after the path was created again. The path itself is the bounded check — when
        // it is really gone the subtree is dropped, and when it is back the observation
        // has been overtaken, so it is handled exactly like a change to that path. That
        // keeps the model-visible index correct immediately instead of waiting for a
        // later event that may never come, and it costs one existence check per removed
        // path rather than a rebuild.
        let change = match change {
            CatalogChange::Removed if path_exists(resolver, path) => CatalogChange::Changed,
            other => *other,
        };
        match change {
            CatalogChange::Removed => remove_subtree(state, path, &mut plan.removed),
            CatalogChange::Changed if path.is_root() => {
                // The root is never an entry of its own: a change reported there can
                // only be described as its listing.
                plan.child_scopes.push(path.clone());
                let mut ignores = inherited_ignores(resolver, path)?;
                list_directory_once(
                    resolver,
                    path,
                    &partition_for(state, path),
                    &mut ignores,
                    &mut pending,
                    &mut stats,
                    &mut plan.watch_directories,
                )?;
            }
            CatalogChange::Changed => {
                let parent = parent_of(path)?;
                if parent.is_root() || is_catalogued_directory(state, &parent) {
                    plan.checked.insert(path.clone());
                    let mut ignores = inherited_ignores(resolver, &parent)?;
                    pending.extend(scan_directory_entries(
                        resolver,
                        &parent,
                        &partition_for(state, &parent),
                        &mut ignores,
                        &BTreeSet::from([path.clone()]),
                    )?);
                } else if resolver
                    .resolve_existing(&parent)
                    .is_ok_and(|resolved| resolved.info.is_directory)
                {
                    // A directory the index has never seen. Its subtree has to be
                    // indexed for this path to have a place in the index at all,
                    // and the scope is exactly that new subtree.
                    rescan_subtree(state, resolver, &mut plan, &mut pending, &mut stats, &parent)?;
                } else {
                    // The containing directory is gone, so nothing about this path
                    // belongs in the index any more.
                    remove_subtree(state, path, &mut plan.removed);
                }
            }
            CatalogChange::DirectoryListed | CatalogChange::DirectoryRescanned => {
                let is_directory = path.is_root()
                    || is_catalogued_directory(state, path)
                    || resolver
                        .resolve_existing(path)
                        .is_ok_and(|resolved| resolved.info.is_directory);
                if !is_directory {
                    // A stale event for a path that is not a directory: re-check it
                    // exactly, and drop any subtree the index still records there.
                    if is_catalogued_directory(state, path) {
                        remove_subtree(state, path, &mut plan.removed);
                    }
                    plan.checked.insert(path.clone());
                    let parent = parent_of(path)?;
                    let mut ignores = inherited_ignores(resolver, &parent)?;
                    pending.extend(scan_directory_entries(
                        resolver,
                        &parent,
                        &partition_for(state, &parent),
                        &mut ignores,
                        &BTreeSet::from([path.clone()]),
                    )?);
                } else if matches!(change, CatalogChange::DirectoryListed) {
                    plan.child_scopes.push(path.clone());
                    // The directory that reported the change is watched in its own
                    // right; see `rescan_subtree` for why the scope is pushed and not
                    // only its children.
                    plan.watch_directories.push(resolver.absolute_path(path));
                    let mut ignores = inherited_ignores(resolver, path)?;
                    list_directory_once(
                        resolver,
                        path,
                        &partition_for(state, path),
                        &mut ignores,
                        &mut pending,
                        &mut stats,
                        &mut plan.watch_directories,
                    )?;
                    refresh_directory_entry(state, resolver, path, &mut pending)?;
                } else {
                    rescan_subtree(state, resolver, &mut plan, &mut pending, &mut stats, path)?;
                }
            }
        }
    }
    // Reading the fresh entries first also decides what the authoritative scopes
    // reproduced: a catalogued path inside such a scope which the fresh listing did
    // not produce is gone from disk. Without this the index keeps reporting files
    // that no longer exist.
    publish_pending(state, resolver, options, &mut plan, pending)?;
    for directory in &plan.subtree_scopes {
        for path in state.entries.keys() {
            if is_within(path, directory) && !plan.reproduced.contains(path) {
                plan.removed.insert(path.clone());
            }
        }
    }
    for directory in &plan.child_scopes {
        for (path, entry) in &state.entries {
            if entry.parent_path == *directory && !plan.reproduced.contains(path) {
                plan.removed.insert(path.clone());
            }
        }
    }
    for path in &plan.checked {
        if !plan.reproduced.contains(path) {
            plan.removed.insert(path.clone());
        }
    }
    // A path can appear in both sets when one update moved it; the removal wins so
    // a stale entry never survives its own replacement.
    for path in &plan.removed {
        plan.upserts.remove(path);
        plan.indexed.remove(path);
    }
    Ok(plan)
}

/// Reads the entries a bounded scan produced and decides, per path, whether the
/// index has anything to change.
///
/// Two questions are deliberately kept apart, because they have different answers:
///
/// - *does this path have to be read again?* A fresh listing carries the file's
///   length, its modification time and whether it still qualifies for indexing, but
///   not its bytes. When a recorded entry already holds the digest of exactly those
///   bytes for that same length and time, the file cannot have changed without one of
///   those facts changing too, so re-reading it would reproduce the recorded entry.
///   That is the whole reason an access-control change — which touches every object
///   in the tree and reports a modification for each one — does not turn into a read
///   of the whole workspace. Every other file is read.
/// - *does this path have to be written again?* The answer comes only from the
///   complete observable facts, compared after hydration. A file whose bytes and
///   encoding are unchanged is a no-op: no durable row, no text-index rewrite, no
///   new revision. Deciding this before the content was read is what made the old
///   path rewrite whole directories that had not changed.
///
/// The read decision therefore never replaces the write decision. A file that was
/// read is still compared in full, and a file whose recorded entry is missing the
/// text facts a full-text partition should hold is read even when its length and
/// time match, because the recorded row does not yet say what those facts are.
fn publish_pending(
    state: &CatalogView,
    resolver: &SecurePathResolver,
    options: &CatalogOptions,
    plan: &mut CatalogPlan,
    pending: Vec<PendingEntry>,
) -> Result<(), WorkspaceError> {
    let mut pending = pending;
    // Every path a fresh listing produced is reproduced, including the copies that are
    // about to be merged away: what matters is that the path exists on disk.
    for entry in &mut pending {
        entry.listed = true;
        plan.reproduced.insert(entry.record.path.clone());
    }
    // One path can be described more than once inside a single update — the write path
    // and the watcher name the same file, and a changed path also re-reads its parent's
    // listing. The last description of a path wins, exactly as the durable upsert
    // would, so the earlier ones are never read and never decide anything.
    let mut latest = BTreeMap::new();
    let mut described = Vec::with_capacity(pending.len());
    for (index, entry) in pending.drain(..).enumerate() {
        if let Some(previous) = latest.insert(entry.record.path.clone(), index) {
            described[previous] = None;
            plan.duplicates = plan.duplicates.saturating_add(1);
        }
        described.push(Some(entry));
    }
    let described = described.into_iter().flatten().collect::<Vec<_>>();
    // The partition modes are decided from every entry the scan produced — not only the
    // ones this update is about to write. A partition's mode depends on all of its
    // files, so a mode computed from one written path would report "still small" for a
    // partition that just grew past its budget.
    let modes = pending_partition_modes(options, state, &described);
    for entry in described {
        let mut entry = entry;
        let full_text = modes.get(&entry.record.partition_path).copied().unwrap_or(true);
        if plan.removed.contains(&entry.record.path) {
            continue;
        }
        match state.entries.get(&entry.record.path) {
            // The index has never recorded this path, so there is nothing to compare
            // against and it has to be read and written.
            None => {
                plan.read = plan.read.saturating_add(1);
                plan.reasons.record(CatalogReadReason::NoRecordedEntry);
                hydrate_entry(resolver, options, &mut entry, full_text)?;
                plan.indexed.insert(entry.record.path.clone(), entry.record.content.take());
                plan.upserts.insert(entry.record.path.clone(), CatalogEntry::from_record(&entry));
            }
            Some(recorded) => {
                let Some(reason) = read_reason(state, recorded, &entry, full_text) else {
                    // The recorded facts already say everything a listing can say about
                    // this path, so there was nothing to read and nothing to write.
                    continue;
                };
                plan.read = plan.read.saturating_add(1);
                plan.reasons.record(reason);
                let recorded = recorded.clone();
                hydrate_entry(resolver, options, &mut entry, full_text)?;
                if entry_unchanged(state, &recorded, &entry, full_text, plan.writes_missing_text) {
                    // Read, and the bytes, the encoding and every listed fact still
                    // match. There is nothing to write, so nothing is written.
                    plan.no_op = plan.no_op.saturating_add(1);
                    continue;
                }
                plan.indexed.insert(entry.record.path.clone(), entry.record.content.take());
                plan.upserts.insert(entry.record.path.clone(), CatalogEntry::from_record(&entry));
            }
        }
    }
    Ok(())
}

/// Whether this path has to be read again, and the bounded reason why.
///
/// `None` means there is nothing a read could add. **A file that was described by a
/// change notification is never in that state.** A listing carries a file's length and
/// its modification time, and those two facts cannot prove the bytes are the same: a
/// rewrite that keeps the length and puts the time back produces a listing that is
/// indistinguishable from an untouched file, and a real `Changed` notification for
/// exactly that case was observed to leave a stale line in the index while reporting
/// itself as a no-op. Treating it as settled is not a trade-off the index is allowed to
/// make, because the whole point of the index is that what it holds is what is on disk.
///
/// So the read decision is now only about paths a notification did **not** describe:
/// text facts that already came from a read of this very file, and non-text paths whose
/// own listed facts are unchanged. Everything else is read, and the decision to write
/// nothing is taken afterwards, from the complete observable facts. Skipping the
/// rewrite is what the incident needed — a text row that is already correct is not
/// deleted and re-inserted — and that decision is unchanged.
fn read_reason(
    state: &CatalogView,
    recorded: &CatalogEntry,
    fresh: &PendingEntry,
    full_text: bool,
) -> Option<CatalogReadReason> {
    let record = &fresh.record;
    let listed = fresh.listed;
    let listed_facts_differ = !listing_facts_unchanged(recorded, fresh);
    if record.kind == CatalogEntryKind::File
        && state
            .partitions
            .iter()
            .find(|partition| partition.partition_path == record.partition_path)
            .is_some_and(|partition| {
                (partition.index_mode == CatalogIndexMode::FullText) != full_text
            })
    {
        // The partition changed how much of a file it keeps, so the stored text facts
        // have to be rebuilt even when the file itself did not change.
        return Some(CatalogReadReason::PartitionModeChanged);
    }
    let indexed_as_text =
        record.kind == CatalogEntryKind::File && full_text && !record.hidden_from_search;
    if !indexed_as_text {
        // Nothing about this path is held as text, so the listed facts decide
        // everything — unless the content facts in hand are the stale remains of a
        // partition that used to index it.
        let stale_text =
            !listed && (recorded.content_hash.is_some() || recorded.text_encoding.is_some());
        return (listed_facts_differ || stale_text)
            .then_some(CatalogReadReason::PartitionWithoutText);
    }
    if !listed {
        // These content facts came from a read of this very file, during this update.
        return None;
    }
    if !fresh.text_candidate || record.byte_length.is_none() {
        // The file has a body but this build does not index its kind as text, so its
        // bytes are not what the index stores. Re-read only if its own facts moved.
        return listed_facts_differ.then_some(CatalogReadReason::PartitionWithoutText);
    }
    // A text file a change notification described. It is always read, whatever its
    // length and modification time say: only the bytes can settle whether the index
    // still matches the disk.
    if recorded.content_hash.is_none() {
        return Some(CatalogReadReason::NoRecordedDigest);
    }
    if recorded.text_encoding.is_none() {
        return Some(CatalogReadReason::MissingRecordedText);
    }
    Some(CatalogReadReason::ContentNotProvenById)
}

/// True when every fact a fresh listing produced matches the recorded entry.
///
/// The listed facts are the ones a directory enumeration can produce: kind, place,
/// length, modification time, ignore and search classification. The content-derived
/// facts are deliberately absent here — a listing never fills in a digest or an
/// encoding, so comparing them against a fresh listing compares two empty fields. That
/// was the original defect: `entry_unchanged` ran before hydration, so for every
/// indexed text file it compared `None` to `None` for the hash, found the two "equal"
/// and skipped the entry — or, once the guard was tightened naively, read and rewrote
/// the whole directory. Neither answer comes from the file; both come from comparing
/// the wrong fields.
fn listing_facts_unchanged(recorded: &CatalogEntry, fresh: &PendingEntry) -> bool {
    let record = &fresh.record;
    recorded.kind == record.kind
        && recorded.parent_path == record.parent_path
        && recorded.file_name == record.file_name
        && recorded.partition_path == record.partition_path
        && recorded.byte_length == record.byte_length
        && recorded.modified_at == record.modified_at
        && recorded.ignored == record.ignored
        && recorded.generated == record.generated
        && recorded.hidden_from_search == record.hidden_from_search
        && recorded.reparse_tag == record.reparse_tag
}

/// True when the recorded entry and the freshly read one describe the same file,
/// including whether the partition still stores its text.
///
/// Every observable fact is compared, and the two content-derived facts — the digest
/// and the encoding — are only available because the file was read. This is the
/// decision that keeps an unchanged file out of the durable write; the earlier
/// question of whether to read at all is [`read_reason`]'s, and it never substitutes
/// for this comparison.
fn entry_unchanged(
    state: &CatalogView,
    recorded: &CatalogEntry,
    fresh: &PendingEntry,
    full_text: bool,
    writes_missing_text: bool,
) -> bool {
    let record = &fresh.record;
    if record.kind == CatalogEntryKind::File
        && state
            .partitions
            .iter()
            .find(|partition| partition.partition_path == record.partition_path)
            .is_some_and(|partition| {
                (partition.index_mode == CatalogIndexMode::FullText) != full_text
            })
    {
        return false;
    }
    // "The facts match" is not the same as "the index holds the text" — but that
    // distinction is only available once the set of paths that really have text has been
    // loaded. A catalog that was not told does not pretend to know, or every unchanged
    // file would be written again.
    let expects_text = record.kind == CatalogEntryKind::File && full_text;
    if writes_missing_text {
        // This update exists to write the text the index is missing, so nothing about the
        // file can make it a no-op.
        return false;
    }
    if state.text_rows_known
        && expects_text
        && record.content_hash.is_some()
        && !state.indexed_paths.contains(&record.path)
    {
        return false;
    }
    listing_facts_unchanged(recorded, fresh)
        && recorded.content_hash == record.content_hash
        && recorded.text_encoding == record.text_encoding
}

/// Re-reads one directory's whole subtree and records it as an authoritative
/// scope.
fn rescan_subtree(
    state: &CatalogView,
    resolver: &SecurePathResolver,
    plan: &mut CatalogPlan,
    pending: &mut Vec<PendingEntry>,
    stats: &mut BTreeMap<WorkspacePath, PartitionStats>,
    directory: &WorkspacePath,
) -> Result<(), WorkspaceError> {
    plan.subtree_scopes.push(directory.clone());
    // The scope itself has to be registered, not only the directories found inside it.
    // A parent's registration is not recursive, so a change to one of this directory's
    // own children is reported by this directory's registration and by nothing else —
    // and a directory that is indexed but not registered is exactly how a new directory
    // became invisible to the watcher for the rest of the session.
    plan.watch_directories.push(resolver.absolute_path(directory));
    let mut ignores = inherited_ignores(resolver, directory)?;
    scan_directory(
        resolver,
        directory.clone(),
        partition_for(state, directory),
        &mut ignores,
        pending,
        stats,
        &mut plan.watch_directories,
    )?;
    refresh_directory_entry(state, resolver, directory, pending)
}

/// Re-reads the scope's own entry through its parent listing: a directory's
/// listing decides its direct children, and the scope is one of them. Without
/// this the scope's own metadata would stay stale after its contents changed, and
/// an authoritative subtree would drop its own root entry.
fn refresh_directory_entry(
    state: &CatalogView,
    resolver: &SecurePathResolver,
    directory: &WorkspacePath,
    pending: &mut Vec<PendingEntry>,
) -> Result<(), WorkspaceError> {
    if directory.is_root() {
        // The root is never stored as an entry of its own.
        return Ok(());
    }
    let parent = parent_of(directory)?;
    if !(parent.is_root() || is_catalogued_directory(state, &parent)) {
        return Ok(());
    }
    let mut ignores = inherited_ignores(resolver, &parent)?;
    pending.extend(scan_directory_entries(
        resolver,
        &parent,
        &partition_for(state, &parent),
        &mut ignores,
        &BTreeSet::from([directory.clone()]),
    )?);
    Ok(())
}

/// The partition summaries the published view would have after this update.
///
/// The summaries are derived from the entry set, so the candidate set is walked
/// once: recorded entries minus the removals, with the new entries substituted for
/// the ones they replace. The walk is in memory only — the point of the bounded
/// update is that no file outside the described scope is read.
fn candidate_partitions(
    state: &CatalogView,
    plan: &CatalogPlan,
    options: &CatalogOptions,
) -> Vec<CatalogPartitionSummary> {
    let mut stats = BTreeMap::<WorkspacePath, PartitionStats>::new();
    let mut tally = |entry: &CatalogEntry| {
        let bucket = stats.entry(entry.partition_path.clone()).or_default();
        if entry.kind == CatalogEntryKind::File {
            bucket.file_count = bucket.file_count.saturating_add(1);
            if is_text_candidate(&entry.file_name) {
                bucket.indexable_text_bytes =
                    bucket.indexable_text_bytes.saturating_add(entry.byte_length.unwrap_or(0));
            }
        }
    };
    for (path, entry) in &state.entries {
        if plan.removed.contains(path) {
            continue;
        }
        tally(plan.upserts.get(path).unwrap_or(entry));
    }
    for (path, entry) in &plan.upserts {
        if !state.entries.contains_key(path) {
            tally(entry);
        }
    }
    stats
        .into_iter()
        .map(|(partition_path, stats)| CatalogPartitionSummary {
            index_mode: partition_mode(options, stats.file_count, stats.indexable_text_bytes),
            partition_path,
            file_count: stats.file_count,
            indexable_text_bytes: stats.indexable_text_bytes,
        })
        .collect()
}

/// Whether a path is the scope itself or inside it.
pub(super) fn is_within(path: &WorkspacePath, scope: &WorkspacePath) -> bool {
    if scope.is_root() {
        return true;
    }
    path == scope || path.as_str().starts_with(&format!("{scope}/"))
}

/// Drops every watch registration a removal took with it.
///
/// A watch directory is an absolute path, so it has to be turned back into a
/// workspace-relative one before it can be compared with what was removed. A directory
/// outside the root, or one whose name the workspace path rules reject, is left alone:
/// it was not derived from this index and is not this function's to remove.
fn prune_watch_directories(
    root: &Path,
    directories: &mut Vec<PathBuf>,
    removed: &BTreeSet<WorkspacePath>,
) {
    if removed.is_empty() {
        return;
    }
    directories.retain(|directory| {
        let Ok(relative) = directory.strip_prefix(root) else {
            return true;
        };
        let text = relative.to_string_lossy().replace('\\', "/");
        let Ok(path) = WorkspacePath::parse(if text.is_empty() {
            WorkspacePath::ROOT.to_owned()
        } else {
            text
        }) else {
            return true;
        };
        !removed.iter().any(|gone| is_within(&path, gone))
    });
}

fn partition_mode(
    options: &CatalogOptions,
    file_count: u64,
    indexable_text_bytes: u64,
) -> CatalogIndexMode {
    if file_count <= options.max_partition_files
        && indexable_text_bytes <= options.max_partition_text_bytes
    {
        CatalogIndexMode::FullText
    } else {
        CatalogIndexMode::MetadataOnly
    }
}

fn is_catalogued_directory(state: &CatalogView, path: &WorkspacePath) -> bool {
    state.entries.get(path).is_some_and(|entry| entry.kind == CatalogEntryKind::Directory)
}

fn remove_subtree(
    state: &CatalogView,
    path: &WorkspacePath,
    removed: &mut BTreeSet<WorkspacePath>,
) {
    let prefix = format!("{path}/");
    let mut found = false;
    for entry in state.entries.keys() {
        if entry == path || entry.as_str().starts_with(&prefix) {
            removed.insert(entry.clone());
            found = true;
        }
    }
    if !found {
        // A path the catalog never recorded still has to leave the durable store
        // so a late delta cannot leave a removed entry behind.
        removed.insert(path.clone());
    }
}

/// Whether a path a change named still exists.
///
/// This is the bounded check that separates a real removal from an observation that
/// has been overtaken. A path that cannot be opened for any reason counts as absent,
/// matching how the other change descriptions treat a path they cannot resolve: the
/// change is then honoured as described rather than silently ignored.
fn path_exists(resolver: &SecurePathResolver, path: &WorkspacePath) -> bool {
    resolver.open_existing(path, aura_windows_platform::NativeFileAccess::Metadata).is_ok()
}

fn parent_of(path: &WorkspacePath) -> Result<WorkspacePath, WorkspaceError> {
    let mut components = path.as_str().split('/').collect::<Vec<_>>();
    components.pop();
    if components.is_empty() {
        return WorkspacePath::parse(WorkspacePath::ROOT)
            .map_err(|error| WorkspaceError::Watcher(format!("unsafe workspace entry: {error}")));
    }
    WorkspacePath::parse(components.join("/"))
        .map_err(|error| WorkspaceError::Watcher(format!("unsafe workspace entry: {error}")))
}

/// The partition a directory inherits. A recorded partition wins; otherwise the
/// nearest recorded ancestor decides, which keeps a nested package root intact.
fn partition_for(state: &CatalogView, directory: &WorkspacePath) -> WorkspacePath {
    if let Some(entry) = state.entries.get(directory)
        && entry.kind == CatalogEntryKind::Directory
        && entry.partition_path != *directory
    {
        return entry.partition_path.clone();
    }
    if directory.is_root() {
        return WorkspacePath::parse(WorkspacePath::ROOT).expect("root path");
    }
    if is_catalogued_directory(state, directory) {
        return directory.clone();
    }
    if let Ok(parent) = parent_of(directory)
        && !parent.is_root()
        && state.entries.contains_key(&parent)
    {
        return partition_for(state, &parent);
    }
    WorkspacePath::parse(WorkspacePath::ROOT).expect("root path")
}

/// A bounded scan produces entries but no partition summaries yet, so the index
/// mode for each touched partition is derived from the entries it just produced
/// combined with what the catalog already records for that partition.
fn pending_partition_modes(
    options: &CatalogOptions,
    state: &CatalogView,
    pending: &[PendingEntry],
) -> BTreeMap<WorkspacePath, bool> {
    let touched =
        pending.iter().map(|entry| entry.record.partition_path.clone()).collect::<BTreeSet<_>>();
    let mut modes = BTreeMap::new();
    for path in touched {
        // Recompute the whole partition exactly, replacing the rescanning
        // directory's contribution with the fresh entries.
        let replaced = pending
            .iter()
            .filter(|entry| entry.record.partition_path == path)
            .map(|entry| entry.record.path.clone())
            .collect::<BTreeSet<_>>();
        let mut files = 0_u64;
        let mut bytes = 0_u64;
        for entry in state.entries.values() {
            if entry.partition_path != path || replaced.contains(&entry.path) {
                continue;
            }
            if entry.kind == CatalogEntryKind::File {
                files = files.saturating_add(1);
                if is_text_candidate(&entry.file_name) {
                    bytes = bytes.saturating_add(entry.byte_length.unwrap_or(0));
                }
            }
        }
        for entry in pending.iter().filter(|entry| entry.record.partition_path == path) {
            if entry.record.kind == CatalogEntryKind::File {
                files = files.saturating_add(1);
                if entry.text_candidate {
                    bytes = bytes.saturating_add(entry.record.byte_length.unwrap_or(0));
                }
            }
        }
        modes.insert(path, partition_mode(options, files, bytes) == CatalogIndexMode::FullText);
    }
    modes
}
