//! Opening an existing index: reuse every fact a proof still covers, re-read only
//! what nothing covers.
//!
//! A full [`PathCatalog::rebuild`](super::scan::PathCatalog::rebuild) reads, decodes
//! and hashes the text of every file in the workspace on every open. On a real
//! development workspace that is the largest single piece of a cold start — measured
//! at 18.1 s of a 19.0 s workspace open over 14 004 entries, and at 6.4 s of a 6.8 s
//! open over a synthetic 14 123-entry tree — and almost all of it is work the
//! previous open already did, on files that did not change while the Core was down.
//!
//! This pass walks the same tree the rebuild walks, so it still proves what a rebuild
//! proves: every path that exists is in the index, and every path that does not is
//! removed. What it does not do is read a file whose stored facts a *proof* still
//! covers. The proof is
//!
//! * the object identity the row was written for (volume serial and file id),
//! * the exact size,
//! * the volume's change generation for that file, read through a handle that
//!   excluded every writer,
//! * and, once per pass, a demonstration that the volume's counter still responds to
//!   a write this process made — the closest thing to the journal-instance query a
//!   non-elevated process can obtain (`FSCTL_QUERY_USN_JOURNAL` needs rights it does
//!   not have; measured here: `ERROR_INVALID_FUNCTION` on an attribute-only volume
//!   handle, `ERROR_ACCESS_DENIED` on a read handle).
//!
//! The exclusive handle is what makes the value mean anything. A write needs a handle
//! with write access, so a rewrite is either refused while the witness is held — the
//! file is read again — or it has already advanced the generation by the time a later
//! witness reads it. The number alone is not that proof: measured on NTFS, a second
//! same-length write inside one still-open write handle does not advance it at all.
//!
//! Nothing here degrades to "the size and the modification time match". A file whose
//! generation cannot be read, whose volume cannot be shown live in this pass, or whose
//! row carries no generation at all — every row written before this column existed —
//! is read again, and the pass reports how many that was.
use std::{collections::BTreeMap, path::PathBuf};

use aura_domain_common::{CorrelationId, Revision, WorkspacePath};
use aura_persistence::{
    CatalogCommitOutcome, CatalogCommitPosition, CatalogCommitRefusal, CatalogPartitionRecord,
};
use aura_windows_platform::ChangeWitnessOutcome;
use aura_workspace_domain::{CatalogIndexMode, CatalogPartitionSummary};

use super::delta::CatalogView;
use super::scan::{
    CatalogEntry, CatalogOptions, PartitionStats, PathCatalog, PendingEntry, adopt_text,
    hydrate_entry, scan_directory, text_holding_paths, wants_text,
};
use crate::{SecurePathResolver, WorkspaceError};

/// What one open reused and what it had to re-read. Counts only: no path, no
/// content, no identity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct CatalogReconcileReport {
    /// Files whose stored text a witness still covered, so nothing was read.
    pub reused: u64,
    /// Files whose text was read again: changed, new, or occupied by a writer.
    pub rehydrated: u64,
    /// Of those, the ones read again because their volume could not be shown live.
    pub unproven_volume: u64,
    /// Entries the walk found that the index did not have.
    pub added: u64,
    /// Entries the index had that the walk no longer finds.
    pub removed: u64,
    /// Entries whose listed facts moved, so the persisted row has to be rewritten
    /// even where no text was read.
    pub metadata_changed: u64,
}

/// Whether the facts a directory listing establishes for this entry differ from
/// the row the index holds. A row that disagrees is written again; a row that
/// agrees is left exactly as it is.
fn metadata_differs(previous: &CatalogEntry, entry: &PendingEntry) -> bool {
    previous.kind != entry.record.kind
        || previous.byte_length != entry.record.byte_length
        || previous.modified_at != entry.record.modified_at
        || previous.ignored != entry.record.ignored
        || previous.generated != entry.record.generated
        || previous.hidden_from_search != entry.record.hidden_from_search
        || previous.reparse_tag != entry.record.reparse_tag
}

/// One volume's proof for one pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VolumeProof {
    volume_serial: u64,
    live: bool,
}

impl VolumeProof {
    fn covers(self, volume_serial: u64) -> bool {
        self.live && self.volume_serial == volume_serial
    }

    fn absent() -> Self {
        Self { volume_serial: 0, live: false }
    }
}

struct ReconcileResult {
    entries: BTreeMap<WorkspacePath, CatalogEntry>,
    partitions: Vec<CatalogPartitionSummary>,
    persisted_entries: Vec<aura_persistence::CatalogEntryRecord>,
    persisted_partitions: Vec<CatalogPartitionRecord>,
    watch_directories: Vec<PathBuf>,
    report: CatalogReconcileReport,
}

/// What to do with one listed entry that the index already has.
enum Reuse {
    /// The stored text is still covered by a proof: keep it, read nothing.
    Stored,
    /// Read the file again. `volume_unproven` records why, when the reason was the
    /// volume rather than the file.
    ReRead { volume_unproven: bool },
    /// Not a text entry: nothing to reuse and nothing to read.
    NotIndexed,
}

impl PathCatalog {
    /// Reconcile the persisted catalog with what is on disk now.    ///
    /// Same completeness as `rebuild` — the walk visits every directory a scan
    /// visits, so a path the walk does not see is a path that is gone — with the
    /// text reused wherever the stored proof still holds. A failed pass changes
    /// nothing: the persisted catalog and the in-memory view are replaced only once
    /// the whole result is in hand.
    ///
    /// The protocol is the catalog's one commit protocol: the complete committed view and
    /// its revision are taken in **one** read, the walk happens outside every lock, and
    /// the write is guarded inside its own transaction by the revision the walk started
    /// from and by the instance that claimed this workspace's index. An overtaken pass
    /// therefore cannot destroy durable content on its way to reporting that it lost.
    pub fn reconcile(
        &self,
        resolver: &SecurePathResolver,
        correlation_id: CorrelationId,
    ) -> Result<(Revision, CatalogReconcileReport), WorkspaceError> {
        if resolver.workspace_id() != self.workspace_id {
            return Err(WorkspaceError::ScopeUnavailable {
                path: WorkspacePath::parse(WorkspacePath::ROOT).expect("root path"),
            });
        }
        // One pass, and one refusal: a reconcile is a full walk of the workspace, so
        // re-walking inside this call to answer a race would double an open's cost. A lost
        // baseline is reported with the committed view restored, and the caller decides
        // whether to retry.
        let replans = 0_usize;
        // One read lock, one snapshot: the entries, the partitions and the revision the
        // walk is derived from all come from the same published view. Sampling them
        // separately is how a pass ends up planning against three different points in
        // time.
        let stored = {
            let state =
                self.state.read().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
            CatalogView::of(&state)
        };
        let stored_revision = stored.revision;
        let proof = self.volume_proof();
        let scan = reconcile_workspace(resolver, &self.options, &stored.entries, proof)?;
        // A pass that proved nothing changed has nothing to write: every stored row
        // is already the row this walk would write, and rewriting fourteen thousand
        // unchanged rows on every start is the other half of the cost this pass
        // exists to remove. The walk still ran in full, so the completeness claim is
        // the same one a rebuild makes.
        let unchanged = scan.report.added == 0
            && scan.report.removed == 0
            && scan.report.rehydrated == 0
            && scan.report.metadata_changed == 0
            && scan.partitions == stored.partitions;
        // The first durable answer, taken before the commit coordination: one short read
        // that carries both halves of the condition — the revision the walk was derived
        // from and the instance that owns this workspace's index. It is read code, so it
        // never makes a query wait, and it is what a test parks on to hold a completed
        // walk before its commit section.
        let observed = self.persistence.workspace_catalog_commit_position(self.workspace_id)?;
        if self.position_mismatch(observed, stored_revision).is_some() {
            // The durable facts moved under the walk. Nothing was written; the
            // committed view is restored so this instance is not left describing facts
            // that no longer exist, and the loss is reported to the caller — a
            // reconcile is a full walk, so re-walking inside this call would double an
            // open's cost to answer a race the caller can retry.
            self.restore_committed_view()?;
            return Err(WorkspaceError::CatalogPlanSuperseded { replans });
        }
        // The walk is complete and nothing has been validated or written yet: this is
        // the entry of the commit section, and the point a test parks to observe what a
        // commit does with a plan another writer has already overtaken. See
        // [`PathCatalog::arm_commit_hold`].
        let _commit = self.enter_commit()?;
        // A pass that writes nothing has no transaction of its own to re-check the
        // condition in, so it re-reads it under the commit coordination: the answer it
        // publishes has to describe the durable facts at the moment it publishes. A
        // pass that does write leaves the authoritative check to the guarded
        // transaction below.
        let position = if unchanged {
            let confirmed =
                self.persistence.workspace_catalog_commit_position(self.workspace_id)?;
            if self.position_mismatch(confirmed, stored_revision).is_some() {
                self.restore_committed_view()?;
                return Err(WorkspaceError::CatalogPlanSuperseded { replans });
            }
            confirmed
        } else {
            observed
        };
        // Which paths hold text after this pass, derived from the same values the write
        // uses rather than read back afterwards — a read back could observe another
        // instance's commit and publish facts this pass never wrote.
        let indexed_paths = (!unchanged).then(|| text_holding_paths(&scan.persisted_entries));
        let revision = if unchanged {
            stored_revision
        } else {
            match self.persistence.replace_workspace_catalog_guarded(
                self.workspace_id,
                position,
                scan.persisted_entries,
                scan.persisted_partitions,
                correlation_id,
            )? {
                CatalogCommitOutcome::Applied(revision) => revision,
                CatalogCommitOutcome::Refused(CatalogCommitRefusal::RevisionMoved { .. }) => {
                    // The walk's baseline was overtaken before the write could use it.
                    // Nothing was written; the committed view is restored so this
                    // instance keeps describing facts that exist.
                    self.restore_committed_view()?;
                    return Err(WorkspaceError::CatalogPlanSuperseded { replans });
                }
                // The guard runs inside the writing transaction, so this is the durable
                // facts saying no for a reason re-reading cannot fix.
                CatalogCommitOutcome::Refused(
                    CatalogCommitRefusal::InstanceReplaced { .. }
                    | CatalogCommitRefusal::WorkspaceMissing,
                ) => {
                    return Err(WorkspaceError::CatalogPlanSuperseded { replans });
                }
            }
        };
        // The published view is swapped, not waited for: no storage call happens under
        // the state lock, so a query answers from the previous committed view while this
        // one publishes — and from this one immediately afterwards.
        let mut state =
            self.state.write().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
        // The walk planned against `stored`. If the published view moved while it ran,
        // the newer view's facts are the ones to keep for the paths this pass did not
        // itself re-derive: publishing `scan.entries` wholesale would roll them back.
        if state.revision != stored_revision {
            drop(state);
            self.restore_committed_view()?;
            return Err(WorkspaceError::CatalogPlanSuperseded { replans });
        }
        if let Some(indexed_paths) = indexed_paths {
            state.entries = scan.entries;
            state.partitions = scan.partitions;
            state.indexed_paths = indexed_paths;
        }
        state.revision = revision;
        state.watch_directories = scan.watch_directories;
        state.text_rows_known = true;
        drop(state);
        Ok((revision, scan.report))
    }

    /// Which half of the durable position no longer matches, or `None` when it does.
    fn position_mismatch(
        &self,
        position: CatalogCommitPosition,
        revision: Revision,
    ) -> Option<&'static str> {
        if position.instance_seq != self.catalog_instance() {
            return Some("instance_replaced");
        }
        if position.catalog_revision != revision {
            return Some("revision_moved");
        }
        None
    }

    /// Whether this pass may believe a change generation at all: its own sentinel
    /// write has to move the volume's counter on the volume the sentinel lives on.
    fn volume_proof(&self) -> VolumeProof {
        let Some(root) = self.persistence.database_path().parent() else {
            return VolumeProof::absent();
        };
        match aura_windows_platform::probe_change_epoch(&root.join(CHANGE_EPOCH_SENTINEL)) {
            Ok(Some(epoch)) => VolumeProof { volume_serial: epoch.volume_serial, live: true },
            _ => VolumeProof::absent(),
        }
    }
}

/// Written in the data root beside the database it protects. One small file, one
/// write and two reads per open that has anything to reuse.
const CHANGE_EPOCH_SENTINEL: &str = "workspace-index-change-epoch.bin";

fn reconcile_workspace(
    resolver: &SecurePathResolver,
    options: &CatalogOptions,
    stored: &BTreeMap<WorkspacePath, CatalogEntry>,
    proof: VolumeProof,
) -> Result<ReconcileResult, WorkspaceError> {
    resolver.verify_root_identity()?;
    let mut pending = Vec::new();
    let mut stats = BTreeMap::<WorkspacePath, PartitionStats>::new();
    let mut watch_directories = vec![resolver.canonical_root().to_path_buf()];
    let mut ignores = Vec::new();
    let root = WorkspacePath::parse(WorkspacePath::ROOT).expect("root path");
    scan_directory(
        resolver,
        root.clone(),
        root,
        &mut ignores,
        &mut pending,
        &mut stats,
        &mut watch_directories,
    )?;
    let modes = stats
        .iter()
        .map(|(path, stats)| {
            let mode = if stats.file_count <= options.max_partition_files
                && stats.indexable_text_bytes <= options.max_partition_text_bytes
            {
                CatalogIndexMode::FullText
            } else {
                CatalogIndexMode::MetadataOnly
            };
            (path.clone(), mode)
        })
        .collect::<BTreeMap<_, _>>();

    let mut report = CatalogReconcileReport::default();
    let decisions = decide_reuse(resolver, &pending, stored, &modes, options, proof);
    for (entry, decision) in pending.iter_mut().zip(decisions) {
        let full_text =
            modes.get(&entry.record.partition_path) == Some(&CatalogIndexMode::FullText);
        let previous = stored.get(&entry.record.path);
        match previous {
            None => report.added += 1,
            Some(previous) => {
                // A row whose listed facts moved has to be written even when no text
                // was read: the persisted row still describes the file as it was.
                if metadata_differs(previous, entry) {
                    report.metadata_changed += 1;
                }
            }
        }
        match decision {
            Reuse::Stored => {
                let previous = previous.expect("a stored decision has a stored entry");
                entry.listed = false;
                entry.record.content_hash = previous.content_hash.clone();
                entry.record.text_encoding = previous.text_encoding;
                entry.record.change_usn = previous.change_usn;
                report.reused += 1;
            }
            Reuse::ReRead { volume_unproven } => {
                if volume_unproven {
                    report.unproven_volume += 1;
                }
                rehydrate(resolver, options, entry, full_text, proof)?;
                report.rehydrated += 1;
            }
            Reuse::NotIndexed => {
                entry.listed = false;
                entry.record.content_hash = None;
                entry.record.text_encoding = None;
                entry.record.change_usn = None;
            }
        }
    }
    let live = pending
        .iter()
        .map(|entry| entry.record.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    report.removed = stored.keys().filter(|path| !live.contains(*path)).count() as u64;

    let entries = pending
        .iter()
        .map(|entry| (entry.record.path.clone(), CatalogEntry::from_record(entry)))
        .collect();
    let persisted_entries = pending.into_iter().map(|entry| entry.record).collect();
    let partitions = stats
        .into_iter()
        .map(|(path, stats)| CatalogPartitionSummary {
            index_mode: modes[&path],
            partition_path: path,
            file_count: stats.file_count,
            indexable_text_bytes: stats.indexable_text_bytes,
        })
        .collect::<Vec<_>>();
    let persisted_partitions = partitions
        .iter()
        .map(|partition| CatalogPartitionRecord {
            partition_path: partition.partition_path.clone(),
            file_count: partition.file_count,
            indexable_text_bytes: partition.indexable_text_bytes,
            index_mode: partition.index_mode,
        })
        .collect();
    Ok(ReconcileResult {
        entries,
        partitions,
        persisted_entries,
        persisted_partitions,
        watch_directories,
        report,
    })
}

/// Proofs taken on more threads than this gain nothing on one disk.
const REUSE_THREADS: usize = 8;
/// Below this many entries one thread is quicker than starting several.
const PARALLEL_FROM: usize = 256;

/// The reuse decision of every listed entry, in the walk's order.
///
/// A decision that keeps stored text opens the file
/// to prove it, about 0.4 ms each, and the owner's two workspaces hold fifteen thousand entries:
/// taken one after another they were most of a five-second start. The proofs are independent of
/// each other, so they are taken on several threads; nothing about any decision changes.
fn decide_reuse(
    resolver: &SecurePathResolver,
    pending: &[PendingEntry],
    stored: &BTreeMap<WorkspacePath, CatalogEntry>,
    modes: &BTreeMap<WorkspacePath, CatalogIndexMode>,
    options: &CatalogOptions,
    proof: VolumeProof,
) -> Vec<Reuse> {
    let decide = |entry: &PendingEntry| {
        let full_text =
            modes.get(&entry.record.partition_path) == Some(&CatalogIndexMode::FullText);
        match stored.get(&entry.record.path) {
            Some(previous) => reuse_decision(resolver, entry, previous, options, full_text, proof),
            None => Reuse::ReRead { volume_unproven: false },
        }
    };
    let threads = std::thread::available_parallelism().map_or(1, usize::from).min(REUSE_THREADS);
    if threads <= 1 || pending.len() < PARALLEL_FROM {
        return pending.iter().map(decide).collect();
    }
    std::thread::scope(|scope| {
        let parts = pending
            .chunks(pending.len().div_ceil(threads))
            .map(|part| scope.spawn(move || part.iter().map(decide).collect::<Vec<_>>()))
            .collect::<Vec<_>>();
        parts
            .into_iter()
            .flat_map(|part| part.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
            .collect()
    })
}

/// Decide whether one stored entry still describes the file on disk.
///
/// The listing facts that both the scan and the stored row carry are the cheap
/// filter that decides whether asking for a proof is worth a handle at all. They are
/// not the proof: a rewrite can keep every one of them.
fn reuse_decision(
    resolver: &SecurePathResolver,
    entry: &PendingEntry,
    previous: &CatalogEntry,
    options: &CatalogOptions,
    full_text_partition: bool,
    proof: VolumeProof,
) -> Reuse {
    if !wants_text(entry, options, full_text_partition) {
        return Reuse::NotIndexed;
    }
    if previous.change_usn.is_none()
        || previous.content_hash.is_none()
        || previous.kind != entry.record.kind
        || previous.byte_length != entry.record.byte_length
        || previous.modified_at != entry.record.modified_at
        || previous.hidden_from_search != entry.record.hidden_from_search
        || previous.reparse_tag != entry.record.reparse_tag
        || previous.volume_serial != entry.record.volume_serial
        || previous.file_id != entry.record.file_id
    {
        return Reuse::ReRead { volume_unproven: false };
    }
    let path = resolver.absolute_path(&entry.record.path);
    let Ok(ChangeWitnessOutcome::Witnessed(witness)) =
        aura_windows_platform::witness_change_generation(&path)
    else {
        // Occupied by a writer, or the volume will not describe the file: both are
        // "cannot prove", and both mean the text is read again.
        return Reuse::ReRead { volume_unproven: false };
    };
    let identity = witness.identity();
    if !proof.covers(identity.volume_serial_number) {
        return Reuse::ReRead { volume_unproven: true };
    }
    let volume_serial = identity.volume_serial_number.to_string();
    let file_id: String = identity.file_id.iter().map(|byte| format!("{byte:02x}")).collect();
    if previous.volume_serial.as_deref() != Some(volume_serial.as_str())
        || previous.file_id.as_deref() != Some(file_id.as_str())
        || previous.byte_length != Some(witness.byte_length())
        || previous.change_usn != Some(witness.change_usn())
    {
        return Reuse::ReRead { volume_unproven: false };
    }
    Reuse::Stored
}

/// Read one file again, through the change witness wherever the volume allows it.
///
/// The text and the generation have to describe one state of one file, so the bytes
/// are read through the very handle that excluded writers: no write can land between
/// them. When the witness is refused, the text is still read — the index needs the
/// text — but **no generation is stored**, so the next open reads that file again
/// instead of trusting a claim nobody could check.
fn rehydrate(
    resolver: &SecurePathResolver,
    options: &CatalogOptions,
    entry: &mut PendingEntry,
    full_text_partition: bool,
    proof: VolumeProof,
) -> Result<(), WorkspaceError> {
    if !wants_text(entry, options, full_text_partition) {
        entry.listed = false;
        entry.record.content_hash = None;
        entry.record.text_encoding = None;
        entry.record.change_usn = None;
        return Ok(());
    }
    let path = resolver.absolute_path(&entry.record.path);
    if let Ok(ChangeWitnessOutcome::Witnessed(mut witness)) =
        aura_windows_platform::witness_change_generation(&path)
    {
        let identity = witness.identity().clone();
        let volume_serial = identity.volume_serial_number.to_string();
        let file_id: String = identity.file_id.iter().map(|byte| format!("{byte:02x}")).collect();
        // Read through the witness only when it is the very object the walk
        // listed: a path that now resolves to something else falls back to the
        // containment-verified resolver open, which also stores no proof.
        if entry.record.volume_serial.as_deref() == Some(volume_serial.as_str())
            && entry.record.file_id.as_deref() == Some(file_id.as_str())
        {
            let generation = witness.change_usn();
            let mut bytes = Vec::new();
            entry.listed = false;
            if witness.read_to_end(&mut bytes).is_ok() {
                adopt_text(entry, &bytes);
                // The proof is only stored when this pass could also show the
                // volume's counter live; otherwise the text is kept without one and
                // read again next time.
                if proof.covers(identity.volume_serial_number)
                    && entry.record.content_hash.is_some()
                {
                    entry.record.change_usn = Some(generation);
                }
            }
            return Ok(());
        }
    }
    hydrate_entry(resolver, options, entry, full_text_partition)
}
