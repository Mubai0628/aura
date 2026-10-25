mod saving;
mod watching;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use watching::WorkspaceWatchHandle;

use aura_domain_common::{CorrelationId, Revision, SessionId, WorkspaceId, WorkspacePath};
use aura_persistence::PersistenceCoordinator;
use aura_windows_platform::{
    FileWatchBackend, FileWatchEventKind, VolumeCaseRule, WorkspaceFileWatcher, volume_case_rule,
};
use aura_workspace_domain::{
    BufferEditCommand, BufferEditResult, BufferOpenCommand, BufferSaveCommand, CatalogFreshness,
    EligibilityOwner, LogicalFileStatus, SlotCaseRule, WorkspaceCatalogStatus,
    WorkspaceCreateCommand, WorkspaceCreateDisposition, WorkspaceCreateResult, WorkspaceFileStatus,
    WorkspaceFilesProjection, WorkspaceState, WorkspaceSummary, WorkspaceWatcherState,
};

use crate::path::domain_identity;
use crate::resource_facts::ResourceFacts;
use crate::{BufferStore, CatalogOptions, PathCatalog, SecurePathResolver, WorkspaceError};

/// The volume's case rule in the form the resource comparison uses.
///
/// The mapping is total and explicit so a new platform answer is a compile error rather than a
/// silent fallback to the case-insensitive rule — which would be this module asserting a rule the
/// volume never stated.
fn volume_case_rule_domain() -> SlotCaseRule {
    match volume_case_rule(&std::env::temp_dir()) {
        VolumeCaseRule::InsensitivePreserving => SlotCaseRule::InsensitivePreserving,
        VolumeCaseRule::Sensitive => SlotCaseRule::Sensitive,
        VolumeCaseRule::Unknown => SlotCaseRule::Unknown,
    }
}

mod lifecycle;

#[derive(Clone, Debug)]
pub struct WorkspaceRuntimeOptions {
    pub catalog: CatalogOptions,
    pub lifecycle_prepare_ack_timeout: Duration,
    pub discard_confirmation_ttl: Duration,
    pub watcher_coalesce: Duration,
    pub watcher_poll_interval: Duration,
    pub watcher_rescan_attempts: u32,
    /// A test-only observation point inside the watcher thread.
    ///
    /// The failure this batch fixes is an event that arrives between a batch being
    /// drained and the watcher being updated. Reproducing it needs the loop held at
    /// exactly that instant — a sleep only makes the race unlikely, and a test that
    /// passes because a race did not happen proves nothing. The hook runs on the
    /// watcher thread after a batch has been applied and before the registration set
    /// is updated, and is `None` in every production configuration.
    pub watcher_batch_hook: Option<WatcherBatchHook>,
}

/// A blocking observation point for the watcher thread. See
/// [`WorkspaceRuntimeOptions::watcher_batch_hook`].
#[derive(Clone)]
pub struct WatcherBatchHook(Arc<dyn Fn() + Send + Sync>);

impl WatcherBatchHook {
    pub fn new(run: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(run))
    }

    fn run(&self) {
        (self.0)();
    }
}

impl std::fmt::Debug for WatcherBatchHook {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("WatcherBatchHook")
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

impl Default for WorkspaceRuntimeOptions {
    fn default() -> Self {
        Self {
            catalog: CatalogOptions::default(),
            lifecycle_prepare_ack_timeout: Duration::from_secs(5),
            discard_confirmation_ttl: Duration::from_secs(60),
            watcher_coalesce: Duration::from_millis(250),
            watcher_poll_interval: Duration::from_secs(5),
            watcher_rescan_attempts: 3,
            watcher_batch_hook: None,
        }
    }
}

pub struct WorkspaceRuntime {
    buffers: Arc<BufferStore>,
    catalogs: RwLock<HashMap<WorkspaceId, Arc<PathCatalog>>>,
    /// The **one** eligibility owner this process has.
    ///
    /// It is created here, at the same assembly point that creates the one `BufferStore` and the
    /// per-workspace resolvers, and every file entry — Main Fs, the Agent file tools, Buffer save,
    /// the artifact writer and the P04 external writer — reads this same instance. That is the
    /// whole requirement: five entries used to keep five unrelated views of one file. The proof is
    /// the pointer identity of this `Arc`, which [`Self::eligibility_owner`] hands out and which
    /// each entry stores, not an exported type or a same-named service.
    eligibility: Arc<EligibilityOwner>,
    lifecycles: Mutex<HashMap<WorkspaceId, lifecycle::LifecycleRuntimeEntry>>,
    options: WorkspaceRuntimeOptions,
    persistence: Arc<PersistenceCoordinator>,
    projection_signals: WorkspaceProjectionSignals,
    resolvers: RwLock<HashMap<WorkspaceId, Arc<SecurePathResolver>>>,
    watchers: Mutex<HashMap<WorkspaceId, WorkspaceWatchHandle>>,
    /// Written once by the open that built this runtime, before it returns.
    open_report: std::sync::OnceLock<WorkspaceOpenReport>,
}

/// What opening the workspace runtime cost, by stage. Counts and milliseconds
/// only: no workspace id, path or catalog content is ever in this record.
///
/// A cold start with a large store spends its time in exactly one of these
/// fields — recovery reconciliation, the session projection, a catalog load,
/// an unproven-text repair, a rebuild scan, or the watcher spawn — and this
/// record is how a reader tells which without re-running anything.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct WorkspaceOpenReport {
    pub mutation_recovery_ms: u64,
    pub lifecycle_recovery_ms: u64,
    pub projection_ms: u64,
    /// Online workspaces whose catalog and watcher were installed at open.
    pub online_workspaces: u64,
    /// Persisted catalog loads across those workspaces.
    pub catalog_load_ms: u64,
    /// Unproven-text repairs across those workspaces.
    pub catalog_repair_ms: u64,
    /// Rebuilt paths the repairs re-read.
    pub repaired_paths: u64,
    /// Rebuild (scan + persist) across those workspaces.
    pub catalog_rebuild_ms: u64,
    /// Files whose stored text a proof still covered at open, so nothing was read.
    pub catalog_reused: u64,
    /// Files whose text had to be read at open: changed, new, unproven or occupied.
    pub catalog_rehydrated: u64,
    /// Of those, the ones read because their volume could not be shown live.
    pub catalog_unproven_volume: u64,
    /// Entries the walk no longer found.
    pub catalog_removed: u64,
    /// Entries whose listed facts moved, so the persisted row was rewritten even
    /// where no text had to be read.
    pub catalog_metadata_changed: u64,
    /// Catalog entries and partitions loaded across those workspaces.
    pub catalog_entries: u64,
    pub catalog_partitions: u64,
    /// Watcher spawns across those workspaces.
    pub watcher_spawn_ms: u64,
    /// The whole open, end to end.
    pub total_ms: u64,
}

/// One online workspace's install cost; aggregated into [`WorkspaceOpenReport`]
/// by the runtime open and discarded everywhere else.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct InstallMs {
    catalog: crate::catalog::CatalogOpenReport,
    rebuild_ms: u64,
    watcher_spawn_ms: u64,
    reconcile: crate::catalog::CatalogReconcileReport,
}

impl WorkspaceRuntime {
    pub fn open(
        persistence: Arc<PersistenceCoordinator>,
        options: WorkspaceRuntimeOptions,
    ) -> Result<Arc<Self>, WorkspaceError> {
        Self::open_with_activity_publisher(persistence, options, None)
    }

    /// Opens the runtime with an activity publisher installed on the buffer store.
    ///
    /// The publisher is passed in rather than built here because the information module lives
    /// *downstream* of this crate: `aura-workspace` must not depend on it, and this seam is what
    /// keeps the dependency pointing the right way. `None` leaves the store exactly as it was.
    pub fn open_with_activity_publisher(
        persistence: Arc<PersistenceCoordinator>,
        options: WorkspaceRuntimeOptions,
        activity_publisher: Option<crate::buffer::BufferCommitPublisher>,
    ) -> Result<Arc<Self>, WorkspaceError> {
        let total_started = Instant::now();
        let mut report = WorkspaceOpenReport::default();
        let started = Instant::now();
        persistence.recover_interrupted_workspace_mutations(CorrelationId::new())?;
        report.mutation_recovery_ms = elapsed_ms(started);
        let started = Instant::now();
        persistence.recover_interrupted_workspace_lifecycles()?;
        report.lifecycle_recovery_ms = elapsed_ms(started);
        let eligibility = Arc::new(EligibilityOwner::new());
        let buffers = {
            let store = BufferStore::with_eligibility_owner(Arc::clone(&eligibility));
            match activity_publisher {
                Some(publisher) => store.with_activity_publisher(publisher),
                None => store,
            }
        };
        let runtime = Arc::new(Self {
            // The store is built **with the shared owner**, so the Buffer, the file tools, the
            // artifact writer and the external writer all hold one `Arc<EligibilityOwner>`. A store
            // built with its own owner would protect nothing, which is the state this batch
            // repairs: the owner existed but no production entry held it.
            buffers: Arc::new(buffers),
            catalogs: RwLock::new(HashMap::new()),
            eligibility,
            lifecycles: Mutex::new(HashMap::new()),
            options,
            persistence,
            projection_signals: WorkspaceProjectionSignals::new(),
            resolvers: RwLock::new(HashMap::new()),
            watchers: Mutex::new(HashMap::new()),
            open_report: std::sync::OnceLock::new(),
        });
        let started = Instant::now();
        let workspaces = runtime.persistence.workspace_session_projection()?.workspaces;
        report.projection_ms = elapsed_ms(started);
        for workspace in workspaces {
            if workspace.state == WorkspaceState::Online {
                let install = runtime.install_online_workspace(workspace, CorrelationId::new())?;
                report.online_workspaces += 1;
                report.catalog_load_ms += install.catalog.load_ms
                    + install.catalog.recent_use_ms
                    + install.catalog.content_paths_ms;
                report.catalog_repair_ms += install.catalog.repair_ms;
                report.repaired_paths += install.catalog.repaired_paths;
                report.catalog_rebuild_ms += install.rebuild_ms;
                report.catalog_reused += install.reconcile.reused;
                report.catalog_rehydrated += install.reconcile.rehydrated;
                report.catalog_unproven_volume += install.reconcile.unproven_volume;
                report.catalog_removed += install.reconcile.removed;
                report.catalog_metadata_changed += install.reconcile.metadata_changed;
                report.catalog_entries += install.catalog.entries;
                report.catalog_partitions += install.catalog.partitions;
                report.watcher_spawn_ms += install.watcher_spawn_ms;
            }
        }
        report.total_ms = elapsed_ms(total_started);
        // Set-once by construction; a second set is a programming error, not a
        // runtime condition, so discard the result rather than fail the open.
        let _ = runtime.open_report.set(report);
        Ok(runtime)
    }

    /// The measured cost of the open that built this runtime.
    pub fn open_report(&self) -> Option<WorkspaceOpenReport> {
        self.open_report.get().copied()
    }

    /// Stops every watcher this runtime started and waits for its thread to end.
    ///
    /// Dropping the handles does the same thing, but not *at a known moment*: the
    /// watcher thread only notices the stop flag on its next turn of the loop, so a test
    /// that drops the runtime and immediately applies a change can still have the
    /// watcher deliver that change first. This makes "the index is quiescent from here"
    /// something a caller can establish instead of hope for, which is what a test that
    /// measures one update in isolation needs. Returns how many watchers were stopped.
    pub fn stop_watchers(&self) -> Result<usize, WorkspaceError> {
        let handles = {
            let mut watchers = self
                .watchers
                .lock()
                .map_err(|_| WorkspaceError::LockPoisoned("workspace watchers"))?;
            std::mem::take(&mut *watchers)
        };
        let stopped = handles.len();
        for (_, handle) in handles {
            // `WorkspaceWatchHandle::drop` sets the flag and joins the thread.
            drop(handle);
        }
        Ok(stopped)
    }

    #[must_use]
    pub fn buffers(&self) -> Arc<BufferStore> {
        Arc::clone(&self.buffers)
    }

    /// The **one** eligibility owner every file entry coordinates through.
    ///
    /// A caller stores the returned `Arc` and shares it: `Arc::ptr_eq` between two entries' handles
    /// is the same-instance proof, and it is the only proof this design accepts. A caller that
    /// builds its own `EligibilityOwner` would protect nothing, and the call-graph check in this
    /// batch's evidence is what catches that.
    #[must_use]
    pub fn eligibility_owner(&self) -> Arc<EligibilityOwner> {
        Arc::clone(&self.eligibility)
    }

    /// The resource facts for one workspace, derived from the resolver the runtime already
    /// verified.
    ///
    /// This is the derivation every entry uses, so Main Fs, the Agent file tools, Buffer and the
    /// artifact writer cannot disagree about what a resource is. The case rule is stated from the
    /// platform's own answer for the root volume rather than assumed.
    ///
    /// # Errors
    /// Returns the real resolver error when the workspace is not online.
    pub fn resource_facts(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
    ) -> Result<ResourceFacts, WorkspaceError> {
        let resolver = self.resolver(workspace_id)?;
        Ok(ResourceFacts::root(resolver, session_id).with_case_rule(volume_case_rule_domain()))
    }

    pub fn resolver(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Arc<SecurePathResolver>, WorkspaceError> {
        self.resolvers
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace resolvers"))?
            .get(&workspace_id)
            .cloned()
            .ok_or(WorkspaceError::Offline)
    }

    pub fn catalog(&self, workspace_id: WorkspaceId) -> Result<Arc<PathCatalog>, WorkspaceError> {
        self.catalogs
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace catalogs"))?
            .get(&workspace_id)
            .cloned()
            .ok_or(WorkspaceError::CatalogUnavailable)
    }

    pub fn open_buffer(
        &self,
        command: BufferOpenCommand,
    ) -> Result<LogicalFileStatus, WorkspaceError> {
        let resolver = self.resolver(command.workspace_id)?;
        let status =
            self.buffers.open(&resolver, command.session_id, command.path, command.writable)?;
        self.notify_projection_change()?;
        Ok(status)
    }

    pub fn edit_buffer(
        &self,
        command: BufferEditCommand,
        correlation_id: CorrelationId,
    ) -> Result<BufferEditResult, WorkspaceError> {
        let workspace_id = command.workspace_id;
        let path = command.path.clone();
        let active = command.active_editing;
        let status = self.buffers.apply_edit(command)?;
        let deferred_intents_superseded = if active {
            0
        } else {
            self.persistence.resolve_deferred_mutations_for_path(
                workspace_id,
                &path,
                correlation_id,
            )?
        };
        let result = BufferEditResult {
            status,
            deferred_intents_superseded,
            requires_model_reread: deferred_intents_superseded > 0,
        };
        self.notify_projection_change()?;
        Ok(result)
    }

    #[must_use]
    pub fn projection_revision(&self) -> Revision {
        Revision::new(self.projection_signals.revision.load(Ordering::Acquire))
    }

    pub fn subscribe_projection_changes(&self) -> Result<Receiver<Revision>, WorkspaceError> {
        self.projection_signals.subscribe()
    }

    pub fn notify_projection_change(&self) -> Result<Revision, WorkspaceError> {
        self.projection_signals.bump()
    }

    /// The Workspace Files projection, built one workspace at a time.
    ///
    /// A workspace whose status cannot be read is **isolated**: it is reported with an
    /// unavailable catalogue and a `Failed` watcher state - a real, visible degradation that
    /// names no other workspace - and every other workspace's status is still produced. One
    /// workspace that is stuck, closed or failing at storage must not remove another
    /// workspace's projection from the same batch, because that batch is what carries an
    /// unrelated workspace's permitted notification.
    pub fn workspace_files_projection(&self) -> Result<WorkspaceFilesProjection, WorkspaceError> {
        let mut statuses = Vec::new();
        for workspace in self.persistence.workspace_session_projection()?.workspaces {
            let workspace_id = workspace.workspace_id;
            match self.workspace_file_status(&workspace.workspace_id, workspace.revision) {
                Ok(status) => statuses.push(status),
                Err(error) => {
                    crate::diagnostics::append_workspace_log(
                        self.persistence.database_path(),
                        "workspace_files_projection_isolated",
                        serde_json::json!({
                            "workspace_id": workspace_id.to_string(),
                            "reason": error.to_string(),
                            "scope": "this workspace only; every other workspace is still projected",
                        }),
                    );
                    statuses.push(WorkspaceFileStatus {
                        workspace_id,
                        workspace_revision: workspace.revision,
                        catalog: WorkspaceCatalogStatus {
                            freshness: CatalogFreshness::Unavailable,
                            catalog_revision: Revision::new(0),
                            indexed_at: None,
                            watcher_state: WorkspaceWatcherState::Failed,
                            partitions: Vec::new(),
                        },
                        logical_files: Vec::new(),
                        editor_reservations: Vec::new(),
                        deferred_mutations: Vec::new(),
                    });
                }
            }
        }
        statuses.sort_by_key(|status| status.workspace_id);
        Ok(WorkspaceFilesProjection {
            workspaces: statuses,
            lifecycle_operations: self.persistence.workspace_lifecycle_operations()?,
        })
    }

    /// One workspace's Workspace Files status. The unit that is isolated above.
    fn workspace_file_status(
        &self,
        workspace_id: &WorkspaceId,
        workspace_revision: Revision,
    ) -> Result<WorkspaceFileStatus, WorkspaceError> {
        let catalog = self.catalog(*workspace_id).ok();
        let catalog_status = if let Some(catalog) = catalog {
            WorkspaceCatalogStatus {
                freshness: self.persistence.workspace(*workspace_id)?.catalog_freshness,
                catalog_revision: catalog.revision()?,
                indexed_at: None,
                watcher_state: self.watcher_state(*workspace_id)?,
                partitions: catalog.partitions()?,
            }
        } else {
            WorkspaceCatalogStatus {
                freshness: self.persistence.workspace(*workspace_id)?.catalog_freshness,
                catalog_revision: Revision::new(0),
                indexed_at: None,
                watcher_state: WorkspaceWatcherState::Failed,
                partitions: Vec::new(),
            }
        };
        Ok(WorkspaceFileStatus {
            workspace_id: *workspace_id,
            workspace_revision,
            catalog: catalog_status,
            logical_files: self.buffers.statuses(*workspace_id)?,
            editor_reservations: self.buffers.reservation_summaries(*workspace_id)?,
            deferred_mutations: self.persistence.pending_deferred_mutations(*workspace_id)?,
        })
    }

    pub fn create(
        &self,
        command: WorkspaceCreateCommand,
        correlation_id: CorrelationId,
    ) -> Result<WorkspaceCreateResult, WorkspaceError> {
        if command.display_name.trim().is_empty() {
            return Err(WorkspaceError::InvalidRoot);
        }
        let selected = PathBuf::from(&command.selected_root);
        let root = SecurePathResolver::inspect_selected_root(&selected)?;
        let identity = domain_identity(&root.identity);
        let canonical_root = root.final_path.display().to_string();
        if let Some(mut existing) = self.persistence.find_workspace_by_root(
            &canonical_root,
            Some(&identity.volume_serial_number),
            Some(&identity.file_id),
        )? {
            let disposition = if existing.state == WorkspaceState::Archived {
                self.ensure_no_active_lifecycle(existing.workspace_id)?;
                existing.state = WorkspaceState::Online;
                existing.catalog_freshness = CatalogFreshness::Unavailable;
                existing.revision = increment(existing.revision)?;
                existing.authorization_epoch = increment(existing.authorization_epoch)?;
                self.persistence.update_workspace(
                    existing.clone(),
                    "workspace.restored",
                    correlation_id,
                    true,
                )?;
                self.install_online_workspace(existing.clone(), correlation_id)?;
                self.notify_projection_change()?;
                WorkspaceCreateDisposition::RestoredArchived
            } else {
                WorkspaceCreateDisposition::Existing
            };
            let current = self.persistence.workspace(existing.workspace_id)?;
            return Ok(WorkspaceCreateResult { disposition, workspace: current });
        }
        let workspace = WorkspaceSummary {
            workspace_id: WorkspaceId::new(),
            display_name: command.display_name,
            selected_root: selected.display().to_string(),
            canonical_root,
            root_identity: Some(identity),
            state: WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        };
        self.persistence.create_workspace(workspace.clone(), correlation_id)?;
        self.install_online_workspace(workspace.clone(), correlation_id)?;
        self.notify_projection_change()?;
        let current = self.persistence.workspace(workspace.workspace_id)?;
        Ok(WorkspaceCreateResult {
            disposition: WorkspaceCreateDisposition::Created,
            workspace: current,
        })
    }

    pub fn restore(
        &self,
        workspace_id: WorkspaceId,
        correlation_id: CorrelationId,
    ) -> Result<WorkspaceSummary, WorkspaceError> {
        self.ensure_no_active_lifecycle(workspace_id)?;
        let mut workspace = self.persistence.workspace(workspace_id)?;
        if workspace.state != WorkspaceState::Archived {
            return Ok(workspace);
        }
        workspace.revision = increment(workspace.revision)?;
        workspace.authorization_epoch = increment(workspace.authorization_epoch)?;
        workspace.catalog_freshness = CatalogFreshness::Unavailable;
        workspace.state = if SecurePathResolver::from_workspace(&WorkspaceSummary {
            state: WorkspaceState::Online,
            ..workspace.clone()
        })
        .is_ok()
        {
            WorkspaceState::Online
        } else {
            WorkspaceState::Offline
        };
        self.persistence.update_workspace(
            workspace.clone(),
            "workspace.restored",
            correlation_id,
            false,
        )?;
        if workspace.state == WorkspaceState::Online {
            self.install_online_workspace(workspace.clone(), correlation_id)?;
        }
        self.notify_projection_change()?;
        self.persistence.workspace(workspace_id).map_err(Into::into)
    }

    pub(super) fn install_online_workspace(
        &self,
        workspace: WorkspaceSummary,
        correlation_id: CorrelationId,
    ) -> Result<InstallMs, WorkspaceError> {
        let resolver = match SecurePathResolver::from_workspace(&workspace) {
            Ok(resolver) => Arc::new(resolver),
            Err(_error) => {
                let mut offline = workspace;
                offline.state = WorkspaceState::Offline;
                offline.catalog_freshness = CatalogFreshness::Unavailable;
                offline.revision = increment(offline.revision)?;
                self.persistence.update_workspace(
                    offline,
                    "workspace.offline",
                    correlation_id,
                    false,
                )?;
                return Ok(InstallMs::default());
            }
        };
        let catalog = Arc::new(PathCatalog::open(
            workspace.workspace_id,
            Arc::clone(&self.persistence),
            self.options.catalog.clone(),
        )?);
        let started = Instant::now();
        // An open reconciles instead of rebuilding: the same walk, with the text of
        // every file whose stored proof still holds left where it is. A rebuild
        // re-reads and re-hashes every file in the workspace on every start, which
        // is what put a large development workspace within seconds of the Core
        // connect budget. A proven lost watcher event still takes the full rebuild
        // below, because that is the one case where nothing may be assumed.
        let (_, reconcile) = catalog.reconcile(&resolver, correlation_id)?;
        let rebuild_ms = elapsed_ms(started);
        self.resolvers
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace resolvers"))?
            .insert(workspace.workspace_id, Arc::clone(&resolver));
        let report = catalog.open_report();
        self.catalogs
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace catalogs"))?
            .insert(workspace.workspace_id, Arc::clone(&catalog));
        let started = Instant::now();
        let watcher = WorkspaceWatchHandle::spawn(
            workspace.workspace_id,
            resolver,
            catalog,
            Arc::clone(&self.persistence),
            self.projection_signals.clone(),
            self.options.watcher_coalesce,
            self.options.watcher_poll_interval,
            self.options.watcher_rescan_attempts,
            self.options.watcher_batch_hook.clone(),
        )?;
        self.watchers
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace watchers"))?
            .insert(workspace.workspace_id, watcher);
        Ok(InstallMs {
            catalog: report,
            rebuild_ms,
            watcher_spawn_ms: elapsed_ms(started),
            reconcile,
        })
    }

    pub(super) fn remove_online_runtime_state(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<(), WorkspaceError> {
        self.watchers
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace watchers"))?
            .remove(&workspace_id);
        self.catalogs
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace catalogs"))?
            .remove(&workspace_id);
        self.resolvers
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace resolvers"))?
            .remove(&workspace_id);
        Ok(())
    }

    fn watcher_state(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<WorkspaceWatcherState, WorkspaceError> {
        self.watchers
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace watchers"))?
            .get(&workspace_id)
            .map(WorkspaceWatchHandle::state)
            .transpose()
            .map(|state| state.unwrap_or(WorkspaceWatcherState::Failed))
    }
}

#[derive(Clone)]
struct WorkspaceProjectionSignals {
    revision: Arc<AtomicU64>,
    subscribers: Arc<Mutex<Vec<Sender<Revision>>>>,
}

impl WorkspaceProjectionSignals {
    fn new() -> Self {
        Self {
            revision: Arc::new(AtomicU64::new(Revision::INITIAL.value())),
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn subscribe(&self) -> Result<Receiver<Revision>, WorkspaceError> {
        let (sender, receiver) = mpsc::channel();
        self.subscribers
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace projection subscribers"))?
            .push(sender);
        Ok(receiver)
    }

    fn bump(&self) -> Result<Revision, WorkspaceError> {
        let next = self
            .revision
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| current.checked_add(1))
            .map_err(|_| WorkspaceError::RevisionExhausted)?
            .checked_add(1)
            .ok_or(WorkspaceError::RevisionExhausted)?;
        let revision = Revision::new(next);
        let mut subscribers = self
            .subscribers
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace projection subscribers"))?;
        subscribers.retain(|subscriber| subscriber.send(revision).is_ok());
        Ok(revision)
    }
}

fn increment(revision: Revision) -> Result<Revision, WorkspaceError> {
    Ok(Revision::new(revision.value().checked_add(1).ok_or(WorkspaceError::RevisionExhausted)?))
}
