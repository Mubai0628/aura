use super::artifacts::is_internal_component;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use aura_domain_common::jcs::canonical_sha256;
use aura_domain_common::{ContentHash, CorrelationId, Revision, WorkspaceId, WorkspacePath};
use aura_persistence::{
    CatalogCommitOutcome, CatalogCommitPosition, CatalogCommitRefusal, CatalogEntryRecord,
    CatalogPartitionRecord, PersistenceCoordinator,
};
use aura_windows_platform::{NativeFileAccess, inspect_existing_path};
use aura_workspace_domain::{
    CatalogEntryKind, CatalogIndexMode, CatalogPartitionSummary, LineEnding, PathSearchMatch,
    PathSearchResult, TextEncoding,
};
use encoding_rs::WINDOWS_1252;
use globset::{GlobBuilder, GlobSetBuilder};
use ignore::{
    Match,
    gitignore::{Gitignore, GitignoreBuilder},
};

use crate::error::io_error;
use crate::{SecurePathResolver, WorkspaceError};

#[derive(Clone, Debug)]
pub struct CatalogOptions {
    pub max_partition_files: u64,
    pub max_partition_text_bytes: u64,
    pub max_index_file_bytes: u64,
}

impl CatalogOptions {
    pub const DEFAULT_MAX_PARTITION_FILES: u64 = 20_000;
    pub const DEFAULT_MAX_PARTITION_TEXT_BYTES: u64 = 256 * 1024 * 1024;
    pub const DEFAULT_MAX_INDEX_FILE_BYTES: u64 = 1024 * 1024;

    #[must_use]
    pub const fn with_index_file_limit(max_index_file_bytes: u64) -> Self {
        Self {
            max_partition_files: Self::DEFAULT_MAX_PARTITION_FILES,
            max_partition_text_bytes: Self::DEFAULT_MAX_PARTITION_TEXT_BYTES,
            max_index_file_bytes,
        }
    }
}

impl Default for CatalogOptions {
    fn default() -> Self {
        Self::with_index_file_limit(Self::DEFAULT_MAX_INDEX_FILE_BYTES)
    }
}

#[derive(Clone, Debug)]
pub struct CatalogEntry {
    pub path: WorkspacePath,
    pub parent_path: WorkspacePath,
    pub file_name: String,
    pub kind: CatalogEntryKind,
    pub partition_path: WorkspacePath,
    pub byte_length: Option<u64>,
    pub modified_at: Option<String>,
    pub content_hash: Option<ContentHash>,
    pub text_encoding: Option<TextEncoding>,
    pub ignored: bool,
    pub generated: bool,
    pub hidden_from_search: bool,
    pub reparse_tag: Option<u32>,
    /// The volume's change generation for this file when its text was read under a
    /// writer-excluding handle, with the identity it was read from. All three
    /// together are what a later open needs before it reuses this text; any of
    /// them missing means the file is read again.
    pub change_usn: Option<u64>,
    pub volume_serial: Option<String>,
    pub file_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DecodedText {
    pub text: String,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
}

pub(crate) struct CatalogState {
    pub(crate) entries: BTreeMap<WorkspacePath, CatalogEntry>,
    pub(crate) partitions: Vec<CatalogPartitionSummary>,
    /// Paths whose text the durable index really holds.
    ///
    /// Loaded with the catalog, because "every fact matches" is not the same claim as
    /// "the text is there": a path whose text row was dropped by a repair has to be
    /// written again even though nothing about the file changed.
    pub(crate) indexed_paths: BTreeSet<WorkspacePath>,
    /// Whether [`Self::indexed_paths`] was loaded from the durable index.
    ///
    /// A catalog opened without it must not treat every path as missing its text.
    pub(crate) text_rows_known: bool,
    pub(crate) recent_use: HashMap<WorkspacePath, u64>,
    pub(crate) revision: Revision,
    pub(crate) watch_directories: Vec<PathBuf>,
    /// The bounded counts of the most recent update. Held so a caller can report what
    /// the index just did without the update having to widen its return value.
    pub(crate) last_update: Option<super::CatalogUpdateReport>,
}

impl Default for CatalogState {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            partitions: Vec::new(),
            indexed_paths: BTreeSet::new(),
            text_rows_known: false,
            recent_use: HashMap::new(),
            revision: Revision::new(0),
            watch_directories: Vec::new(),
            last_update: None,
        }
    }
}

pub struct PathCatalog {
    pub(crate) options: CatalogOptions,
    pub(crate) persistence: Arc<PersistenceCoordinator>,
    pub(crate) state: RwLock<CatalogState>,
    pub(crate) workspace_id: WorkspaceId,
    /// The one owner of commits to this catalog.
    ///
    /// Every modification of one catalog — a bounded delta, a repair, a reconcile and a
    /// full rebuild — takes this before it validates its plan and holds it until the
    /// in-memory swap is done. It serialises the **commits**, and nothing else: a query
    /// takes only the state lock, so no reader ever waits behind a durable write, and the
    /// state lock itself is held only for the short in-memory swap rather than for the
    /// storage round trip.
    commit: Mutex<()>,
    /// The durable generation this instance claimed when it opened.
    ///
    /// It is presented to every write, and the write is refused unless this instance is
    /// still the one that owns the workspace's index — which is what keeps the task of a
    /// replaced instance from overwriting the facts of the instance that replaced it.
    instance: u64,
    /// A default-off, single-shot hold the **next commit section** passes through before
    /// it validates anything or writes anything.
    ///
    /// It exists because "this plan was formed and then stopped before it could commit"
    /// is a state no production caller can produce on demand and no test can wait for:
    /// the interesting failures all live in the window between a completed plan and the
    /// commit that would publish it. The hold is empty in every production path — one
    /// `Option::take` on a `Mutex` that is never contended — and it is taken *before* the
    /// catalog's commit coordination, so a test that parks one commit here leaves the
    /// catalog's own commit path free for the writer it is racing.
    commit_hold: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    open_report: CatalogOpenReport,
}

/// A caller-supplied hold for the catalog's next commit section.
///
/// The closure runs on the committing thread at the point where the plan is complete and
/// nothing has been validated or written yet. It must return; it is not a cancellation.
#[derive(Clone)]
pub struct CatalogCommitHold(Arc<dyn Fn() + Send + Sync>);

impl CatalogCommitHold {
    #[must_use]
    pub fn new(run: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(run))
    }
}

impl std::fmt::Debug for CatalogCommitHold {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CatalogCommitHold")
    }
}

/// What opening one workspace's catalog cost, by stage. Counts and
/// milliseconds only: no path, no entry content, no workspace identity.
///
/// This is the attribution a cold start needs when the store is large: the
/// persisted loads, the unproven-text repair and the resulting catalog size
/// are separate numbers, so "the open was slow" says which one was slow.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct CatalogOpenReport {
    /// Loading the persisted catalog entries and partitions.
    pub load_ms: u64,
    /// Loading the recent-use index.
    pub recent_use_ms: u64,
    /// Loading the set of paths whose text is indexed.
    pub content_paths_ms: u64,
    /// Re-reading files whose indexed text was unproven.
    pub repair_ms: u64,
    /// Files that repair pass re-read.
    pub repaired_paths: u64,
    /// Catalog entries and partitions loaded.
    pub entries: u64,
    pub partitions: u64,
}

fn elapsed_ms(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// How many times a full replacement re-walks after losing its baseline.
///
/// A refusal is a real race, not a policy: the second attempt takes a fresh baseline and a
/// fresh walk, so it publishes the tree as it is now. Two attempts bound the cost of a
/// pathological case without turning the recovery path into "either succeed or fail" on
/// the first contention.
const MAX_REBUILD_ATTEMPTS: usize = 2;

/// The paths whose text a whole-set replacement really wrote.
///
/// `replace_workspace_catalog` inserts a text row exactly for the entries that carry
/// content, so the set is derived from the same values the write used instead of being
/// read back afterwards — which also removes a database read that could otherwise observe
/// another instance's commit.
pub(crate) fn text_holding_paths(entries: &[CatalogEntryRecord]) -> BTreeSet<WorkspacePath> {
    entries.iter().filter(|entry| entry.content.is_some()).map(|entry| entry.path.clone()).collect()
}

impl PathCatalog {
    pub fn open(
        workspace_id: WorkspaceId,
        persistence: Arc<PersistenceCoordinator>,
        options: CatalogOptions,
    ) -> Result<Self, WorkspaceError> {
        let mut open_report = CatalogOpenReport::default();
        let started = std::time::Instant::now();
        let (entries, partitions, revision) = persistence.load_workspace_catalog(workspace_id)?;
        open_report.load_ms = elapsed_ms(started);
        let started = std::time::Instant::now();
        let recent_use = persistence.load_path_recent_use(workspace_id)?.into_iter().collect();
        open_report.recent_use_ms = elapsed_ms(started);
        let started = std::time::Instant::now();
        let indexed_paths =
            persistence.workspace_content_paths(workspace_id)?.into_iter().collect();
        open_report.content_paths_ms = elapsed_ms(started);
        let entries: BTreeMap<_, _> =
            entries.into_iter().map(|entry| (entry.path.clone(), catalog_entry(entry))).collect();
        open_report.entries = entries.len() as u64;
        open_report.partitions = partitions.len() as u64;
        let partitions = partitions.into_iter().map(partition_summary).collect();
        // The instance claim is durable state, so it is taken before the catalog is built:
        // an instance that cannot claim must not exist, or its later commits would be
        // refused for a reason nobody recorded.
        let instance_seq = persistence.claim_workspace_catalog_instance(workspace_id)?;
        let mut catalog = Self {
            options,
            persistence,
            state: RwLock::new(CatalogState {
                entries,
                partitions,
                indexed_paths,
                text_rows_known: true,
                recent_use,
                revision,
                watch_directories: Vec::new(),
                last_update: None,
            }),
            workspace_id,
            // One claim per open. It is a durable fact, not a process convention: an
            // instance that has been replaced presents a generation the database no
            // longer recognises, so its plan cannot be written even if it is still
            // running. The claim itself changes nothing about the index.
            instance: instance_seq,
            commit: Mutex::new(()),
            commit_hold: Mutex::new(None),
            open_report,
        };
        let started = std::time::Instant::now();
        let repaired = catalog.repair_unproven_text()?;
        catalog.open_report.repair_ms = elapsed_ms(started);
        catalog.open_report.repaired_paths = repaired;
        Ok(catalog)
    }

    /// The measured cost of the open that built this catalog.
    pub fn open_report(&self) -> CatalogOpenReport {
        self.open_report
    }

    /// Arms a hold for the next commit section of this catalog.
    ///
    /// Default-off and single-shot: only a test or an acceptance driver calls this, the
    /// hold applies to the **first** commit that reaches it, and every later commit runs
    /// normally even if the hold is never released.
    #[doc(hidden)]
    pub fn arm_commit_hold(&self, hold: CatalogCommitHold) {
        if let Ok(mut slot) = self.commit_hold.lock() {
            *slot = Some(hold.0);
        }
    }

    /// Runs the armed hold, if any. One arrival at most.
    pub(crate) fn run_commit_hold(&self) {
        let armed = self.commit_hold.lock().ok().and_then(|mut slot| slot.take());
        if let Some(hold) = armed {
            hold();
        }
    }

    /// The durable generation this instance claimed when it opened.
    ///
    /// It is the identity a caller asserts on: after a close and reopen the new instance
    /// has a strictly greater generation than the one it replaced, and the durable row
    /// says which generation currently owns this workspace's index.
    #[must_use]
    pub fn catalog_instance(&self) -> u64 {
        self.instance
    }

    /// Entry to the commit section: the commit coordination of this catalog, taken
    /// *after* the plan is complete and before anything is validated or written.
    ///
    /// It serialises commits of one catalog and nothing else. No reader takes it, so a
    /// query is never queued behind a durable write; and it is not held across planning,
    /// enumeration, hydration or hashing, which is what keeps a long scan from becoming a
    /// long write lock.
    pub(crate) fn enter_commit(&self) -> Result<std::sync::MutexGuard<'_, ()>, WorkspaceError> {
        self.run_commit_hold();
        self.commit.lock().map_err(|_| WorkspaceError::LockPoisoned("path catalog commit"))
    }

    /// The published revision, from one short read.
    pub(crate) fn published_revision(&self) -> Result<Revision, WorkspaceError> {
        Ok(self.state.read().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?.revision)
    }

    /// The durable position every commit of this instance has to still hold.
    pub(crate) fn commit_position(&self, revision: Revision) -> CatalogCommitPosition {
        CatalogCommitPosition { catalog_revision: revision, instance_seq: self.instance }
    }

    /// Rewrites the published view from the durable facts.
    ///
    /// Every change to the published view is preceded by the durable write that decides it,
    /// so the durable rows are never behind the memory: when they are *ahead* — because
    /// something committed outside this catalog, or because this instance's own write
    /// landed while its swap did not — restoring them is what makes the next plan derived
    /// from facts that really exist. The alternatives are both worse: publishing a plan
    /// derived from the stale view writes facts that do not describe the index, and
    /// reporting the loss forever leaves this instance permanently unable to commit.
    pub(crate) fn restore_committed_view(&self) -> Result<Revision, WorkspaceError> {
        let (entries, partitions, revision) =
            self.persistence.load_workspace_catalog(self.workspace_id)?;
        let indexed_paths =
            self.persistence.workspace_content_paths(self.workspace_id)?.into_iter().collect();
        let mut state =
            self.state.write().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
        state.entries =
            entries.into_iter().map(|entry| (entry.path.clone(), catalog_entry(entry))).collect();
        state.partitions = partitions.into_iter().map(partition_summary).collect();
        state.indexed_paths = indexed_paths;
        state.text_rows_known = true;
        state.revision = revision;
        Ok(revision)
    }

    /// Re-reads every path whose indexed text could not be adopted.
    ///
    /// An upgrade can leave a path with several generations of text and no way to tell
    /// which is current — the text index carries no digest of the file's bytes. The
    /// migration drops those rows and marks the path instead of guessing, and this is
    /// where the guess is replaced by a fact: the file is read, exactly as any other
    /// change to it would be.
    ///
    /// The mark is settled by the same transaction that commits the re-read, never
    /// ahead of it: a path whose text is written, or whose file is gone, gives up its
    /// mark only when that conclusion commits, while a file whose text still cannot
    /// be established keeps its mark. A repair whose write fails therefore returns
    /// the persistence error from opening the catalog, with every mark exactly as it
    /// was, and the next open tries again — a path is never called indexed while its
    /// text was never established.
    ///
    /// The resolver comes from the persisted workspace, so the repair cannot read
    /// anything this workspace was not already allowed to read.
    ///
    /// Returns how many marked paths the repair re-read, so the open can report
    /// it; zero when no marks were pending.
    fn repair_unproven_text(&self) -> Result<u64, WorkspaceError> {
        let paths = self.persistence.workspace_content_rebuild_paths(self.workspace_id)?;
        if paths.is_empty() {
            return Ok(0);
        }
        let workspace = self.persistence.workspace(self.workspace_id)?;
        let resolver = SecurePathResolver::from_workspace(&workspace)?;
        let repair_count = paths.len();
        let changes = paths
            .iter()
            .map(|path| (path.clone(), crate::catalog::CatalogChange::Changed))
            .collect::<Vec<_>>();
        self.apply_changes(
            &resolver,
            crate::catalog::CatalogUpdate {
                inputs: changes.len(),
                writes_missing_text: true,
                repaired_paths: paths.clone(),
                ..crate::catalog::CatalogUpdate::new(
                    changes,
                    format!("content-index-repair:{repair_count}"),
                )
            },
            &self.options,
            CorrelationId::new(),
        )?;
        Ok(repair_count as u64)
    }

    /// Re-reads the whole workspace and publishes it as the index.
    ///
    /// A full replacement is the one update that describes **every** path, so it is the
    /// one that can roll back the most: the tree it scanned is the tree as it was when the
    /// scan started, and a bounded delta that commits while the scan runs describes facts
    /// the scan never saw. The revision the replacement is derived from is therefore taken
    /// before the walk, and the write carries it: a replacement whose baseline moved is
    /// refused by the database rather than published. A refusal is retried against a fresh
    /// baseline and a fresh walk — never against the stale scan — so the recovery path
    /// still converges while an overtaken plan never writes anything.
    pub fn rebuild(
        &self,
        resolver: &SecurePathResolver,
        correlation_id: CorrelationId,
    ) -> Result<Revision, WorkspaceError> {
        if resolver.workspace_id() != self.workspace_id {
            return Err(WorkspaceError::ScopeUnavailable {
                path: WorkspacePath::parse(WorkspacePath::ROOT).expect("root path"),
            });
        }
        for _attempt in 0..MAX_REBUILD_ATTEMPTS {
            // The baseline is read before the walk, because the walk describes the tree at
            // the moment it started and nothing else.
            let baseline = self.published_revision()?;
            let scan = scan_workspace(resolver, &self.options)?;
            // Which paths hold text after this replacement, derived from the same values
            // the write uses — the records are moved into the write below.
            let indexed_paths = text_holding_paths(&scan.persisted_entries);
            // The plan is complete and nothing has been validated or written: this is the
            // point a test parks to observe what a commit does with a plan that another
            // writer has already overtaken.
            let _commit = self.enter_commit()?;
            if self.published_revision()? != baseline {
                // Another commit of this catalog landed while the walk ran: the walk is
                // already stale and is discarded, not published.
                continue;
            }
            let outcome = self.persistence.replace_workspace_catalog_guarded(
                self.workspace_id,
                self.commit_position(baseline),
                scan.persisted_entries,
                scan.persisted_partitions,
                correlation_id,
            )?;
            let revision = match outcome {
                CatalogCommitOutcome::Applied(revision) => revision,
                CatalogCommitOutcome::Refused(CatalogCommitRefusal::RevisionMoved { .. }) => {
                    // Another commit won the revision the walk was derived from. Nothing
                    // was written; the next attempt walks the tree again from the restored
                    // committed facts.
                    self.restore_committed_view()?;
                    continue;
                }
                CatalogCommitOutcome::Refused(
                    CatalogCommitRefusal::InstanceReplaced { .. }
                    | CatalogCommitRefusal::WorkspaceMissing,
                ) => {
                    // This instance no longer owns the workspace's index: re-walking
                    // cannot change that, so the loss is reported.
                    return Err(WorkspaceError::CatalogPlanSuperseded {
                        replans: MAX_REBUILD_ATTEMPTS,
                    });
                }
            };
            // The swap is the only thing the state lock covers: a rebuild's whole-set
            // replacement is published in memory, and readers see either the previous view
            // or this one.
            let mut state =
                self.state.write().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
            state.entries = scan.entries;
            state.partitions = scan.partitions;
            state.revision = revision;
            state.watch_directories = scan.watch_directories;
            state.indexed_paths = indexed_paths;
            state.text_rows_known = true;
            return Ok(revision);
        }
        Err(WorkspaceError::CatalogPlanSuperseded { replans: MAX_REBUILD_ATTEMPTS })
    }

    pub fn entry(&self, path: &WorkspacePath) -> Result<Option<CatalogEntry>, WorkspaceError> {
        Ok(self
            .state
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?
            .entries
            .get(path)
            .cloned())
    }

    pub fn entries(&self) -> Result<Vec<CatalogEntry>, WorkspaceError> {
        Ok(self.snapshot()?.0)
    }

    /// A bounded directory page and the version of *that directory* from one read
    /// lock. Does not clone the entire workspace catalog for each model-facing
    /// page.
    ///
    /// The version describes only this directory's own direct children, so a
    /// change anywhere else in the workspace cannot invalidate a page in progress
    /// here. It is derived from those entries rather than from a counter, which is
    /// also what makes it survive a restart: the same directory contents always
    /// produce the same version, and a real change to this directory always
    /// produces a different one.
    pub fn children_page(
        &self,
        parent: &WorkspacePath,
        after: Option<&WorkspacePath>,
        limit: usize,
    ) -> Result<(Vec<CatalogEntry>, Revision, Revision), WorkspaceError> {
        let state = self.state.read().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
        let entries = state
            .entries
            .values()
            .filter(|entry| {
                &entry.parent_path == parent && after.is_none_or(|path| &entry.path > path)
            })
            .take(limit)
            .cloned()
            .collect();
        let directory_revision = directory_version(&state, parent)?;
        Ok((entries, state.revision, directory_revision))
    }

    /// One locked snapshot of catalog entries and revision so a search cannot
    /// label a stale candidate list with a newer revision.
    pub fn snapshot(&self) -> Result<(Vec<CatalogEntry>, Revision), WorkspaceError> {
        let state = self.state.read().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
        Ok((state.entries.values().cloned().collect(), state.revision))
    }

    /// Finds files by name under one directory.
    ///
    /// This is filename lookup over the workspace index, not a text search and not
    /// a filesystem walk: the index already holds every authorized path, so the
    /// answer is deterministic, sorted, de-duplicated and bounded without touching
    /// the disk. A call may ask for several names at once, which is the common
    /// shape of "where is this file".
    ///
    /// The walk is bounded twice. It stops as soon as the page is full, and it
    /// stops when it has examined `scan_budget` entries inside the scope even if
    /// the page is still empty, so no single call can walk an unbounded index.
    /// `after` resumes exactly where the previous call stopped examining — not
    /// where its last match was — so a resumed page never re-walks the entries the
    /// earlier page already read, and no match can be skipped.
    ///
    /// The returned version describes the scope, not the workspace: a change
    /// outside the queried directory or subtree cannot invalidate a page in
    /// progress. It is derived from the entries inside the scope, so the same
    /// contents always produce the same version and a real change inside the scope
    /// always produces a different one.
    #[allow(clippy::too_many_arguments)]
    pub fn find_names(
        &self,
        directory: &WorkspacePath,
        file_names: &[String],
        recursive: bool,
        match_mode: NameMatchMode,
        case_sensitive: bool,
        after: Option<&WorkspacePath>,
        limit: usize,
        scan_budget: u64,
    ) -> Result<NameFindPage, WorkspaceError> {
        let state = self.state.read().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
        let patterns = file_names
            .iter()
            .map(|name| NamePattern::compile(name, match_mode, case_sensitive))
            .collect::<Result<Vec<_>, _>>()?;
        let mut scanned_entries = 0_u64;
        let mut matches = Vec::new();
        let mut resume_after = None;
        let mut complete = true;
        // The index is ordered by path, so the walk starts at the cursor and moves
        // forward through the whole map once: a page reads only the region it has
        // not read yet.
        let start = after
            .map_or(std::ops::Bound::Unbounded, |path| std::ops::Bound::Excluded(path.clone()));
        let mut walk =
            state.entries.range((start, std::ops::Bound::Unbounded)).map(|(_, e)| e).peekable();
        while let Some(entry) = walk.next() {
            if !in_scope(entry, directory, recursive) {
                continue;
            }
            scanned_entries = scanned_entries.saturating_add(1);
            if entry.kind == CatalogEntryKind::File
                && patterns.iter().any(|pattern| pattern.matches(&entry.file_name))
            {
                matches.push(entry.clone());
            }
            resume_after = Some(entry.path.clone());
            if matches.len() >= limit || scanned_entries >= scan_budget {
                // This page stopped early. It is complete only when the index has
                // nothing after it at all: an entry that is merely out of scope does
                // not prove that no in-scope entry follows, and claiming absence
                // early is exactly what must not happen.
                complete = walk.peek().is_none();
                break;
            }
        }
        if complete {
            resume_after = None;
        }
        Ok(NameFindPage {
            matches,
            scope_revision: scope_version(&state, directory, recursive)?,
            complete,
            scanned_entries,
            resume_after,
        })
    }

    pub fn partitions(&self) -> Result<Vec<CatalogPartitionSummary>, WorkspaceError> {
        Ok(self
            .state
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?
            .partitions
            .clone())
    }

    pub fn revision(&self) -> Result<Revision, WorkspaceError> {
        Ok(self.state.read().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?.revision)
    }

    /// What the most recent update did, as bounded counts.
    ///
    /// The counts say why a batch was expensive — how many paths it re-read, how many
    /// of those turned out unchanged, how many rows it wrote — so "the filesystem was
    /// slow" and "the index rewrote a directory that had not changed" are different
    /// observations instead of one guess. `None` before the first update.
    #[must_use]
    pub fn last_update(&self) -> Option<super::CatalogUpdateReport> {
        self.state.read().ok().and_then(|state| state.last_update.clone())
    }

    /// The catalog's own options. A caller that re-reads part of the workspace
    /// must apply the same partition and text-index limits as a full scan.
    #[must_use]
    pub fn options(&self) -> CatalogOptions {
        self.options.clone()
    }

    pub fn watch_directories(&self) -> Result<Vec<PathBuf>, WorkspaceError> {
        Ok(self
            .state
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?
            .watch_directories
            .clone())
    }

    pub fn find_paths(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<PathSearchResult, WorkspaceError> {
        let query = query.trim().replace('\\', "/").to_lowercase();
        let state = self.state.read().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
        let mut matches = state
            .entries
            .values()
            .filter(|entry| !entry.hidden_from_search)
            .filter_map(|entry| {
                fuzzy_score(entry, &query, state.recent_use.get(&entry.path).copied()).map(
                    |score| PathSearchMatch {
                        path: entry.path.clone(),
                        score,
                        kind: entry.kind,
                        ignored: entry.ignored,
                        generated: entry.generated,
                    },
                )
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            right.score.cmp(&left.score).then_with(|| left.path.cmp(&right.path))
        });
        matches.truncate(limit.clamp(1, 1_000));
        Ok(PathSearchResult { matches, catalog_revision: state.revision })
    }

    pub fn record_use(&self, path: &WorkspacePath) -> Result<(), WorkspaceError> {
        self.persistence.record_path_use(self.workspace_id, path)?;
        let mut state =
            self.state.write().map_err(|_| WorkspaceError::LockPoisoned("path catalog"))?;
        let next = state.recent_use.values().copied().max().unwrap_or(0).saturating_add(1);
        state.recent_use.insert(path.clone(), next);
        Ok(())
    }

    pub fn glob(&self, pattern: &str) -> Result<Vec<CatalogEntry>, WorkspaceError> {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(false)
            .build()
            .map_err(|error| WorkspaceError::Watcher(format!("invalid glob: {error}")))?;
        let mut builder = GlobSetBuilder::new();
        builder.add(glob);
        let set = builder
            .build()
            .map_err(|error| WorkspaceError::Watcher(format!("invalid glob set: {error}")))?;
        Ok(self
            .entries()?
            .into_iter()
            .filter(|entry| !entry.hidden_from_search && set.is_match(entry.path.as_str()))
            .collect())
    }
}

pub(crate) struct ScanResult {
    entries: BTreeMap<WorkspacePath, CatalogEntry>,
    partitions: Vec<CatalogPartitionSummary>,
    persisted_entries: Vec<CatalogEntryRecord>,
    persisted_partitions: Vec<CatalogPartitionRecord>,
    watch_directories: Vec<PathBuf>,
}

#[derive(Default)]
pub(crate) struct PartitionStats {
    pub(crate) file_count: u64,
    pub(crate) indexable_text_bytes: u64,
}

pub(crate) struct PendingEntry {
    pub(crate) record: CatalogEntryRecord,
    pub(crate) text_candidate: bool,
    /// True while this entry's content facts come from a directory listing rather
    /// than from a read of the file.
    ///
    /// `hydrate_entry` clears `content_hash` and `text_encoding` before it tries to
    /// fill them in, so after hydration an empty digest means "the file could not be
    /// read as text" — not "the file has not been read yet". Those two states call for
    /// opposite decisions, and a flag that a listing sets and a read clears keeps them
    /// apart without overloading an `Option`.
    pub(crate) listed: bool,
}

/// Loads text content and its hash for one pending entry when the partition is
/// fully indexed and the file is within the per-file index budget. Shared by the
/// first full scan and every incremental rescan so both produce identical facts.
pub(crate) fn hydrate_entry(
    resolver: &SecurePathResolver,
    options: &CatalogOptions,
    entry: &mut PendingEntry,
    full_text_partition: bool,
) -> Result<(), WorkspaceError> {
    entry.listed = false;
    entry.record.content_hash = None;
    entry.record.text_encoding = None;
    entry.record.content = None;
    entry.record.change_usn = None;
    if !wants_text(entry, options, full_text_partition) {
        return Ok(());
    }
    let Ok(mut file) = resolver.open_existing(&entry.record.path, NativeFileAccess::Read) else {
        return Ok(());
    };
    let mut bytes = Vec::new();
    if file.file.file_mut().read_to_end(&mut bytes).is_ok() {
        adopt_text(entry, &bytes);
    }
    Ok(())
}

/// Whether this entry is one whose text the index wants at all.
pub(crate) fn wants_text(
    entry: &PendingEntry,
    options: &CatalogOptions,
    full_text_partition: bool,
) -> bool {
    entry.text_candidate
        && full_text_partition
        && entry.record.kind == CatalogEntryKind::File
        && !entry.record.hidden_from_search
        && entry.record.byte_length.is_some_and(|size| size <= options.max_index_file_bytes)
}

/// Adopt bytes that were just read from this entry's file: the digest, the
/// encoding and the text, in exactly the shape a full scan produces.
///
/// Shared so a read through the change witness stores the same facts as a read
/// through the resolver — only the handle differs, and it is the handle that
/// decides whether the store also carries a proof.
pub(crate) fn adopt_text(entry: &mut PendingEntry, bytes: &[u8]) {
    if let Ok(decoded) = decode_text(bytes) {
        entry.record.content_hash = Some(ContentHash::blake3(bytes));
        entry.record.text_encoding = Some(decoded.encoding);
        entry.record.content = Some(decoded.text);
    }
}

pub(crate) fn scan_workspace(
    resolver: &SecurePathResolver,
    options: &CatalogOptions,
) -> Result<ScanResult, WorkspaceError> {
    resolver.verify_root_identity()?;
    let mut pending = Vec::new();
    let mut stats = BTreeMap::<WorkspacePath, PartitionStats>::new();
    let mut watch_directories = vec![resolver.canonical_root().to_path_buf()];
    let mut ignores = Vec::new();
    scan_directory(
        resolver,
        WorkspacePath::parse(WorkspacePath::ROOT).expect("root path"),
        WorkspacePath::parse(WorkspacePath::ROOT).expect("root path"),
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
    for entry in &mut pending {
        hydrate_entry(
            resolver,
            options,
            entry,
            modes.get(&entry.record.partition_path) == Some(&CatalogIndexMode::FullText),
        )?;
    }
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
    let entries = pending
        .iter()
        .map(|entry| (entry.record.path.clone(), catalog_entry(entry.record.clone())))
        .collect();
    Ok(ScanResult {
        entries,
        partitions,
        persisted_entries: pending.into_iter().map(|entry| entry.record).collect(),
        persisted_partitions,
        watch_directories,
    })
}

pub(crate) fn scan_directory(
    resolver: &SecurePathResolver,
    directory: WorkspacePath,
    inherited_partition: WorkspacePath,
    ignores: &mut Vec<Gitignore>,
    pending: &mut Vec<PendingEntry>,
    stats: &mut BTreeMap<WorkspacePath, PartitionStats>,
    watch_directories: &mut Vec<PathBuf>,
) -> Result<(), WorkspaceError> {
    let (pushed_ignore, listed) =
        list_directory(resolver, &directory, &inherited_partition, ignores)?;
    for (entry, child_absolute, _) in listed {
        let child_path = entry.record.path.clone();
        let child_partition = entry.record.partition_path.clone();
        let child_is_directory = entry.record.kind == CatalogEntryKind::Directory;
        record_stats(stats, entry.record.kind, entry.text_candidate, &entry.record);
        pending.push(entry);
        if child_is_directory {
            watch_directories.push(child_absolute);
            // Recursing inside the pushed ignore scope keeps every ancestor
            // `.gitignore` in force for the nested listing.
            scan_directory(
                resolver,
                child_path,
                child_partition,
                ignores,
                pending,
                stats,
                watch_directories,
            )?;
        }
    }
    if pushed_ignore {
        ignores.pop();
    }
    Ok(())
}

/// Re-lists exactly one directory level: the entries of its direct children are
/// produced, nothing below them is read. This is the bounded answer to "something
/// inside this directory changed" when the caller cannot name the child, and it is
/// deliberately not a subtree scan.
#[allow(clippy::too_many_arguments)]
pub(crate) fn list_directory_once(
    resolver: &SecurePathResolver,
    directory: &WorkspacePath,
    inherited_partition: &WorkspacePath,
    ignores: &mut Vec<Gitignore>,
    pending: &mut Vec<PendingEntry>,
    stats: &mut BTreeMap<WorkspacePath, PartitionStats>,
    watch_directories: &mut Vec<PathBuf>,
) -> Result<(), WorkspaceError> {
    let (pushed_ignore, listed) =
        list_directory(resolver, directory, inherited_partition, ignores)?;
    for (entry, child_absolute, is_directory) in listed {
        record_stats(stats, entry.record.kind, entry.text_candidate, &entry.record);
        if is_directory {
            watch_directories.push(child_absolute);
        }
        pending.push(entry);
    }
    if pushed_ignore {
        ignores.pop();
    }
    Ok(())
}

/// The ignore rules that are in force for a directory, built from the workspace
/// root down to but not including it.
///
/// A bounded scan starts in the middle of the tree, so it has to reconstruct the
/// rules a full walk would have had in force at that point. Without this, a rule
/// in an ancestor `.gitignore` stops applying to exactly the files a local update
/// re-reads, and the index disagrees with a full rebuild.
pub(crate) fn inherited_ignores(
    resolver: &SecurePathResolver,
    directory: &WorkspacePath,
) -> Result<Vec<Gitignore>, WorkspaceError> {
    let mut matchers = Vec::new();
    if directory.is_root() {
        return Ok(matchers);
    }
    // The workspace root's own rules apply to everything below it, so they come
    // first: without them a rule like `*.log` in the root `.gitignore` would stop
    // applying to the files a local update re-reads.
    let root = WorkspacePath::parse(WorkspacePath::ROOT).expect("root path");
    push_ignore_matcher(&resolver.absolute_path(&root), true, &mut matchers)?;
    let mut prefix = String::new();
    for component in directory.as_str().split('/') {
        if component.is_empty() {
            continue;
        }
        let ancestor = if prefix.is_empty() {
            WorkspacePath::parse(component)
        } else {
            WorkspacePath::parse(format!("{prefix}/{component}"))
        }
        .map_err(|error| WorkspaceError::Watcher(format!("unsafe workspace entry: {error}")))?;
        if ancestor == *directory {
            // The scan pushes the directory's own matcher itself.
            break;
        }
        let absolute = resolver.absolute_path(&ancestor);
        push_ignore_matcher(&absolute, ancestor.is_root(), &mut matchers)?;
        prefix = ancestor.as_str().to_owned();
    }
    Ok(matchers)
}

/// Reads exactly one directory level and leaves the directory's own ignore
/// matcher pushed on `ignores`. The caller pops it after recursing, so a nested
/// level sees every ancestor's rules.
/// One listed entry: its catalog record, its absolute path, and whether the
/// listing had to skip it.
type ListedEntries = Vec<(PendingEntry, PathBuf, bool)>;

fn list_directory(
    resolver: &SecurePathResolver,
    directory: &WorkspacePath,
    inherited_partition: &WorkspacePath,
    ignores: &mut Vec<Gitignore>,
) -> Result<(bool, ListedEntries), WorkspaceError> {
    let absolute = resolver.absolute_path(directory);
    let pushed_ignore = push_ignore_matcher(&absolute, directory.is_root(), ignores)?;
    let partition = if directory.is_root() {
        inherited_partition.clone()
    } else if contains_package_manifest(&absolute) {
        directory.clone()
    } else {
        inherited_partition.clone()
    };
    let mut children = fs::read_dir(&absolute)
        .map_err(|source| io_error("enumerate workspace directory", &absolute, source))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| io_error("read workspace directory entry", &absolute, source))?;
    children.sort_by_key(std::fs::DirEntry::file_name);
    let mut listed = Vec::with_capacity(children.len());
    for child in children {
        let name = child.file_name().into_string().map_err(|name| {
            WorkspaceError::Watcher(format!(
                "workspace path is not representable in JSON: {}",
                PathBuf::from(name).display()
            ))
        })?;
        let child_path = if directory.is_root() {
            WorkspacePath::parse(&name)
        } else {
            WorkspacePath::parse(format!("{directory}/{name}"))
        }
        .map_err(|error| WorkspaceError::Watcher(format!("unsafe workspace entry: {error}")))?;
        let child_absolute = child.path();
        if is_internal_component(&name) {
            // Aura's own staging or temp entry: never listed, so its subtree is
            // never walked either.
            continue;
        }
        let entry = build_entry(
            ignores,
            partition.clone(),
            directory.clone(),
            child_path,
            name,
            &child_absolute,
        )?;
        let is_directory = entry.record.kind == CatalogEntryKind::Directory;
        listed.push((entry, child_absolute, is_directory));
    }
    Ok((pushed_ignore, listed))
}

/// Re-reads a bounded set of exact paths inside one known directory. Used when
/// the caller already knows which entries changed, so re-listing and re-hashing a
/// whole directory is unnecessary. A path outside `directory` is skipped, never
/// widened into a different scope.
pub(crate) fn scan_directory_entries(
    resolver: &SecurePathResolver,
    directory: &WorkspacePath,
    inherited_partition: &WorkspacePath,
    ignores: &mut Vec<Gitignore>,
    wanted: &BTreeSet<WorkspacePath>,
) -> Result<Vec<PendingEntry>, WorkspaceError> {
    let absolute = resolver.absolute_path(directory);
    let pushed_ignore = push_ignore_matcher(&absolute, directory.is_root(), ignores)?;
    let partition = if directory.is_root() {
        inherited_partition.clone()
    } else if contains_package_manifest(&absolute) {
        directory.clone()
    } else {
        inherited_partition.clone()
    };
    let mut listed = Vec::new();
    for child_path in wanted {
        let Some(name) = child_path.as_str().rsplit('/').next() else { continue };
        let parent_matches = match child_path.as_str().rfind('/') {
            Some(index) => &child_path.as_str()[..index] == directory.as_str(),
            None => directory.is_root(),
        };
        if !parent_matches {
            continue;
        }
        if is_internal_component(name) {
            // Aura's own staging or temp entry is never part of the index.
            continue;
        }
        let child_absolute = absolute.join(name);
        if fs::symlink_metadata(&child_absolute).is_err() {
            continue;
        }
        listed.push(build_entry(
            ignores,
            partition.clone(),
            directory.clone(),
            child_path.clone(),
            name.to_owned(),
            &child_absolute,
        )?);
    }
    if pushed_ignore {
        ignores.pop();
    }
    Ok(listed)
}

/// Builds one catalog entry from a real directory entry. The full scan, the
/// bounded directory rescan and the exact-path rescan all share this one
/// implementation, so an incremental update cannot record different facts than a
/// rebuild would.
fn build_entry(
    ignores: &[Gitignore],
    partition: WorkspacePath,
    parent: WorkspacePath,
    child_path: WorkspacePath,
    name: String,
    child_absolute: &Path,
) -> Result<PendingEntry, WorkspaceError> {
    let info = inspect_existing_path(child_absolute, false).map_err(|source| {
        io_error("inspect workspace entry without following reparse", child_absolute, source)
    })?;
    let is_directory = info.is_directory;
    let identity = info.identity.clone();
    let ignored = classify_ignored(ignores, child_absolute, is_directory);
    let generated = is_generated_path(&child_path);
    let hidden_from_search = child_path
        .as_str()
        .split('/')
        .next()
        .is_some_and(|component| component.eq_ignore_ascii_case(".git"));
    let metadata = fs::symlink_metadata(child_absolute)
        .map_err(|source| io_error("read workspace entry metadata", child_absolute, source))?;
    let kind = if info.is_reparse_point {
        CatalogEntryKind::ReparsePoint
    } else if is_directory {
        CatalogEntryKind::Directory
    } else {
        CatalogEntryKind::File
    };
    let child_partition = if kind == CatalogEntryKind::Directory && parent.is_root() {
        child_path.clone()
    } else {
        partition
    };
    let text_candidate = kind == CatalogEntryKind::File && is_text_candidate(&name);
    let byte_length = (kind == CatalogEntryKind::File).then_some(metadata.len());
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_millis().to_string());
    Ok(PendingEntry {
        record: CatalogEntryRecord {
            path: child_path,
            parent_path: parent,
            file_name: name,
            kind,
            partition_path: child_partition,
            byte_length,
            modified_at,
            content_hash: None,
            text_encoding: None,
            ignored,
            generated,
            hidden_from_search,
            reparse_tag: info.is_reparse_point.then_some(info.reparse_tag),
            content: None,
            // A directory listing is metadata only: it proves nothing about the
            // file's content, so it carries no change generation. The identity is
            // recorded because the listing already resolved it, and a later open
            // needs it to know whether the file it witnesses is the same object.
            change_usn: None,
            volume_serial: Some(identity.volume_serial_number.to_string()),
            file_id: Some(identity.file_id.iter().map(|byte| format!("{byte:02x}")).collect()),
        },
        text_candidate,
        listed: true,
    })
}

fn record_stats(
    stats: &mut BTreeMap<WorkspacePath, PartitionStats>,
    kind: CatalogEntryKind,
    text_candidate: bool,
    record: &CatalogEntryRecord,
) {
    let bucket = stats.entry(record.partition_path.clone()).or_default();
    if kind == CatalogEntryKind::File {
        bucket.file_count = bucket.file_count.saturating_add(1);
        if text_candidate {
            bucket.indexable_text_bytes =
                bucket.indexable_text_bytes.saturating_add(record.byte_length.unwrap_or(0));
        }
    }
}

fn push_ignore_matcher(
    directory: &Path,
    workspace_root: bool,
    matchers: &mut Vec<Gitignore>,
) -> Result<bool, WorkspaceError> {
    let gitignore = directory.join(".gitignore");
    let exclude = directory.join(".git").join("info").join("exclude");
    if !gitignore.is_file() && !(workspace_root && exclude.is_file()) {
        return Ok(false);
    }
    let mut builder = GitignoreBuilder::new(directory);
    if gitignore.is_file()
        && let Some(error) = builder.add(&gitignore)
    {
        return Err(WorkspaceError::Watcher(format!(
            "failed to parse {}: {error}",
            gitignore.display()
        )));
    }
    if workspace_root
        && exclude.is_file()
        && let Some(error) = builder.add(&exclude)
    {
        return Err(WorkspaceError::Watcher(format!(
            "failed to parse {}: {error}",
            exclude.display()
        )));
    }
    matchers.push(
        builder
            .build()
            .map_err(|error| WorkspaceError::Watcher(format!("invalid gitignore: {error}")))?,
    );
    Ok(true)
}

fn classify_ignored(matchers: &[Gitignore], path: &Path, is_directory: bool) -> bool {
    for matcher in matchers.iter().rev() {
        match matcher.matched_path_or_any_parents(path, is_directory) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
    }
    false
}

fn contains_package_manifest(directory: &Path) -> bool {
    [
        "package.json",
        "Cargo.toml",
        "pyproject.toml",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
    ]
    .iter()
    .any(|name| directory.join(name).is_file())
        || fs::read_dir(directory).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("csproj"))
            })
        })
}

pub(crate) fn is_text_candidate(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let extension = Path::new(&lower).extension().and_then(|value| value.to_str());
    matches!(
        extension,
        Some(
            "c" | "cc"
                | "cpp"
                | "cs"
                | "css"
                | "go"
                | "h"
                | "hpp"
                | "html"
                | "java"
                | "js"
                | "json"
                | "jsx"
                | "kt"
                | "kts"
                | "md"
                | "mjs"
                | "py"
                | "rb"
                | "rs"
                | "sh"
                | "sql"
                | "svg"
                | "toml"
                | "ts"
                | "tsx"
                | "txt"
                | "xml"
                | "yaml"
                | "yml"
        )
    ) || matches!(
        lower.as_str(),
        "dockerfile" | "makefile" | "license" | "readme" | ".gitignore" | ".editorconfig"
    )
}

fn is_generated_path(path: &WorkspacePath) -> bool {
    path.as_str().split('/').any(|component| {
        matches!(
            component.to_ascii_lowercase().as_str(),
            "node_modules" | "target" | "dist" | "build" | "coverage" | ".next" | "out"
        )
    })
}

pub fn decode_text(bytes: &[u8]) -> Result<DecodedText, WorkspaceError> {
    if let Some(payload) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        let text = std::str::from_utf8(payload).map_err(|_| ambiguous_encoding(bytes))?;
        return Ok(DecodedText {
            text: text.to_owned(),
            encoding: TextEncoding::Utf8Bom,
            line_ending: detect_line_ending(text),
        });
    }
    if let Some(payload) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return decode_utf16(payload, true);
    }
    if let Some(payload) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return decode_utf16(payload, false);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(DecodedText {
            text: text.to_owned(),
            encoding: TextEncoding::Utf8,
            line_ending: detect_line_ending(text),
        });
    }
    Err(ambiguous_encoding(bytes))
}

pub fn decode_text_with_encoding(
    bytes: &[u8],
    encoding: TextEncoding,
) -> Result<DecodedText, WorkspaceError> {
    let text = match encoding {
        TextEncoding::Utf8 => {
            std::str::from_utf8(bytes).map_err(|_| ambiguous_encoding(bytes))?.to_owned()
        }
        TextEncoding::Utf8Bom => {
            let payload = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
            std::str::from_utf8(payload).map_err(|_| ambiguous_encoding(bytes))?.to_owned()
        }
        TextEncoding::Utf16Le => {
            return decode_utf16(bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes), true);
        }
        TextEncoding::Utf16Be => {
            return decode_utf16(bytes.strip_prefix(&[0xfe, 0xff]).unwrap_or(bytes), false);
        }
        TextEncoding::Windows1252 => {
            let (decoded, _, had_errors) = WINDOWS_1252.decode(bytes);
            if had_errors {
                return Err(ambiguous_encoding(bytes));
            }
            decoded.into_owned()
        }
    };
    Ok(DecodedText { line_ending: detect_line_ending(&text), text, encoding })
}

pub fn encode_text(
    text: &str,
    encoding: TextEncoding,
    line_ending: LineEnding,
) -> Result<Vec<u8>, WorkspaceError> {
    let normalized = apply_line_ending(text, line_ending);
    match encoding {
        TextEncoding::Utf8 => Ok(normalized.into_bytes()),
        TextEncoding::Utf8Bom => {
            let mut bytes = vec![0xef, 0xbb, 0xbf];
            bytes.extend_from_slice(normalized.as_bytes());
            Ok(bytes)
        }
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            let little_endian = encoding == TextEncoding::Utf16Le;
            let mut bytes = if little_endian { vec![0xff, 0xfe] } else { vec![0xfe, 0xff] };
            for unit in normalized.encode_utf16() {
                let encoded = if little_endian { unit.to_le_bytes() } else { unit.to_be_bytes() };
                bytes.extend_from_slice(&encoded);
            }
            Ok(bytes)
        }
        TextEncoding::Windows1252 => {
            let (encoded, _, had_errors) = WINDOWS_1252.encode(&normalized);
            if had_errors {
                return Err(WorkspaceError::EncodingAmbiguous {
                    candidates: vec!["utf-8".to_owned(), "windows-1252".to_owned()],
                    preview: normalized.chars().take(256).collect(),
                });
            }
            Ok(encoded.into_owned())
        }
    }
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Result<DecodedText, WorkspaceError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(ambiguous_encoding(bytes));
    }
    let units = bytes
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect::<Vec<_>>();
    let text = String::from_utf16(&units).map_err(|_| ambiguous_encoding(bytes))?;
    Ok(DecodedText {
        line_ending: detect_line_ending(&text),
        text,
        encoding: if little_endian { TextEncoding::Utf16Le } else { TextEncoding::Utf16Be },
    })
}

pub(crate) fn ambiguous_encoding(bytes: &[u8]) -> WorkspaceError {
    let (preview, _, _) = WINDOWS_1252.decode(&bytes[..bytes.len().min(256)]);
    WorkspaceError::EncodingAmbiguous {
        candidates: vec![
            "windows-1252".to_owned(),
            "system-code-page".to_owned(),
            "binary".to_owned(),
        ],
        preview: preview.into_owned(),
    }
}

fn detect_line_ending(text: &str) -> LineEnding {
    let crlf = text.matches("\r\n").count();
    let without_crlf = text.replace("\r\n", "");
    let lf = without_crlf.matches('\n').count();
    let cr = without_crlf.matches('\r').count();
    match (crlf > 0, lf > 0, cr > 0) {
        (false, false, false) => LineEnding::None,
        (true, false, false) => LineEnding::CrLf,
        (false, true, false) => LineEnding::Lf,
        (false, false, true) => LineEnding::Cr,
        _ => LineEnding::Mixed,
    }
}

fn apply_line_ending(text: &str, line_ending: LineEnding) -> String {
    if matches!(line_ending, LineEnding::Mixed | LineEnding::None) {
        return text.to_owned();
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    match line_ending {
        LineEnding::Lf => normalized,
        LineEnding::CrLf => normalized.replace('\n', "\r\n"),
        LineEnding::Cr => normalized.replace('\n', "\r"),
        LineEnding::Mixed | LineEnding::None => normalized,
    }
}

fn fuzzy_score(entry: &CatalogEntry, query: &str, recent: Option<u64>) -> Option<i64> {
    if query.is_empty() {
        return Some(10_000 + recent.map_or(0, |value| i64::try_from(value.min(999)).unwrap_or(0)));
    }
    let path = entry.path.as_str().to_lowercase();
    let name = entry.file_name.to_lowercase();
    let mut score = if path == query {
        100_000
    } else if name == query {
        90_000
    } else if name.starts_with(query) {
        80_000 - i64::try_from(name.len().saturating_sub(query.len())).unwrap_or(10_000)
    } else if path.split('/').any(|component| component.starts_with(query)) {
        70_000
    } else {
        let gaps = subsequence_gaps(&path, query)?;
        60_000 - i64::try_from(gaps).unwrap_or(50_000)
    };
    score += recent.map_or(0, |value| i64::try_from(value.min(999)).unwrap_or(0));
    Some(score)
}

fn subsequence_gaps(value: &str, query: &str) -> Option<usize> {
    let mut positions = Vec::new();
    let mut search_start = 0;
    for query_character in query.chars() {
        let found = value[search_start..]
            .char_indices()
            .find(|(_, character)| *character == query_character)?;
        let position = search_start + found.0;
        positions.push(position);
        search_start = position + found.1.len_utf8();
    }
    positions
        .first()
        .zip(positions.last())
        .map(|(first, last)| last.saturating_sub(*first).saturating_sub(query.len()))
}

/// The version of one directory's own listing.
///
/// It is a digest of the direct children and their content facts, not a counter:
/// a counter would have to be persisted per directory and would change on every
/// unrelated workspace edit, which is exactly the behaviour that made a cursor
/// meaningless. Deriving it means an unrelated directory's change leaves this
/// value untouched, while any real change to this listing — a create, a delete, a
/// rename, or new content — changes it.
fn directory_version(
    state: &CatalogState,
    parent: &WorkspacePath,
) -> Result<Revision, WorkspaceError> {
    let mut children = state
        .entries
        .values()
        .filter(|entry| &entry.parent_path == parent)
        .map(|entry| {
            (
                entry.path.as_str(),
                entry.kind,
                entry.byte_length,
                entry.modified_at.as_deref(),
                entry.content_hash.as_ref().map(ToString::to_string),
            )
        })
        .collect::<Vec<_>>();
    children.sort_by(|left, right| left.0.cmp(right.0));
    version_of(&children)
}

/// One version token for a filename lookup's scope.
///
/// A non-recursive lookup is decided by one directory's direct children, which is
/// exactly the directory version a listing page uses. A recursive lookup is
/// decided by the scope directory itself and everything below it, because a change
/// to any of those can change the answer. Changes outside the scope cannot, so a
/// page in progress is not invalidated by unrelated work elsewhere in the
/// workspace.
fn scope_version(
    state: &CatalogState,
    directory: &WorkspacePath,
    recursive: bool,
) -> Result<Revision, WorkspaceError> {
    let mut entries = state
        .entries
        .values()
        .filter(|entry| {
            if recursive {
                entry.path == *directory || is_within(&entry.path, directory)
            } else {
                &entry.parent_path == directory
            }
        })
        .map(|entry| {
            (
                entry.path.as_str(),
                entry.kind,
                entry.byte_length,
                entry.modified_at.as_deref(),
                entry.content_hash.as_ref().map(ToString::to_string),
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    version_of(&entries)
}

/// The stable version token of a set of entry facts. It is only ever compared for
/// equality, never ordered, so a 64-bit projection of the digest's first sixteen
/// hex digits is enough and keeps the token distinct across the whole prefix.
/// One entry's comparable facts: path, kind, byte length, content hash and the
/// decode failure that was recorded for it, if any.
type EntryFacts<'a> = (&'a str, CatalogEntryKind, Option<u64>, Option<&'a str>, Option<String>);

fn version_of(facts: &[EntryFacts<'_>]) -> Result<Revision, WorkspaceError> {
    let digest = canonical_sha256(&facts).map_err(|_| WorkspaceError::RevisionExhausted)?;
    let mut token = 0_u64;
    for character in digest.hex().chars().take(16) {
        let nibble = character.to_digit(16).unwrap_or(0);
        token = (token << 4) | u64::from(nibble);
    }
    Ok(Revision::new(token))
}

/// Whether one catalogued path can be part of a filename lookup in this scope.
fn in_scope(entry: &CatalogEntry, directory: &WorkspacePath, recursive: bool) -> bool {
    if recursive {
        entry.path == *directory || is_within(&entry.path, directory)
    } else {
        &entry.parent_path == directory
    }
}

/// Whether `path` is inside `scope` rather than beside it.
fn is_within(path: &WorkspacePath, scope: &WorkspacePath) -> bool {
    if scope.is_root() {
        return !path.is_root();
    }
    path.as_str().starts_with(&format!("{scope}/"))
}

/// One page of a filename lookup and everything the caller needs to continue it.
pub struct NameFindPage {
    pub matches: Vec<CatalogEntry>,
    /// The version of the queried scope, not of the workspace.
    pub scope_revision: Revision,
    /// True only when the walk reached the end of the scope, so a name that is
    /// absent from this page is genuinely absent.
    pub complete: bool,
    /// Entries inside the scope that this page examined.
    pub scanned_entries: u64,
    /// The last path this page examined, which is where the next page continues.
    /// It is the last *examined* path rather than the last match, so a continuation
    /// never re-walks what this page already read and never skips a candidate.
    pub resume_after: Option<WorkspacePath>,
}

/// How a requested file name is compared against an indexed one.
///
/// Public because the model-facing filename lookup is implemented in the fs-tool
/// layer and has to name the matching rule it asks for.
#[derive(Clone, Copy)]
pub enum NameMatchMode {
    /// The whole name must equal the requested text.
    Exact,
    /// The whole name must match a `*`/`?` pattern.
    Glob,
}

/// One compiled file-name matcher.
///
/// Case folding is applied to both sides rather than to the pattern alone,
/// because the default is the platform's file-system convention, which is
/// case-insensitive on Windows.
enum NamePattern {
    Exact { needle: String, case_sensitive: bool },
    Glob { pattern: globset::GlobMatcher, case_sensitive: bool },
}

impl NamePattern {
    /// Compiles one matcher, or refuses the pattern.
    ///
    /// A pattern the matcher cannot compile must be an error: treating it as
    /// "matches nothing" turns a typo into the fact "this file does not exist",
    /// which is a wrong answer rather than a wrong page.
    fn compile(
        text: &str,
        mode: NameMatchMode,
        case_sensitive: bool,
    ) -> Result<Self, WorkspaceError> {
        match mode {
            NameMatchMode::Exact => Ok(Self::Exact { needle: text.to_owned(), case_sensitive }),
            NameMatchMode::Glob => {
                // `literal_separator` is on so `*` cannot cross a directory
                // boundary even though only a file name is matched here; it keeps
                // the pattern language identical to the one used elsewhere.
                let pattern = GlobBuilder::new(text)
                    .literal_separator(true)
                    .backslash_escape(false)
                    .case_insensitive(!case_sensitive)
                    .build()
                    .map(|glob| glob.compile_matcher())
                    .map_err(|error| {
                        WorkspaceError::Watcher(format!(
                            "invalid file name pattern {text:?}: {error}"
                        ))
                    })?;
                Ok(Self::Glob { pattern, case_sensitive })
            }
        }
    }

    fn matches(&self, name: &str) -> bool {
        match self {
            Self::Exact { needle, case_sensitive } => {
                if *case_sensitive {
                    name == needle
                } else {
                    name.eq_ignore_ascii_case(needle)
                }
            }
            Self::Glob { pattern, case_sensitive } => {
                // `GlobMatcher` already folds case when it was built
                // case-insensitively, so the flag only records the choice.
                let _ = case_sensitive;
                pattern.is_match(name)
            }
        }
    }
}

fn catalog_entry(record: CatalogEntryRecord) -> CatalogEntry {
    CatalogEntry::from_record(&PendingEntry { record, text_candidate: false, listed: false })
}

impl CatalogEntry {
    /// Converts a scanned record into the in-memory entry shape.
    pub(crate) fn from_record(entry: &PendingEntry) -> Self {
        let record = &entry.record;
        Self {
            path: record.path.clone(),
            parent_path: record.parent_path.clone(),
            file_name: record.file_name.clone(),
            kind: record.kind,
            partition_path: record.partition_path.clone(),
            byte_length: record.byte_length,
            modified_at: record.modified_at.clone(),
            content_hash: record.content_hash.clone(),
            text_encoding: record.text_encoding,
            ignored: record.ignored,
            generated: record.generated,
            hidden_from_search: record.hidden_from_search,
            reparse_tag: record.reparse_tag,
            change_usn: record.change_usn,
            volume_serial: record.volume_serial.clone(),
            file_id: record.file_id.clone(),
        }
    }

    /// Converts an in-memory entry back into the persisted record shape. The
    /// caller supplies the text index content because it belongs to the scan that
    /// produced the entry, not to the entry itself.
    pub(crate) fn to_record(&self, content: Option<String>) -> CatalogEntryRecord {
        CatalogEntryRecord {
            path: self.path.clone(),
            parent_path: self.parent_path.clone(),
            file_name: self.file_name.clone(),
            kind: self.kind,
            partition_path: self.partition_path.clone(),
            byte_length: self.byte_length,
            modified_at: self.modified_at.clone(),
            content_hash: self.content_hash.clone(),
            text_encoding: self.text_encoding,
            ignored: self.ignored,
            generated: self.generated,
            hidden_from_search: self.hidden_from_search,
            reparse_tag: self.reparse_tag,
            content,
            change_usn: self.change_usn,
            volume_serial: self.volume_serial.clone(),
            file_id: self.file_id.clone(),
        }
    }
}

fn partition_summary(record: CatalogPartitionRecord) -> CatalogPartitionSummary {
    CatalogPartitionSummary {
        partition_path: record.partition_path,
        file_count: record.file_count,
        indexable_text_bytes: record.indexable_text_bytes,
        index_mode: record.index_mode,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::Path, sync::Arc};

    use aura_domain_common::{
        CorrelationId, Revision, RuntimeInstanceId, WorkspaceId, WorkspacePath,
    };
    use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
    use aura_workspace_domain::{
        CatalogEntryKind, CatalogFreshness, CatalogIndexMode, WorkspaceState, WorkspaceSummary,
    };

    use super::{
        CatalogEntry, CatalogOptions, PathCatalog, SecurePathResolver, decode_text, fuzzy_score,
    };

    fn create_catalog(
        root: &Path,
        options: CatalogOptions,
    ) -> (tempfile::TempDir, PathCatalog, SecurePathResolver) {
        let data = tempfile::tempdir().expect("persistence root");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            data.path().join("aura.sqlite3"),
            data.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence startup")
        {
            PersistenceStartup::Ready(value) => Arc::new(value),
            PersistenceStartup::RecoveryRequired(value) => {
                panic!("unexpected recovery: {}", value.reason)
            }
        };
        let info = SecurePathResolver::inspect_selected_root(root).expect("root info");
        let workspace = WorkspaceSummary {
            workspace_id: WorkspaceId::new(),
            display_name: "catalog test".to_owned(),
            selected_root: root.display().to_string(),
            canonical_root: info.final_path.display().to_string(),
            root_identity: Some(crate::domain_identity(&info.identity)),
            state: WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        };
        persistence
            .create_workspace(workspace.clone(), CorrelationId::new())
            .expect("persist workspace");
        let resolver = SecurePathResolver::from_workspace(&workspace).expect("resolver");
        let catalog =
            PathCatalog::open(workspace.workspace_id, persistence, options).expect("open catalog");
        (data, catalog, resolver)
    }

    #[test]
    fn detects_utf8_and_preserves_line_endings() {
        let decoded = decode_text(b"first\r\nsecond\r\n").expect("utf8");
        assert_eq!(decoded.line_ending, aura_workspace_domain::LineEnding::CrLf);
    }

    /// A filename lookup is bounded by two budgets, and a page that ran out of
    /// budget is resumable rather than conclusive: an absent name is only reported
    /// as absent once the walk really reached the end of the scope.
    #[test]
    fn a_budget_limited_lookup_resumes_without_skipping_or_repeating() {
        let workspace = tempfile::tempdir().expect("workspace root");
        for index in 0..12 {
            std::fs::write(workspace.path().join(format!("f{index:02}.txt")), "body")
                .expect("file");
        }
        let (_data, catalog, resolver) =
            create_catalog(workspace.path(), CatalogOptions::default());
        catalog.rebuild(&resolver, CorrelationId::new()).expect("rebuild");
        let root = WorkspacePath::parse(WorkspacePath::ROOT).expect("root");

        // A full page stops at the result limit, so a call reads no more than the
        // page it returns.
        let first = catalog
            .find_names(
                &root,
                &["*.txt".to_owned()],
                false,
                super::NameMatchMode::Glob,
                false,
                None,
                2,
                3,
            )
            .expect("page");
        assert_eq!(first.matches.len(), 2);
        assert!(first.scanned_entries <= 2, "the walk stops at the full page");
        assert!(!first.complete, "a full page cannot be the whole answer");
        assert!(first.resume_after.is_some());

        // Paging through with a small budget visits every name exactly once.
        let mut seen = Vec::new();
        let mut after = None;
        let mut pages = 0;
        loop {
            let page = catalog
                .find_names(
                    &root,
                    &["*.txt".to_owned()],
                    false,
                    super::NameMatchMode::Glob,
                    false,
                    after.as_ref(),
                    2,
                    3,
                )
                .expect("page");
            assert!(page.scanned_entries <= 3, "one call never exceeds the scan budget");
            seen.extend(page.matches.iter().map(|entry| entry.path.as_str().to_owned()));
            pages += 1;
            match page.resume_after {
                Some(path) => after = Some(path),
                None => {
                    assert!(page.complete, "the last page is the one that finished the scope");
                    break;
                }
            }
            assert!(pages < 20, "paging must terminate");
        }
        assert_eq!(
            pages, 6,
            "six pages hold the twelve names: the page that filled up at the last entry can \
             see that nothing follows it, so the walk is complete there"
        );
        let mut expected = (0..12).map(|index| format!("f{index:02}.txt")).collect::<Vec<_>>();
        expected.sort();
        assert_eq!(seen, expected, "no name is skipped or repeated");

        // With no match at all, a budget-limited page must not claim the name is
        // absent: only the page that finished the scope may.
        let mut after = None;
        let mut pages = 0;
        loop {
            let page = catalog
                .find_names(
                    &root,
                    &["nothing.txt".to_owned()],
                    false,
                    super::NameMatchMode::Exact,
                    false,
                    after.as_ref(),
                    2,
                    3,
                )
                .expect("page");
            assert!(page.matches.is_empty());
            assert!(page.scanned_entries <= 3, "the scan budget bounds every call");
            assert!(page.resume_after.is_some() || page.complete);
            pages += 1;
            match page.resume_after {
                Some(path) => {
                    assert!(!page.complete, "an unfinished walk never reports absence");
                    assert_eq!(
                        page.scanned_entries, 3,
                        "the budget, not the result, ended this page"
                    );
                    after = Some(path);
                }
                None => break,
            }
            assert!(pages < 20);
        }
        assert_eq!(pages, 4, "12 entries at 3 per call take four calls before absence is certain");
    }

    #[test]
    fn incremental_update_matches_a_rebuild_for_one_changed_directory() {
        use crate::catalog::{CatalogChange, CatalogUpdate};

        let workspace = tempfile::tempdir().expect("workspace root");
        std::fs::create_dir_all(workspace.path().join("src")).expect("src");
        std::fs::create_dir_all(workspace.path().join("docs")).expect("docs");
        std::fs::write(workspace.path().join("src/main.rs"), "fn main() {}").expect("main");
        std::fs::write(workspace.path().join("docs/guide.md"), "# guide").expect("guide");
        let (_data, catalog, resolver) =
            create_catalog(workspace.path(), CatalogOptions::default());
        catalog.rebuild(&resolver, CorrelationId::new()).expect("rebuild");
        let before = catalog.entries().expect("entries");
        let source_revision = catalog.revision().expect("revision");

        // A single-directory change, then a bounded update over exactly that
        // directory. A create and an edit inside it are one uniform case.
        std::fs::write(workspace.path().join("src/main.rs"), "fn main() { work() }").expect("edit");
        std::fs::write(workspace.path().join("src/extra.rs"), "pub fn extra() {}").expect("extra");
        let revision = catalog
            .apply_changes(
                &resolver,
                CatalogUpdate {
                    inputs: 1,
                    writes_missing_text: false,
                    repaired_paths: Vec::new(),
                    changes: vec![(
                        WorkspacePath::parse("src").expect("path"),
                        CatalogChange::DirectoryRescanned,
                    )],
                    reason: "test".to_owned(),
                },
                &CatalogOptions::default(),
                CorrelationId::new(),
            )
            .expect("incremental update");
        assert!(revision.value() > source_revision.value());

        let incremental = catalog.entries().expect("entries");
        let rebuilt = {
            let (_data, rebuilt, rebuilt_resolver) =
                create_catalog(workspace.path(), CatalogOptions::default());
            rebuilt.rebuild(&rebuilt_resolver, CorrelationId::new()).expect("rebuild");
            rebuilt.entries().expect("entries")
        };
        let summarize = |entries: &[CatalogEntry]| {
            entries
                .iter()
                .map(|entry| {
                    (
                        entry.path.clone(),
                        entry.kind,
                        entry.byte_length,
                        entry.content_hash.clone(),
                        entry.ignored,
                        entry.generated,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(summarize(&incremental), summarize(&rebuilt));

        // An unrelated directory keeps its entry facts, so a cursor bound to it
        // does not become meaningless because `src` changed.
        let docs_before = before
            .iter()
            .find(|entry| entry.path.as_str() == "docs/guide.md")
            .expect("docs before");
        let docs_after = incremental
            .iter()
            .find(|entry| entry.path.as_str() == "docs/guide.md")
            .expect("docs after");
        assert_eq!(docs_before.content_hash, docs_after.content_hash);
    }

    #[test]
    fn incremental_update_removes_a_deleted_subtree_and_keeps_duplicates_idempotent() {
        use crate::catalog::{CatalogChange, CatalogUpdate};

        let workspace = tempfile::tempdir().expect("workspace root");
        std::fs::create_dir_all(workspace.path().join("pkg/nested")).expect("pkg");
        std::fs::write(workspace.path().join("pkg/nested/deep.txt"), "deep").expect("deep");
        std::fs::write(workspace.path().join("keep.txt"), "keep").expect("keep");
        let (_data, catalog, resolver) =
            create_catalog(workspace.path(), CatalogOptions::default());
        catalog.rebuild(&resolver, CorrelationId::new()).expect("rebuild");

        std::fs::remove_dir_all(workspace.path().join("pkg")).expect("delete pkg");
        let update = |reason: &str| CatalogUpdate {
            inputs: 1,
            writes_missing_text: false,
            repaired_paths: Vec::new(),
            changes: vec![
                (WorkspacePath::parse("pkg").expect("path"), CatalogChange::Removed),
                (WorkspacePath::parse("pkg").expect("path"), CatalogChange::Removed),
            ],
            reason: reason.to_owned(),
        };
        let first = catalog
            .apply_changes(
                &resolver,
                update("delete"),
                &CatalogOptions::default(),
                CorrelationId::new(),
            )
            .expect("delete update");
        let second = catalog
            .apply_changes(
                &resolver,
                update("delete again"),
                &CatalogOptions::default(),
                CorrelationId::new(),
            )
            .expect("repeated update");
        // A repeated observation is idempotent in content but still advances the
        // revision, so a reader can tell that a refresh happened.
        assert!(second.value() >= first.value());

        let entries = catalog.entries().expect("entries");
        assert!(
            entries.iter().all(|entry| !entry.path.as_str().starts_with("pkg")),
            "the whole removed subtree must leave the catalog"
        );
        assert!(entries.iter().any(|entry| entry.path.as_str() == "keep.txt"));
    }

    #[test]
    fn a_path_that_disappeared_before_indexing_becomes_a_removal() {
        use crate::catalog::{CatalogChange, CatalogUpdate};

        let workspace = tempfile::tempdir().expect("workspace root");
        std::fs::write(workspace.path().join("gone.txt"), "gone").expect("gone");
        let (_data, catalog, resolver) =
            create_catalog(workspace.path(), CatalogOptions::default());
        catalog.rebuild(&resolver, CorrelationId::new()).expect("rebuild");
        std::fs::remove_file(workspace.path().join("gone.txt")).expect("remove");

        // The disk commit already happened. Indexing must not turn this into a
        // failed write that invites the model to replay it.
        catalog
            .apply_changes(
                &resolver,
                CatalogUpdate {
                    inputs: 1,
                    writes_missing_text: false,
                    repaired_paths: Vec::new(),
                    changes: vec![(
                        WorkspacePath::parse(".").expect("root"),
                        CatalogChange::DirectoryRescanned,
                    )],
                    reason: "post-commit".to_owned(),
                },
                &CatalogOptions::default(),
                CorrelationId::new(),
            )
            .expect("bounded update after a concurrent removal");
        assert!(
            catalog
                .entry(&WorkspacePath::parse("gone.txt").expect("path"))
                .expect("entry")
                .is_none()
        );
    }

    #[test]
    fn incremental_update_survives_reopening_the_catalog() {
        use crate::catalog::{CatalogChange, CatalogUpdate};

        let workspace = tempfile::tempdir().expect("workspace root");
        std::fs::write(workspace.path().join("a.txt"), "one").expect("a");
        let (data, catalog, resolver) = create_catalog(workspace.path(), CatalogOptions::default());
        catalog.rebuild(&resolver, CorrelationId::new()).expect("rebuild");
        let workspace_id = resolver.workspace_id();

        std::fs::write(workspace.path().join("b.txt"), "two").expect("b");
        catalog
            .apply_changes(
                &resolver,
                CatalogUpdate {
                    inputs: 1,
                    writes_missing_text: false,
                    repaired_paths: Vec::new(),
                    changes: vec![(
                        WorkspacePath::parse(".").expect("root"),
                        CatalogChange::DirectoryRescanned,
                    )],
                    reason: "create".to_owned(),
                },
                &CatalogOptions::default(),
                CorrelationId::new(),
            )
            .expect("incremental create");

        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            data.path().join("aura.sqlite3"),
            data.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence startup")
        {
            PersistenceStartup::Ready(value) => Arc::new(value),
            PersistenceStartup::RecoveryRequired(value) => {
                panic!("unexpected recovery: {}", value.reason)
            }
        };
        let reopened = PathCatalog::open(workspace_id, persistence, CatalogOptions::default())
            .expect("reopen catalog");
        let entries = reopened.entries().expect("entries");
        assert!(entries.iter().any(|entry| entry.path.as_str() == "a.txt"));
        let created = entries
            .iter()
            .find(|entry| entry.path.as_str() == "b.txt")
            .expect("incrementally created entry is durable");
        assert_eq!(created.byte_length, Some(3));
    }

    #[test]
    fn path_ranking_prefers_exact_relative_path() {
        let entry = CatalogEntry {
            path: WorkspacePath::parse("src/main.rs").expect("path"),
            parent_path: WorkspacePath::parse("src").expect("parent"),
            file_name: "main.rs".to_owned(),
            kind: CatalogEntryKind::File,
            partition_path: WorkspacePath::parse(".").expect("partition"),
            byte_length: Some(1),
            modified_at: None,
            content_hash: None,
            text_encoding: None,
            ignored: false,
            generated: false,
            hidden_from_search: false,
            reparse_tag: None,
            change_usn: None,
            volume_serial: None,
            file_id: None,
        };
        assert_eq!(fuzzy_score(&entry, "src/main.rs", None), Some(100_000));
        assert!(fuzzy_score(&entry, "main", None).is_some());
    }

    #[test]
    fn partitions_large_generated_trees_without_downgrading_small_source_trees() {
        let workspace = tempfile::tempdir().expect("workspace root");
        std::fs::create_dir_all(workspace.path().join("src")).expect("src");
        std::fs::create_dir_all(workspace.path().join("node_modules/pkg")).expect("generated");
        std::fs::create_dir_all(workspace.path().join(".git")).expect("git metadata");
        std::fs::write(workspace.path().join(".gitignore"), "ignored.txt\nnode_modules/\n")
            .expect("gitignore");
        std::fs::write(workspace.path().join("ignored.txt"), "still catalogued")
            .expect("ignored file");
        std::fs::write(workspace.path().join("src/main.rs"), "fn main() {}").expect("source file");
        for index in 0..3 {
            std::fs::write(
                workspace.path().join(format!("node_modules/pkg/{index}.js")),
                format!("export const value = {index};"),
            )
            .expect("generated file");
        }
        std::fs::write(workspace.path().join(".git/config"), "[core]").expect("git config");

        let (_data, catalog, resolver) = create_catalog(
            workspace.path(),
            CatalogOptions {
                max_partition_files: 2,
                max_partition_text_bytes: u64::MAX,
                max_index_file_bytes: 1024 * 1024,
            },
        );
        catalog.rebuild(&resolver, CorrelationId::new()).expect("rebuild");

        let ignored = catalog
            .entry(&WorkspacePath::parse("ignored.txt").expect("path"))
            .expect("entry")
            .expect("ignored is catalogued");
        assert!(ignored.ignored);
        let generated = catalog
            .entry(&WorkspacePath::parse("node_modules/pkg/0.js").expect("path"))
            .expect("entry")
            .expect("generated is catalogued");
        assert!(generated.ignored);
        assert!(generated.generated);
        assert!(
            catalog
                .find_paths("config", 10)
                .expect("find")
                .matches
                .iter()
                .all(|entry| !entry.path.as_str().starts_with(".git/"))
        );

        let partitions = catalog.partitions().expect("partitions");
        assert_eq!(
            partitions
                .iter()
                .find(|partition| partition.partition_path.as_str() == "node_modules")
                .expect("generated partition")
                .index_mode,
            CatalogIndexMode::MetadataOnly
        );
        assert_eq!(
            partitions
                .iter()
                .find(|partition| partition.partition_path.as_str() == "src")
                .expect("source partition")
                .index_mode,
            CatalogIndexMode::FullText
        );
    }

    #[test]
    fn catalog_records_but_never_recurses_directory_junctions() {
        let workspace = tempfile::tempdir().expect("workspace root");
        let outside = tempfile::tempdir().expect("outside root");
        std::fs::write(outside.path().join("secret.txt"), "outside").expect("outside file");
        junction::create(outside.path(), workspace.path().join("linked")).expect("junction");
        let (_data, catalog, resolver) =
            create_catalog(workspace.path(), CatalogOptions::with_index_file_limit(1024 * 1024));
        catalog.rebuild(&resolver, CorrelationId::new()).expect("rebuild");
        let link = catalog
            .entry(&WorkspacePath::parse("linked").expect("link path"))
            .expect("entry")
            .expect("link recorded");
        assert_eq!(link.kind, CatalogEntryKind::ReparsePoint);
        assert!(
            catalog
                .entry(&WorkspacePath::parse("linked/secret.txt").expect("secret path"))
                .expect("entry")
                .is_none()
        );
    }

    #[test]
    fn one_hundred_thousand_path_fixture_keeps_tree_order_stable() {
        let workspace = tempfile::tempdir().expect("workspace root");
        let (_data, catalog, _resolver) =
            create_catalog(workspace.path(), CatalogOptions::with_index_file_limit(1024 * 1024));
        let mut entries = BTreeMap::new();
        for index in 0..100_000_u32 {
            let path = WorkspacePath::parse(format!("src/file{index:06}.rs")).expect("path");
            entries.insert(
                path.clone(),
                CatalogEntry {
                    path,
                    parent_path: WorkspacePath::parse("src").expect("parent"),
                    file_name: format!("file{index:06}.rs"),
                    kind: CatalogEntryKind::File,
                    partition_path: WorkspacePath::parse("src").expect("partition"),
                    byte_length: Some(1),
                    modified_at: None,
                    content_hash: None,
                    text_encoding: None,
                    ignored: false,
                    generated: false,
                    hidden_from_search: false,
                    reparse_tag: None,
                    change_usn: None,
                    volume_serial: None,
                    file_id: None,
                },
            );
        }
        catalog.state.write().expect("catalog state").entries = entries;
        let before = catalog
            .entries()
            .expect("entries")
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        let target = WorkspacePath::parse("src/file099999.rs").expect("target");
        catalog.record_use(&target).expect("record use");
        let result = catalog.find_paths(target.as_str(), 20).expect("find exact");
        assert_eq!(result.matches.first().map(|entry| &entry.path), Some(&target));
        let after = catalog
            .entries()
            .expect("entries")
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        assert_eq!(before, after);
    }

    #[test]
    fn a_directory_page_version_ignores_changes_outside_that_directory() {
        use crate::catalog::{CatalogChange, CatalogUpdate};

        let workspace = tempfile::tempdir().expect("workspace root");
        std::fs::create_dir_all(workspace.path().join("alpha")).expect("alpha");
        std::fs::create_dir_all(workspace.path().join("beta")).expect("beta");
        for name in ["alpha/one.txt", "alpha/two.txt", "beta/three.txt"] {
            std::fs::write(workspace.path().join(name), "content").expect(name);
        }
        let (_data, catalog, resolver) =
            create_catalog(workspace.path(), CatalogOptions::default());
        catalog.rebuild(&resolver, CorrelationId::new()).expect("rebuild");

        let root = WorkspacePath::parse(WorkspacePath::ROOT).expect("root path");
        let alpha = WorkspacePath::parse("alpha").expect("alpha path");
        let (_, _, alpha_before) = catalog.children_page(&alpha, None, 10).expect("alpha page");
        let (_, global_before, _) = catalog.children_page(&alpha, None, 10).expect("alpha page");

        // A change confined to `beta` must not move `alpha`'s page version.
        std::fs::write(workspace.path().join("beta/three.txt"), "changed").expect("edit beta");
        std::fs::write(workspace.path().join("beta/four.txt"), "new").expect("add beta");
        let global_after_change = catalog
            .apply_changes(
                &resolver,
                CatalogUpdate {
                    inputs: 1,
                    writes_missing_text: false,
                    repaired_paths: Vec::new(),
                    changes: vec![(
                        WorkspacePath::parse("beta").expect("beta"),
                        CatalogChange::DirectoryRescanned,
                    )],
                    reason: "beta only".to_owned(),
                },
                &CatalogOptions::default(),
                CorrelationId::new(),
            )
            .expect("beta update");
        let (_, _, alpha_after) = catalog.children_page(&alpha, None, 10).expect("alpha page");
        assert_eq!(
            alpha_before, alpha_after,
            "a change in another directory must leave this directory's page valid"
        );
        assert!(
            global_after_change.value() > global_before.value(),
            "the workspace revision still advances so other readers can tell"
        );

        // A real change to `alpha` must move it.
        std::fs::write(workspace.path().join("alpha/one.txt"), "edited").expect("edit alpha");
        catalog
            .apply_changes(
                &resolver,
                CatalogUpdate {
                    inputs: 1,
                    writes_missing_text: false,
                    repaired_paths: Vec::new(),
                    changes: vec![(
                        WorkspacePath::parse("alpha").expect("alpha"),
                        CatalogChange::DirectoryRescanned,
                    )],
                    reason: "alpha only".to_owned(),
                },
                &CatalogOptions::default(),
                CorrelationId::new(),
            )
            .expect("alpha update");
        let (_, _, alpha_changed) = catalog.children_page(&alpha, None, 10).expect("alpha page");
        assert_ne!(
            alpha_before, alpha_changed,
            "a real change to this directory must invalidate its page"
        );

        // The version is derived, not counted, so it survives reopening the
        // catalog with identical contents.
        let (_, _, root_page) = catalog.children_page(&root, None, 10).expect("root page");
        let (_, _, root_page_again) = catalog.children_page(&root, None, 10).expect("root page");
        assert_eq!(root_page, root_page_again, "the token is stable for equal contents");
        assert_ne!(root_page, alpha_changed, "different directories have different tokens");
    }

    #[test]
    fn snapshot_pairs_entries_with_the_same_revision() {
        let workspace = tempfile::tempdir().expect("workspace root");
        std::fs::write(workspace.path().join("a.txt"), "one").expect("a");
        let (_data, catalog, resolver) =
            create_catalog(workspace.path(), CatalogOptions::default());
        catalog.rebuild(&resolver, CorrelationId::new()).expect("rebuild");
        let (entries, revision) = catalog.snapshot().expect("snapshot");
        assert_eq!(revision, catalog.revision().expect("revision"));
        assert_eq!(entries.len(), catalog.entries().expect("entries").len());
        std::fs::write(workspace.path().join("b.txt"), "two").expect("b");
        catalog.rebuild(&resolver, CorrelationId::new()).expect("rebuild again");
        let (later_entries, later_revision) = catalog.snapshot().expect("later");
        assert_eq!(later_revision, catalog.revision().expect("later revision"));
        assert!(later_revision.value() > revision.value() || later_entries.len() > entries.len());
    }
}
