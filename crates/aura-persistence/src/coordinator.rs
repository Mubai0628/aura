use std::{
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

use aura_domain_common::{
    CommandInvocationId, CorrelationId, FileIdentity, Revision, RuntimeInstanceId, SessionId,
    UtcTimestamp, WorkspaceId,
};
use aura_permission_domain::SecurityProjectionPayload;
use aura_runtime_contract::RuntimeProcessKind;
use aura_workspace_domain::{
    CatalogFreshness, SessionSummary, WorkspaceSessionProjection, WorkspaceState, WorkspaceSummary,
};
use rusqlite::{Connection, Transaction, params};
use serde_json::json;

use crate::{
    CommittedProjection, DatabaseOpenOptions, DatabasePragmas, EventActor,
    MigrationRecoveryRequired, PersistenceError, PersistentEventDraft,
    migration::{MigrationResult, open_and_migrate},
    mutation_journal::clear_stale_mutation_leases,
    read_pool::{ReadPool, ReadPoolCounters},
    writer::{Deadline, PersistenceCounters, Writer, WriterHealth},
    writer_queue::WorkClass,
};
// The storage hold points exist only in a debug build; every use below carries the
// same gate, so a release build neither compiles the module nor a caller of it.
#[cfg(debug_assertions)]
use crate::barrier::Barrier;

pub use crate::writer::{
    DEFAULT_SETTLED_RECEIPT_BUDGET, INDEX_QUEUE_CAPACITY, JobSettlement, SETTLEMENT_CAPACITY,
    SYNC_QUEUE_CAPACITY, WriterSettlements, WriterState,
};

const DEFAULT_READ_POOL_SIZE: usize = 4;

/// How long a synchronous operation may wait for the writer when its caller did not
/// carry a deadline of its own. It is an internal storage budget, never a public
/// request deadline: callers with a budget pass theirs in, and this only bounds the
/// paths that have none.
pub const DEFAULT_SYNC_DEADLINE: Duration = Duration::from_secs(30);

/// How long one rebuildable index operation may wait for the writer and then run.
///
/// A full workspace rebuild is legitimately long, so this is deliberately the largest
/// budget here - but it is still a budget. Before this existed, a rebuild transaction
/// waited without any bound at all, which is exactly the shape that turned one large
/// index batch into an indefinite stall for everything behind it.
pub const INDEX_DEADLINE: Duration = Duration::from_secs(300);

#[derive(Clone, Debug)]
pub struct PersistenceOptions {
    pub artifact_root: PathBuf,
    pub tool_package_store_root: PathBuf,
    pub database: DatabaseOpenOptions,
    pub database_path: PathBuf,
    pub read_pool_size: usize,
    pub runtime_instance_id: RuntimeInstanceId,
    pub runtime_process_kind: RuntimeProcessKind,
}

impl PersistenceOptions {
    #[must_use]
    pub fn new(
        database_path: PathBuf,
        artifact_root: PathBuf,
        runtime_instance_id: RuntimeInstanceId,
    ) -> Self {
        let tool_package_store_root = database_path.parent().map_or_else(
            || PathBuf::from("tool-package-store"),
            |parent| parent.join("tool-package-store"),
        );
        Self {
            artifact_root,
            tool_package_store_root,
            database: DatabaseOpenOptions::default(),
            database_path,
            read_pool_size: DEFAULT_READ_POOL_SIZE,
            runtime_instance_id,
            runtime_process_kind: RuntimeProcessKind::Core,
        }
    }

    #[must_use]
    pub const fn with_runtime_process_kind(mut self, kind: RuntimeProcessKind) -> Self {
        self.runtime_process_kind = kind;
        self
    }

    #[must_use]
    pub fn with_tool_package_store_root(mut self, root: PathBuf) -> Self {
        self.tool_package_store_root = root;
        self
    }
}

pub enum PersistenceStartup {
    Ready(PersistenceCoordinator),
    RecoveryRequired(MigrationRecoveryRequired),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistenceFaultPoint {
    BeforeCommit,
}

mod permission_gates;
use permission_gates::{AuthorityWrite, PermissionGates};

struct PersistenceInner {
    credential_scope_owner: std::sync::RwLock<
        Option<Arc<dyn crate::tool_credential_journal::ToolCredentialScopeSource>>,
    >,
    /// Per-session authority/start gates. See [`permission_gates::SessionGate`].
    ///
    /// It is its own `Arc` because the commit job submitted for a mode change takes this
    /// session's authority write **itself**, on the writer thread, rather than the async
    /// caller taking it on the reactor.
    permission_gates: Arc<PermissionGates>,
    artifact_root: PathBuf,
    tool_package_store_root: PathBuf,
    database_path: PathBuf,
    readers: ReadPool,
    runtime_instance_id: RuntimeInstanceId,
    writer: Writer,
    /// The index lane's own budget. It is [`INDEX_DEADLINE`] in every production
    /// configuration; a test or the acceptance driver may inject a shorter one so that
    /// "a queued caller that ends at its deadline" can be produced with a real job
    /// instead of a real 300 s wait. No public request deadline moves with it.
    index_deadline_millis: std::sync::atomic::AtomicU64,
    /// The interactive Sync lane's own wait budget.
    ///
    /// It is [`DEFAULT_SYNC_DEADLINE`] in every production configuration; a test or the acceptance
    /// driver may inject a shorter one so that "the writer's own deadline fired while the real job
    /// was admitted and had not finished" can be produced against the real writer and a real
    /// database instead of a real 30 s wait. It is an internal storage budget: no public request
    /// deadline moves with it, and the product default is unchanged.
    sync_deadline_millis: std::sync::atomic::AtomicU64,
    /// How long a caller keeps receiving an identified job's result after its own wait for
    /// that job ended.
    ///
    /// It is [`DEFAULT_SETTLED_RECEIPT_BUDGET`] in every production configuration. A test or
    /// the acceptance driver may inject a **shorter** one so that "the value never arrived
    /// inside the receipt window" can be produced without waiting the production budget; it
    /// is an internal receipt window, never a public request deadline, and nothing may
    /// lengthen it.
    settled_receipt_millis: std::sync::atomic::AtomicU64,
    workspace_session_subscribers:
        Mutex<Vec<mpsc::Sender<CommittedProjection<WorkspaceSessionProjection>>>>,
    security_subscribers: Mutex<Vec<mpsc::Sender<CommittedProjection<SecurityProjectionPayload>>>>,
    model_projection_subscribers:
        Mutex<Vec<mpsc::Sender<aura_projection_contract::ProjectionChange>>>,
    tool_projection_subscribers:
        Mutex<Vec<mpsc::Sender<aura_projection_contract::ProjectionChange>>>,
}

pub use crate::writer::PersistenceArtifactCounters;

/// A test-only handshake inside the single writer thread.
///
/// The ticket is handed out by [`PersistenceCoordinator::arm_dispatch_gate`] and
/// consumed by the next task the writer picks up: the task announces itself, then
/// waits for the ticket to be released before it touches the connection. That is what
/// lets a test hold a real write transaction open — not a fake that returns a fixed
/// slow value — while it calls the real read paths that would queue behind it.
#[doc(hidden)]
#[derive(Debug)]
pub struct PersistenceDispatchGate {
    arrived: Mutex<bool>,
    arrived_signal: std::sync::Condvar,
    released: Mutex<bool>,
    released_signal: std::sync::Condvar,
}

#[doc(hidden)]
impl PersistenceDispatchGate {
    pub(crate) fn new() -> Self {
        Self {
            arrived: Mutex::new(false),
            arrived_signal: std::sync::Condvar::new(),
            released: Mutex::new(false),
            released_signal: std::sync::Condvar::new(),
        }
    }

    pub(crate) fn announce(&self) {
        if let Ok(mut arrived) = self.arrived.lock() {
            *arrived = true;
            self.arrived_signal.notify_all();
        }
        if let Ok(mut released) = self.released.lock() {
            while !*released {
                match self.released_signal.wait(released) {
                    Ok(guard) => released = guard,
                    Err(_) => break,
                }
            }
        }
    }

    /// Waits until the armed task has reached the gate.
    pub fn wait_until_arrived(&self, timeout: std::time::Duration) -> bool {
        let Ok(arrived) = self.arrived.lock() else {
            return false;
        };
        match self.arrived_signal.wait_timeout_while(arrived, timeout, |arrived| !*arrived) {
            Ok((guard, _)) => *guard,
            Err(_) => false,
        }
    }

    /// Lets the armed task run to completion.
    pub fn release(&self) {
        if let Ok(mut released) = self.released.lock() {
            *released = true;
            self.released_signal.notify_all();
        }
    }
}

impl Drop for PersistenceInner {
    fn drop(&mut self) {
        // The writer's own `Drop` closes admission once, waits a bounded time and
        // records what it found. Nothing here blocks indefinitely on a writer that is
        // still inside a job, and no unbounded number of shutdown messages is sent.
        let health = self.writer.health();
        if !health.joined {
            let _ = self.writer.shutdown(crate::writer::SHUTDOWN_JOIN_BUDGET);
        }
    }
}

#[derive(Clone)]
pub struct PersistenceCoordinator {
    inner: Arc<PersistenceInner>,
}

impl PersistenceCoordinator {
    pub fn bind_tool_credential_scope_owner(
        &self,
        owner: Arc<dyn crate::tool_credential_journal::ToolCredentialScopeSource>,
    ) -> Result<(), PersistenceError> {
        let mut current = self
            .inner
            .credential_scope_owner
            .write()
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        if current.is_none() {
            *current = Some(owner);
        }
        Ok(())
    }

    pub(crate) fn tool_credential_scope_owner(
        &self,
    ) -> Result<
        Option<Arc<dyn crate::tool_credential_journal::ToolCredentialScopeSource>>,
        PersistenceError,
    > {
        self.inner
            .credential_scope_owner
            .read()
            .map(|owner| owner.clone())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)
    }

    pub fn start(options: PersistenceOptions) -> Result<PersistenceStartup, PersistenceError> {
        let recover_tool_conformance = options.runtime_process_kind == RuntimeProcessKind::Core;
        std::fs::create_dir_all(&options.artifact_root).map_err(|source| PersistenceError::Io {
            action: "create artifact object directory",
            path: options.artifact_root.clone(),
            source,
        })?;
        aura_tool_package::ToolPackageStoreV1::open(&options.tool_package_store_root)
            .map_err(|error| PersistenceError::InvalidRequest(error.to_string()))?;
        let mut writer = match open_and_migrate(&options.database_path, options.database)? {
            MigrationResult::Ready(connection) => connection,
            MigrationResult::RecoveryRequired(recovery) => {
                return Ok(PersistenceStartup::RecoveryRequired(recovery));
            }
        };
        register_runtime(&mut writer, options.runtime_instance_id, options.runtime_process_kind)?;
        clear_stale_mutation_leases(&mut writer, options.runtime_instance_id)?;
        let readers = ReadPool::open(&options.database_path, options.read_pool_size.clamp(1, 16))?;
        let counters = Arc::new(PersistenceCounters::default());
        let gate = Arc::new(Mutex::new(None));
        // A hold point is installed only when a test or the acceptance driver armed one;
        // in every product configuration this is `None` and the writer is unchanged.
        // Debug-only: a release build has no hold point to arm and passes none.
        #[cfg(debug_assertions)]
        let barrier = Barrier::from_environment();
        let writer = Writer::start(
            writer,
            Arc::clone(&counters),
            Arc::clone(&gate),
            options.database_path.clone(),
            #[cfg(debug_assertions)]
            barrier,
        )?;
        let coordinator = Self {
            inner: Arc::new(PersistenceInner {
                credential_scope_owner: std::sync::RwLock::new(None),
                permission_gates: Arc::new(PermissionGates::default()),
                artifact_root: options.artifact_root,
                tool_package_store_root: options.tool_package_store_root,
                database_path: options.database_path,
                readers,
                runtime_instance_id: options.runtime_instance_id,
                writer,
                index_deadline_millis: std::sync::atomic::AtomicU64::new(
                    u64::try_from(INDEX_DEADLINE.as_millis()).unwrap_or(u64::MAX),
                ),
                sync_deadline_millis: std::sync::atomic::AtomicU64::new(
                    u64::try_from(DEFAULT_SYNC_DEADLINE.as_millis()).unwrap_or(u64::MAX),
                ),
                settled_receipt_millis: std::sync::atomic::AtomicU64::new(
                    u64::try_from(DEFAULT_SETTLED_RECEIPT_BUDGET.as_millis()).unwrap_or(u64::MAX),
                ),
                workspace_session_subscribers: Mutex::new(Vec::new()),
                security_subscribers: Mutex::new(Vec::new()),
                model_projection_subscribers: Mutex::new(Vec::new()),
                tool_projection_subscribers: Mutex::new(Vec::new()),
            }),
        };
        if recover_tool_conformance {
            // Retired the P07 Conformance, package publish and delete journals;
            // only P07 tool executions still need their startup recovery.
            coordinator.recover_incomplete_tool_executions()?;
        }
        Ok(PersistenceStartup::Ready(coordinator))
    }

    #[must_use]
    pub fn database_path(&self) -> &std::path::Path {
        &self.inner.database_path
    }

    #[must_use]
    pub fn tool_package_store_root(&self) -> &std::path::Path {
        &self.inner.tool_package_store_root
    }

    #[must_use]
    pub fn runtime_instance_id(&self) -> RuntimeInstanceId {
        self.inner.runtime_instance_id
    }

    pub fn pragmas(&self) -> Result<DatabasePragmas, PersistenceError> {
        // `with_configured`, not `with`: a read installs its own busy budget on the
        // connection for the duration of its call, and a diagnostic that reported that
        // would be measuring itself rather than the configuration.
        self.inner.readers.with_configured(|connection| {
            let foreign_keys =
                connection.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, bool>(0))?;
            let journal_mode =
                connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?;
            let busy_timeout =
                connection.query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))?;
            Ok(DatabasePragmas {
                foreign_keys,
                journal_mode_wal: journal_mode.eq_ignore_ascii_case("wal"),
                busy_timeout_milliseconds: u64::try_from(busy_timeout).map_err(|_| {
                    PersistenceError::InvalidRequest("negative SQLite busy timeout".to_owned())
                })?,
            })
        })
    }

    pub fn workspace_session_projection(
        &self,
    ) -> Result<WorkspaceSessionProjection, PersistenceError> {
        self.inner.readers.with(load_workspace_session_projection)
    }

    pub fn subscribe_workspace_sessions(
        &self,
    ) -> Result<Receiver<CommittedProjection<WorkspaceSessionProjection>>, PersistenceError> {
        let (sender, receiver) = mpsc::channel();
        self.inner
            .workspace_session_subscribers
            .lock()
            .map_err(|_| PersistenceError::SubscriptionLockPoisoned)?
            .push(sender);
        Ok(receiver)
    }

    pub fn subscribe_security(
        &self,
    ) -> Result<Receiver<CommittedProjection<SecurityProjectionPayload>>, PersistenceError> {
        let (sender, receiver) = mpsc::channel();
        self.inner
            .security_subscribers
            .lock()
            .map_err(|_| PersistenceError::SubscriptionLockPoisoned)?
            .push(sender);
        Ok(receiver)
    }

    pub fn subscribe_model_projections(
        &self,
    ) -> Result<Receiver<aura_projection_contract::ProjectionChange>, PersistenceError> {
        let (sender, receiver) = mpsc::channel();
        self.inner
            .model_projection_subscribers
            .lock()
            .map_err(|_| PersistenceError::SubscriptionLockPoisoned)?
            .push(sender);
        Ok(receiver)
    }

    pub fn publish_model_projection_changes(
        &self,
        changes: &[aura_projection_contract::ProjectionChange],
    ) -> Result<(), PersistenceError> {
        let mut subscribers = self
            .inner
            .model_projection_subscribers
            .lock()
            .map_err(|_| PersistenceError::SubscriptionLockPoisoned)?;
        subscribers.retain(|subscriber| {
            changes.iter().all(|change| subscriber.send(change.clone()).is_ok())
        });
        Ok(())
    }

    pub fn subscribe_tool_projections(
        &self,
    ) -> Result<Receiver<aura_projection_contract::ProjectionChange>, PersistenceError> {
        let (sender, receiver) = mpsc::channel();
        self.inner
            .tool_projection_subscribers
            .lock()
            .map_err(|_| PersistenceError::SubscriptionLockPoisoned)?
            .push(sender);
        Ok(receiver)
    }

    /// Publishes only already-committed Tool projection facts. Callers must
    /// invoke this after the transaction returning the corresponding
    /// `ProjectionChange` values has committed successfully.
    pub fn publish_tool_projection_changes(
        &self,
        changes: &[aura_projection_contract::ProjectionChange],
    ) -> Result<(), PersistenceError> {
        let mut subscribers = self
            .inner
            .tool_projection_subscribers
            .lock()
            .map_err(|_| PersistenceError::SubscriptionLockPoisoned)?;
        subscribers.retain(|subscriber| {
            changes.iter().all(|change| subscriber.send(change.clone()).is_ok())
        });
        Ok(())
    }

    pub fn upsert_workspace_session(
        &self,
        workspace: WorkspaceSummary,
        session: SessionSummary,
        correlation_id: CorrelationId,
    ) -> Result<CommittedProjection<WorkspaceSessionProjection>, PersistenceError> {
        self.upsert_workspace_session_with_fault(workspace, session, correlation_id, None)
    }

    pub fn upsert_workspace_session_with_fault(
        &self,
        workspace: WorkspaceSummary,
        session: SessionSummary,
        correlation_id: CorrelationId,
        fault: Option<PersistenceFaultPoint>,
    ) -> Result<CommittedProjection<WorkspaceSessionProjection>, PersistenceError> {
        validate_workspace_session(&workspace, &session)?;
        let committed = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let now = UtcTimestamp::now().to_string();
            let workspace_revision = i64::try_from(workspace.revision.value()).map_err(|_| {
                PersistenceError::InvalidRequest(
                    "workspace revision exceeds SQLite integer range".to_owned(),
                )
            })?;
            let authorization_epoch = i64::try_from(workspace.authorization_epoch.value())
                .map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "authorization epoch exceeds SQLite integer range".to_owned(),
                    )
                })?;
            transaction.execute(
                "INSERT INTO workspaces(
                    workspace_id, display_name, selected_root, canonical_root,
                    root_volume_serial, root_file_id, state, revision, catalog_freshness,
                    authorization_epoch, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
                 ON CONFLICT(workspace_id) DO UPDATE SET
                    display_name=excluded.display_name,
                    selected_root=excluded.selected_root,
                    canonical_root=excluded.canonical_root,
                    root_volume_serial=excluded.root_volume_serial,
                    root_file_id=excluded.root_file_id,
                    state=excluded.state,
                    revision=excluded.revision,
                    catalog_freshness=excluded.catalog_freshness,
                    authorization_epoch=excluded.authorization_epoch,
                    updated_at=excluded.updated_at",
                params![
                    workspace.workspace_id.to_string(),
                    workspace.display_name,
                    workspace.selected_root,
                    workspace.canonical_root,
                    workspace.root_identity.as_ref().map(|value| &value.volume_serial_number),
                    workspace.root_identity.as_ref().map(|value| &value.file_id),
                    workspace_state_name(workspace.state),
                    workspace_revision,
                    catalog_freshness_name(workspace.catalog_freshness),
                    authorization_epoch,
                    now
                ],
            )?;
            transaction.execute(
                "INSERT INTO sessions(
                    session_id, workspace_id, title, state, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT(session_id) DO UPDATE SET
                    workspace_id=excluded.workspace_id,
                    title=excluded.title,
                    state=excluded.state,
                    updated_at=excluded.updated_at",
                params![
                    session.session_id.to_string(),
                    session.workspace_id.to_string(),
                    session.title,
                    session.state,
                    now
                ],
            )?;
            let mut event = PersistentEventDraft::system(
                "workspace_session.upserted",
                correlation_id,
                json!({
                    "workspace_id": workspace.workspace_id,
                    "session_id": session.session_id,
                }),
            );
            event.workspace_id = Some(workspace.workspace_id);
            event.session_id = Some(session.session_id);
            insert_event(&transaction, &event)?;
            let payload = load_workspace_session_projection(&transaction)?;
            let source_revision =
                next_projection_revision(&transaction, "workspace_session", "global")?;
            let payload_json = serde_json::to_string(&payload)?;
            transaction.execute(
                "INSERT INTO projection_outbox(
                    projection_kind, projection_key, revision, payload_json, created_at
                 ) VALUES ('workspace_session', 'global', ?1, ?2, ?3)",
                params![
                    i64::try_from(source_revision).map_err(|_| {
                        PersistenceError::InvalidRequest(
                            "projection revision exceeds SQLite integer range".to_owned(),
                        )
                    })?,
                    payload_json,
                    UtcTimestamp::now().to_string()
                ],
            )?;
            let outbox_id = transaction.last_insert_rowid();
            if fault == Some(PersistenceFaultPoint::BeforeCommit) {
                return Err(PersistenceError::InvalidRequest(
                    "injected persistence failure before commit".to_owned(),
                ));
            }
            transaction.commit()?;
            Ok(CommittedProjection { outbox_id, source_revision, payload })
        })?;
        self.publish_workspace_session(committed.clone())?;
        let outbox_id = committed.outbox_id;
        let _ = self.dispatch(move |connection| {
            connection.execute(
                "UPDATE projection_outbox SET published_at=?1 WHERE outbox_id=?2",
                params![UtcTimestamp::now().to_string(), outbox_id],
            )?;
            Ok(())
        });
        Ok(committed)
    }

    pub fn rebuild_workspace_session_projection(
        &self,
    ) -> Result<CommittedProjection<WorkspaceSessionProjection>, PersistenceError> {
        let committed = self.dispatch(|connection| {
            let transaction = connection.transaction()?;
            let payload = load_workspace_session_projection(&transaction)?;
            let source_revision =
                next_projection_revision(&transaction, "workspace_session", "global")?;
            transaction.execute(
                "INSERT INTO projection_outbox(
                    projection_kind, projection_key, revision, payload_json, created_at
                 ) VALUES ('workspace_session', 'global', ?1, ?2, ?3)",
                params![
                    i64::try_from(source_revision).map_err(|_| {
                        PersistenceError::InvalidRequest(
                            "projection revision exceeds SQLite integer range".to_owned(),
                        )
                    })?,
                    serde_json::to_string(&payload)?,
                    UtcTimestamp::now().to_string()
                ],
            )?;
            let outbox_id = transaction.last_insert_rowid();
            transaction.commit()?;
            Ok(CommittedProjection { outbox_id, source_revision, payload })
        })?;
        self.publish_workspace_session(committed.clone())?;
        let outbox_id = committed.outbox_id;
        let _ = self.dispatch(move |connection| {
            connection.execute(
                "UPDATE projection_outbox SET published_at=?1 WHERE outbox_id=?2",
                params![UtcTimestamp::now().to_string(), outbox_id],
            )?;
            Ok(())
        });
        Ok(committed)
    }

    pub fn replay_projection_outbox(&self) -> Result<usize, PersistenceError> {
        let pending = self.inner.readers.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT outbox_id, revision, payload_json
                 FROM projection_outbox
                 WHERE projection_kind='workspace_session' AND published_at IS NULL
                 ORDER BY outbox_id",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        let count = pending.len();
        for (outbox_id, revision, payload) in pending {
            let committed = CommittedProjection {
                outbox_id,
                source_revision: u64::try_from(revision).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "negative projection revision in outbox".to_owned(),
                    )
                })?,
                payload: serde_json::from_str(&payload)?,
            };
            self.publish_workspace_session(committed)?;
            self.dispatch(move |connection| {
                connection.execute(
                    "UPDATE projection_outbox SET published_at=?1 WHERE outbox_id=?2",
                    params![UtcTimestamp::now().to_string(), outbox_id],
                )?;
                Ok(())
            })?;
        }
        Ok(count)
    }

    pub fn acquire_runtime_lease(
        &self,
        owner_kind: impl Into<String>,
        owner_id: impl Into<String>,
    ) -> Result<String, PersistenceError> {
        let owner_kind = owner_kind.into();
        let owner_id = owner_id.into();
        if owner_kind.is_empty() || owner_id.is_empty() {
            return Err(PersistenceError::InvalidRequest(
                "runtime lease owner must not be empty".to_owned(),
            ));
        }
        let lease_id = format!("lea_{}", ulid::Ulid::new());
        let inserted = lease_id.clone();
        let runtime = self.inner.runtime_instance_id;
        self.dispatch(move |connection| {
            if owner_kind == "workspace" {
                crate::workspace::ensure_workspace_accepts_new_activity_by_id(
                    connection, &owner_id,
                )?;
            }
            connection.execute(
                "INSERT INTO runtime_leases(
                    lease_id, runtime_instance_id, owner_kind, owner_id, heartbeat_at, state
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'active')",
                params![
                    inserted,
                    runtime.to_string(),
                    owner_kind,
                    owner_id,
                    UtcTimestamp::now().to_string()
                ],
            )?;
            Ok(())
        })?;
        Ok(lease_id)
    }

    pub fn heartbeat_runtime_lease(&self, lease_id: &str) -> Result<bool, PersistenceError> {
        let lease_id = lease_id.to_owned();
        let runtime = self.inner.runtime_instance_id;
        self.dispatch(move |connection| {
            let changed = connection.execute(
                "UPDATE runtime_leases SET heartbeat_at=?1
                 WHERE lease_id=?2 AND runtime_instance_id=?3 AND state='active'",
                params![UtcTimestamp::now().to_string(), lease_id, runtime.to_string()],
            )?;
            Ok(changed == 1)
        })
    }

    pub fn release_runtime_lease(&self, lease_id: &str) -> Result<bool, PersistenceError> {
        let lease_id = lease_id.to_owned();
        let runtime = self.inner.runtime_instance_id;
        self.dispatch(move |connection| {
            let changed = connection.execute(
                "UPDATE runtime_leases SET state='released', heartbeat_at=?1
                 WHERE lease_id=?2 AND runtime_instance_id=?3 AND state='active'",
                params![UtcTimestamp::now().to_string(), lease_id, runtime.to_string()],
            )?;
            Ok(changed == 1)
        })
    }

    pub fn event_count(&self) -> Result<u64, PersistenceError> {
        self.inner.readers.with(|connection| {
            let count = connection
                .query_row("SELECT COUNT(*) FROM events", [], |row| row.get::<_, i64>(0))?;
            u64::try_from(count)
                .map_err(|_| PersistenceError::InvalidRequest("negative event count".to_owned()))
        })
    }

    pub fn event_search_entry_count(&self) -> Result<u64, PersistenceError> {
        self.inner.readers.with(|connection| {
            let count =
                connection.query_row("SELECT COUNT(*) FROM event_search_fts", [], |row| {
                    row.get::<_, i64>(0)
                })?;
            u64::try_from(count).map_err(|_| {
                PersistenceError::InvalidRequest("negative event search count".to_owned())
            })
        })
    }

    pub fn pending_outbox_count(&self) -> Result<u64, PersistenceError> {
        self.inner.readers.with(|connection| {
            let count = connection.query_row(
                "SELECT COUNT(*) FROM projection_outbox WHERE published_at IS NULL",
                [],
                |row| row.get::<_, i64>(0),
            )?;
            u64::try_from(count).map_err(|_| {
                PersistenceError::InvalidRequest("negative projection outbox count".to_owned())
            })
        })
    }

    /// Runs one synchronous write-class operation on the writer, with the internal
    /// default deadline.
    ///
    /// This is the ordinary entry point for work a caller is waiting on. It refuses at
    /// once if the synchronous lane is full ([`PersistenceError::WriterBusy`]: the work
    /// has not run) and reports [`PersistenceError::WriterUnresponsive`] if the writer
    /// did not answer inside the budget (the answer is unknown, so the caller must not
    /// present it as "not executed").
    pub(crate) fn dispatch<T, F>(&self, operation: F) -> Result<T, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        self.dispatch_class(WorkClass::Sync, Some(Instant::now() + self.sync_deadline()), operation)
    }

    /// The interactive Sync lane's current wait budget.
    ///
    /// [`DEFAULT_SYNC_DEADLINE`] unless [`Self::set_sync_deadline_for_test`] injected a shorter
    /// one. It is an internal storage budget, never a public request deadline.
    #[must_use]
    pub fn sync_deadline(&self) -> Duration {
        Duration::from_millis(
            self.inner.sync_deadline_millis.load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Injects a shorter Sync-lane budget, for a test or the acceptance driver only.
    ///
    /// This exists so that "the writer's own wait deadline fired while the real job was admitted
    /// and had not finished" can be produced against the real writer, the real queue and a real
    /// database file, without really waiting the production [`DEFAULT_SYNC_DEADLINE`]. The
    /// production default is not changed by it, it is not reachable from any product
    /// configuration, and the value it sets belongs to this coordinator instance.
    #[doc(hidden)]
    pub fn set_sync_deadline_for_test(&self, deadline: Duration) {
        self.inner.sync_deadline_millis.store(
            u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// The receipt window an identified business write's caller gets after its own wait for
    /// that write ended.
    ///
    /// [`DEFAULT_SETTLED_RECEIPT_BUDGET`] unless [`Self::set_settled_receipt_budget_for_test`]
    /// injected a shorter one. It only ever decides how long a caller keeps *listening* for a
    /// result the job already produced; it is never a deadline the store's work is judged by.
    #[must_use]
    pub fn settled_receipt_budget(&self) -> Duration {
        Duration::from_millis(
            self.inner.settled_receipt_millis.load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Injects a **shorter** receipt window, for a test or the acceptance driver only.
    ///
    /// This exists so that "the job produced its result after the caller's own wait ended"
    /// and "the job never produced anything inside the window" can both be produced without
    /// waiting the production [`DEFAULT_SETTLED_RECEIPT_BUDGET`]. The production default is
    /// not changed by it, it is not reachable from any product configuration, it belongs to
    /// this coordinator instance, and a value longer than the default is refused: the window
    /// may be made cheaper to observe, never more expensive for the product.
    #[doc(hidden)]
    pub fn set_settled_receipt_budget_for_test(&self, budget: Duration) {
        let requested = u64::try_from(budget.as_millis()).unwrap_or(u64::MAX);
        let ceiling = u64::try_from(DEFAULT_SETTLED_RECEIPT_BUDGET.as_millis()).unwrap_or(u64::MAX);
        self.inner
            .settled_receipt_millis
            .store(requested.min(ceiling), std::sync::atomic::Ordering::Relaxed);
    }

    /// The index lane's own budget as an absolute deadline, for a caller that adds an
    /// identity to the job.
    ///
    /// Index work is a separate lane, so it never occupies the interactive lane's
    /// capacity and never runs ahead of an interactive job that is already waiting — and
    /// the fairness bound in the queue means it is not starved by one either. Its deadline
    /// is finite: a rebuild is long, so it gets a long budget, but no caller waits without
    /// one.
    pub(crate) fn index_deadline_instant(&self) -> Instant {
        Instant::now() + self.index_deadline()
    }

    /// The index lane's current budget: [`INDEX_DEADLINE`] unless
    /// [`Self::set_index_deadline_for_test`] injected a shorter one.
    ///
    /// It is an internal storage budget, never a public request deadline.
    #[must_use]
    pub fn index_deadline(&self) -> Duration {
        Duration::from_millis(
            self.inner.index_deadline_millis.load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Injects a shorter index budget, for a test or the acceptance driver only.
    ///
    /// This exists so that "a queued caller that ends at its own deadline" can be produced
    /// against the real coordinator, the real worker and a real database file, without
    /// really waiting the production [`INDEX_DEADLINE`]. The production default is not
    /// changed by it, it is not reachable from any product configuration, and the value it
    /// sets belongs to this coordinator instance (the same real index lane and the same
    /// commit path, with a shorter clock).
    #[doc(hidden)]
    pub fn set_index_deadline_for_test(&self, deadline: Duration) {
        self.inner.index_deadline_millis.store(
            u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// Runs one write-class operation under a caller-supplied total deadline.
    ///
    /// The deadline is absolute and monotonic and is carried into the writer, where the
    /// SQLite busy handler is set from what is left of it: enqueue wait, SQL lock wait
    /// and the operation itself all come out of the same budget. It is never reset
    /// between stages.
    #[allow(dead_code)]
    pub(crate) fn dispatch_within<T, F>(
        &self,
        class: WorkClass,
        deadline: Instant,
        operation: F,
    ) -> Result<T, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        self.dispatch_class(class, Some(deadline), operation)
    }

    fn dispatch_class<T, F>(
        &self,
        class: WorkClass,
        deadline: Deadline,
        operation: F,
    ) -> Result<T, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        self.inner.writer.submit(class, deadline, operation)
    }

    /// The same dispatch, with the caller's identity attached to the job.
    ///
    /// The identity is what turns "the caller stopped waiting" into something resolvable:
    /// the job records its terminal state under that name whether or not the caller was
    /// still listening, so the outcome can be looked up — by identity, never by resending
    /// the work.
    pub(crate) fn dispatch_class_identified<T, F>(
        &self,
        class: WorkClass,
        deadline: Deadline,
        identity: String,
        operation: F,
    ) -> Result<T, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        self.inner.writer.submit_identified(class, deadline, Some(identity), operation)
    }

    /// The same dispatch, for work whose **value** its caller is owed.
    ///
    /// It differs from [`Self::dispatch_class_identified`] in exactly one way: when the
    /// caller's own deadline expires while the job is still running, the caller keeps
    /// listening for the job's real result for [`SETTLED_RECEIPT_BUDGET`] instead of being
    /// told only "the writer did not answer". A business write's caller is owed what the
    /// store wrote, and a transaction that commits just after that deadline is still this
    /// write's real result - discarding it would be reporting an unknown about a value the
    /// runtime already has.
    ///
    /// It is not a longer wait for the write to be fast, and it is not a retry: the result
    /// returned is the one the thread that ran the job produced, from the same single
    /// execution. When nothing arrives it is [`PersistenceError::WriterUnresponsive`], the
    /// same honest answer the other entry gives.
    pub(crate) fn dispatch_class_settled<T, F>(
        &self,
        class: WorkClass,
        deadline: Deadline,
        identity: String,
        operation: F,
    ) -> Result<T, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        self.inner.writer.submit_settled(
            class,
            deadline,
            identity,
            self.settled_receipt_budget(),
            operation,
        )
    }

    pub(crate) fn dispatch_receipt<T, F>(
        &self,
        identity: String,
        operation: F,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<T, PersistenceError>>, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        self.inner.writer.submit_receipt(
            WorkClass::Sync,
            Some(Instant::now() + self.sync_deadline()),
            identity,
            operation,
        )
    }

    /// One interactive write, submitted so that its caller **awaits** the result instead
    /// of parking a thread for it.
    ///
    /// It is [`Self::dispatch_receipt`] under the identity a caller can look up later,
    /// and it changes nothing about the job: the same `WorkClass::Sync` lane, the same
    /// admission bound (`WriterBusy` means the work provably did not run), the same
    /// [`Self::sync_deadline`] budget (`WriterUnresponsive` means admitted and not
    /// answered, never "did not run"), the same single writer connection and the same
    /// submission-time refusal for a closed queue.
    ///
    /// The submission itself is non-blocking by construction: the queue's admission is a
    /// short `Mutex` critical section that never waits for the writer to become free, so
    /// calling this from a runtime worker cannot hand that worker to the storage layer.
    /// Everything the caller then waits for - the enqueue-to-execution wait, the SQL
    /// lock wait and the transaction - is waited for asynchronously.
    pub(crate) fn dispatch_awaitable<T, F>(
        &self,
        identity: String,
        operation: F,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<T, PersistenceError>>, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        self.dispatch_receipt(identity, operation)
    }

    /// The identity a workspace catalog write is recorded under.
    ///
    /// A caller that keeps the correlation id it passed can resolve its own write's
    /// terminal state later; the value is derived, never stored, so the same pair always
    /// produces the same identity.
    #[must_use]
    pub fn workspace_catalog_write_identity(
        workspace_id: WorkspaceId,
        correlation_id: CorrelationId,
    ) -> String {
        format!("workspace-catalog:{workspace_id}:{correlation_id}")
    }

    /// The identity a session business write is recorded under.
    ///
    /// A session write's caller is owed the value it wrote, so the job carries the original
    /// invocation as its identity: that is what lets the result of a write whose caller
    /// stopped waiting be received rather than discarded, and it is derived, never stored,
    /// so the same invocation always produces the same identity.
    #[must_use]
    pub fn session_write_identity(command: CommandInvocationId) -> String {
        format!("session-write:{command}")
    }

    /// The terminal record of the write submitted under `identity`, once it has settled.
    ///
    /// `None` means no job with that identity has finished: it was never admitted, or it is
    /// still executing. It never means "it did not happen" — a caller that was told
    /// [`PersistenceError::WriterUnresponsive`] has an *unknown* answer until this returns
    /// something.
    #[must_use]
    pub fn job_settlement(&self, identity: &str) -> Option<JobSettlement> {
        self.inner.writer.settlement_of(identity)
    }

    /// The terminal records this runtime's writer has kept, with the counts behind them.
    #[must_use]
    pub fn job_settlements(&self) -> WriterSettlements {
        self.inner.writer.settlements()
    }

    /// How much SQLite work this runtime's writer has done so far.
    ///
    /// The counters are absolute and monotonic, so a caller reports the delta around
    /// one operation. `vm_steps` is the writer connection's executed virtual-machine
    /// instructions: it separates "the same rows were changed with a different plan"
    /// from "fewer rows were changed", which is exactly the difference between an
    /// indexed delete and a table scan that produces the same result.
    #[must_use]
    pub fn measured_work(&self) -> PersistenceArtifactCounters {
        self.inner.writer.measured()
    }

    /// What the writer is doing right now: its state, its queue depths and whether it
    /// is still there. Counts and states only.
    #[must_use]
    pub fn writer_health(&self) -> WriterHealth {
        self.inner.writer.health()
    }

    /// The read pool's own counters: served, waited, refused on deadline, replaced.
    #[must_use]
    pub fn read_pool_counters(&self) -> ReadPoolCounters {
        self.inner.readers.counters()
    }

    #[must_use]
    pub fn read_pool_size(&self) -> usize {
        self.inner.readers.size()
    }

    /// Arms a gate that the next writer task must pass through.
    ///
    /// Used by tests that need a real write to be in flight while they call the real
    /// read paths, so "the read queued behind the writer" is observed rather than
    /// assumed. The gate is held by the returned handle and only affects the very next
    /// task; every later task runs normally even if the gate is never released.
    #[doc(hidden)]
    pub fn arm_dispatch_gate(&self) -> Arc<PersistenceDispatchGate> {
        self.inner.writer.arm_dispatch_gate()
    }

    /// Asks the writer to count the instructions of everything it runs from now on.
    ///
    /// `measured_work` then reports a virtual-machine step count for the statements a
    /// caller just asked for, which is what makes "the same rows, a different plan"
    /// observable. Measuring is off in every production configuration.
    #[doc(hidden)]
    pub fn measure_work(&self) {
        self.inner.writer.measure_work();
    }

    /// The single current mode/revision authority. Snapshots in either journal
    /// are historical facts. Hold this barrier through dispatch reservation (and
    /// launch registration) so a concurrent UI mode change cannot race the read.
    /// The callback must not call a mode setter or wait for process completion.
    ///
    /// The barrier is **this session's** authority gate. A mode change of an
    /// unrelated session neither waits for this section nor is waited on by it.
    pub fn with_session_permission_authority<T>(
        &self,
        session_id: aura_domain_common::SessionId,
        use_authority: impl FnOnce(&aura_permission_domain::SessionPermissionProjection) -> T,
    ) -> Result<T, PersistenceError> {
        let gate = self.inner.permission_gates.of(session_id);
        let _barrier = gate.authority.read()?;
        let current = self.session_permission_state(session_id)?;
        Ok(use_authority(&current))
    }

    /// The write side of one session's authority gate. A mode change of `session_id`
    /// takes it, so it cannot commit inside that session's live authority section.
    ///
    /// This is the **blocking** acquisition, for a caller that already runs whole on a
    /// bounded blocking owner. An async caller uses [`Self::lock_permission_update_awaitable`]
    /// instead.
    pub(crate) fn lock_permission_update(
        &self,
        session_id: aura_domain_common::SessionId,
    ) -> Result<AuthorityWrite, PersistenceError> {
        self.inner.permission_gates.of(session_id).authority.write()
    }

    /// The same exclusion, waited for without parking a runtime worker.
    ///
    /// The returned guard is the ordinary [`AuthorityWrite`]: the same gate, the same
    /// poisoning behaviour, the same fault attribution to this session. What changes is
    /// only *where the waiting happens* — a waiter that cannot take the write side yet
    /// yields to the runtime, so a live same-session authority section cannot stop the
    /// executor from serving a timer, the heartbeat or another session's request.
    ///
    /// Once the guard is held the caller must not wait synchronously for anything else:
    /// it only submits the transaction it is about to commit and awaits that job's
    /// receipt, which the writer thread produces.
    pub(crate) async fn lock_permission_update_awaitable(
        &self,
        session_id: aura_domain_common::SessionId,
    ) -> Result<AuthorityWrite, PersistenceError> {
        let gate = self.inner.permission_gates.of(session_id);
        gate.authority.write_async().await
    }

    /// One session's owned start lease, held across an await by a real start.
    pub async fn permission_start_lease(
        &self,
        session_id: aura_domain_common::SessionId,
    ) -> tokio::sync::OwnedRwLockReadGuard<()> {
        self.inner.permission_gates.of(session_id).start.read_owned().await
    }

    /// One session's change lease: the write side of that session's start gate.
    pub async fn permission_change_lease(
        &self,
        session_id: aura_domain_common::SessionId,
    ) -> tokio::sync::OwnedRwLockWriteGuard<()> {
        self.inner.permission_gates.of(session_id).start.write_owned().await
    }

    pub(crate) fn try_permission_change_lease(
        &self,
        session_id: aura_domain_common::SessionId,
    ) -> Result<tokio::sync::OwnedRwLockWriteGuard<()>, PersistenceError> {
        self.inner
            .permission_gates
            .of(session_id)
            .start
            .try_write_owned()
            .map_err(|_| PersistenceError::InvalidRequest("permission.launch_in_progress".into()))
    }

    /// Which sessions hold which side of their gate right now.
    ///
    /// An observed fact for the R6 regression and the acceptance driver: it names the
    /// session that really holds a lease instead of leaving it to be inferred from timing.
    #[doc(hidden)]
    #[must_use]
    pub fn permission_gate_facts(&self) -> Vec<(aura_domain_common::SessionId, bool, bool)> {
        self.inner.permission_gates.live()
    }

    /// Regression-only: makes every session share one gate.
    ///
    /// This is exactly the pre-S04-R6 behaviour, injected so the R6 regression can show that the
    /// isolation it measures really comes from binding the gate to the session. It is never
    /// reachable from a product path.
    #[doc(hidden)]
    pub fn force_shared_gate_for_test(&self, shared: bool) {
        self.inner.permission_gates.share.store(shared, std::sync::atomic::Ordering::Release);
    }

    /// Runs one read on a pooled connection, with the internal default budget.
    pub(crate) fn with_reader<T, F>(&self, operation: F) -> Result<T, PersistenceError>
    where
        F: FnOnce(&Connection) -> Result<T, PersistenceError>,
    {
        self.inner.readers.with(operation)
    }

    /// Runs one read on a pooled connection under a caller-supplied total deadline.
    ///
    /// The budget covers acquiring a connection and the operation itself. When no
    /// connection comes free the answer is [`PersistenceError::ReadUnavailable`] - a
    /// real unavailable result, never an empty one.
    pub fn read_within<T, F>(&self, budget: Duration, operation: F) -> Result<T, PersistenceError>
    where
        F: FnOnce(&Connection) -> Result<T, PersistenceError>,
    {
        self.inner.readers.with_deadline(budget, operation)
    }

    /// The same read on the internal default budget.
    ///
    /// This is the entry a caller outside this crate uses when it needs one read on a
    /// pooled connection and has no deadline of its own; `with_reader` remains the
    /// crate-internal form.
    pub fn read<T, F>(&self, operation: F) -> Result<T, PersistenceError>
    where
        F: FnOnce(&Connection) -> Result<T, PersistenceError>,
    {
        self.inner.readers.with(operation)
    }

    /// Installs a hold point on read-connection acquisition.
    ///
    /// Default-off: only a test or the acceptance driver calls this, and the returned
    /// barrier is what lets the caller assert the hold really reached this layer. The
    /// hold applies to the **first** matching arrival only, so a test can park one real
    /// read and still measure what the next caller costs.
    #[doc(hidden)]
    #[cfg(debug_assertions)]
    pub fn arm_read_hold(
        &self,
        point: crate::barrier::Point,
        step_millis: u64,
    ) -> crate::barrier::Barrier {
        self.inner.readers.install_once_barrier(Barrier::arm_once(point, None, step_millis))
    }

    /// Installs a hold point on the writer.
    #[doc(hidden)]
    #[cfg(debug_assertions)]
    pub fn arm_writer_hold(
        &self,
        point: crate::barrier::Point,
        label: Option<&str>,
        step_millis: u64,
    ) -> crate::barrier::Barrier {
        self.inner.writer.install_barrier(Barrier::arm(point, label, step_millis))
    }

    /// The same hold, for the **first** matching job only.
    ///
    /// A writer that keeps parking would spend every later caller's budget, so a hold
    /// meant to observe one write in flight has to stop after that write. This is the
    /// hold point a test should use when it wants to look at what other callers cost
    /// while the writer is busy.
    #[doc(hidden)]
    #[cfg(debug_assertions)]
    pub fn arm_writer_hold_once(
        &self,
        point: crate::barrier::Point,
        label: Option<&str>,
        step_millis: u64,
    ) -> crate::barrier::Barrier {
        self.inner.writer.install_barrier(Barrier::arm_once(point, label, step_millis))
    }

    /// Installs a hold point on admission, before a job is enqueued.
    ///
    /// A caller parked here has **not** been admitted, so nothing of its work has run:
    /// the lane can be filled without the writer thread being occupied at all.
    #[doc(hidden)]
    #[cfg(debug_assertions)]
    pub fn arm_admission_hold(
        &self,
        label: Option<&str>,
        step_millis: u64,
    ) -> crate::barrier::Barrier {
        self.inner.writer.install_admission_barrier(Barrier::arm(
            crate::barrier::Point::QueueAdmission,
            label,
            step_millis,
        ))
    }

    /// The same admission hold, restricted to **one named case**.
    ///
    /// `select` is matched against the arriving caller's own identity - a workspace
    /// catalogue write names its workspace and correlation - so a driver can hold the
    /// invocation it means instead of whichever thread reaches the point first. An arrival
    /// that carries no identity is never selected, and no authorisation, claim or routing
    /// step is skipped to make the selection: it only decides which arrival waits.
    #[doc(hidden)]
    #[cfg(debug_assertions)]
    pub fn arm_admission_hold_selected(
        &self,
        label: Option<&str>,
        select: &str,
        step_millis: u64,
    ) -> crate::barrier::Barrier {
        self.inner.writer.install_admission_barrier(Barrier::arm_selected(
            crate::barrier::Point::QueueAdmission,
            label,
            Some(select),
            step_millis,
        ))
    }

    /// Closes the writer for admission and waits a bounded time for it to stop.
    ///
    /// This is what a test calls to prove the teardown story: the returned health says
    /// whether the thread was joined, whether a job was still running when the budget
    /// expired, and how many times a close was requested.
    #[doc(hidden)]
    pub fn close_writer(&self, budget: Duration) -> WriterHealth {
        self.inner.writer.shutdown(budget)
    }

    pub(crate) fn artifact_root(&self) -> &std::path::Path {
        &self.inner.artifact_root
    }

    pub(crate) fn publish_workspace_session(
        &self,
        projection: CommittedProjection<WorkspaceSessionProjection>,
    ) -> Result<(), PersistenceError> {
        let mut subscribers = self
            .inner
            .workspace_session_subscribers
            .lock()
            .map_err(|_| PersistenceError::SubscriptionLockPoisoned)?;
        subscribers.retain(|subscriber| subscriber.send(projection.clone()).is_ok());
        Ok(())
    }

    pub(crate) fn publish_security(
        &self,
        projection: CommittedProjection<SecurityProjectionPayload>,
    ) -> Result<(), PersistenceError> {
        let mut subscribers = self
            .inner
            .security_subscribers
            .lock()
            .map_err(|_| PersistenceError::SubscriptionLockPoisoned)?;
        subscribers.retain(|subscriber| subscriber.send(projection.clone()).is_ok());
        Ok(())
    }
}

fn register_runtime(
    connection: &mut Connection,
    runtime_instance_id: RuntimeInstanceId,
    process_kind: RuntimeProcessKind,
) -> Result<(), PersistenceError> {
    let transaction = connection.transaction()?;
    let now = UtcTimestamp::now().to_string();
    if process_kind == RuntimeProcessKind::Core {
        transaction.execute(
            "UPDATE runtime_leases SET state='expired'
             WHERE state='active' AND runtime_instance_id <> ?1",
            [runtime_instance_id.to_string()],
        )?;
        transaction.execute(
            "UPDATE runtime_instances SET ended_at=?1
             WHERE ended_at IS NULL AND runtime_instance_id <> ?2",
            params![now, runtime_instance_id.to_string()],
        )?;
    }
    let process_kind = runtime_process_kind_name(process_kind);
    transaction.execute(
        "INSERT INTO runtime_instances(runtime_instance_id, process_kind, started_at, ended_at)
         VALUES (?1, ?2, ?3, NULL)
         ON CONFLICT(runtime_instance_id) DO UPDATE SET
            process_kind=excluded.process_kind, started_at=excluded.started_at, ended_at=NULL",
        params![runtime_instance_id.to_string(), process_kind, UtcTimestamp::now().to_string()],
    )?;
    transaction.commit()?;
    Ok(())
}

const fn runtime_process_kind_name(kind: RuntimeProcessKind) -> &'static str {
    match kind {
        RuntimeProcessKind::Launcher => "launcher",
        RuntimeProcessKind::ElectronMain => "electron_main",
        RuntimeProcessKind::Renderer => "renderer",
        RuntimeProcessKind::Core => "core",
        RuntimeProcessKind::SandboxHost => "sandbox_host",
        RuntimeProcessKind::EgressProxy => "egress_proxy",
        RuntimeProcessKind::ElevatedHelper => "elevated_helper",
    }
}

fn validate_workspace_session(
    workspace: &WorkspaceSummary,
    session: &SessionSummary,
) -> Result<(), PersistenceError> {
    if workspace.workspace_id != session.workspace_id {
        return Err(PersistenceError::InvalidRequest(
            "session workspace_id does not match its workspace".to_owned(),
        ));
    }
    if workspace.display_name.trim().is_empty()
        || workspace.selected_root.trim().is_empty()
        || workspace.canonical_root.trim().is_empty()
        || session.title.trim().is_empty()
        || session.state.trim().is_empty()
    {
        return Err(PersistenceError::InvalidRequest(
            "workspace and session projection fields must not be empty".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn load_workspace_session_projection(
    connection: &Connection,
) -> Result<WorkspaceSessionProjection, PersistenceError> {
    let mut workspace_statement = connection.prepare(
        "SELECT workspace_id, display_name, selected_root, canonical_root,
                root_volume_serial, root_file_id, state, revision,
                catalog_freshness, authorization_epoch
         FROM workspaces ORDER BY created_at, workspace_id",
    )?;
    let workspace_rows = workspace_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, i64>(9)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut workspaces = Vec::with_capacity(workspace_rows.len());
    for (
        workspace_id,
        display_name,
        selected_root,
        canonical_root,
        volume_serial_number,
        file_id,
        state,
        revision,
        catalog_freshness,
        authorization_epoch,
    ) in workspace_rows
    {
        workspaces.push(WorkspaceSummary {
            workspace_id: WorkspaceId::from_str(&workspace_id)?,
            display_name,
            selected_root,
            canonical_root,
            root_identity: match (volume_serial_number, file_id) {
                (Some(volume_serial_number), Some(file_id)) => {
                    Some(FileIdentity { volume_serial_number, file_id })
                }
                (None, None) => None,
                _ => {
                    return Err(PersistenceError::InvalidRequest(
                        "persisted workspace root identity is incomplete".to_owned(),
                    ));
                }
            },
            state: parse_workspace_state(&state)?,
            revision: Revision::new(u64::try_from(revision).map_err(|_| {
                PersistenceError::InvalidRequest("negative workspace revision".to_owned())
            })?),
            catalog_freshness: parse_catalog_freshness(&catalog_freshness)?,
            authorization_epoch: Revision::new(u64::try_from(authorization_epoch).map_err(
                |_| PersistenceError::InvalidRequest("negative authorization epoch".to_owned()),
            )?),
        });
    }

    let mut session_statement = connection.prepare(
        "SELECT session_id, workspace_id, title, state
         FROM sessions WHERE state<>'deleting' ORDER BY created_at, session_id",
    )?;
    let session_rows = session_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut sessions = Vec::with_capacity(session_rows.len());
    for (session_id, workspace_id, title, state) in session_rows {
        sessions.push(SessionSummary {
            session_id: SessionId::from_str(&session_id)?,
            workspace_id: WorkspaceId::from_str(&workspace_id)?,
            title,
            state,
        });
    }
    Ok(WorkspaceSessionProjection { workspaces, sessions })
}

pub(crate) fn insert_event(
    transaction: &Transaction<'_>,
    event: &PersistentEventDraft,
) -> Result<i64, PersistenceError> {
    let session_sequence = if let Some(session_id) = event.session_id {
        Some(transaction.query_row(
            "SELECT COALESCE(MAX(session_sequence), 0) + 1
             FROM events WHERE session_id=?1",
            [session_id.to_string()],
            |row| row.get::<_, i64>(0),
        )?)
    } else {
        None
    };
    let actor = match event.actor {
        EventActor::User => "user",
        EventActor::ParentAgent => "parent_agent",
        EventActor::ChildAgent => "child_agent",
        EventActor::SystemRuntime => "system_runtime",
    };
    transaction.execute(
        "INSERT INTO events(
            event_id, session_sequence, workspace_id, session_id, turn_task_id,
            agent_run_id, event_type, event_version, actor, correlation_id,
            causation_id, atomic_group_id, payload_json, payload_artifact_ref, created_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15
         )",
        params![
            event.event_id.to_string(),
            session_sequence,
            event.workspace_id.map(|id| id.to_string()),
            event.session_id.map(|id| id.to_string()),
            event.turn_task_id.map(|id| id.to_string()),
            event.agent_run_id.map(|id| id.to_string()),
            event.event_type,
            i64::from(event.event_version),
            actor,
            event.correlation_id.to_string(),
            event.causation_id.map(|id| id.to_string()),
            event.atomic_group_id,
            event.payload_json.as_ref().map(serde_json::to_string).transpose()?,
            event.payload_artifact_ref.map(|id| id.to_string()),
            event.created_at.to_string()
        ],
    )?;
    if let Some(searchable_text) = event.searchable_text.as_ref() {
        transaction.execute(
            "INSERT INTO event_search_fts(event_id, session_id, searchable_text)
             VALUES (?1, ?2, ?3)",
            params![
                event.event_id.to_string(),
                event.session_id.map(|id| id.to_string()),
                searchable_text
            ],
        )?;
    }
    Ok(transaction.last_insert_rowid())
}

pub(crate) fn next_projection_revision(
    transaction: &Transaction<'_>,
    kind: &str,
    key: &str,
) -> Result<u64, PersistenceError> {
    transaction.execute(
        "INSERT INTO projection_revisions(projection_kind, projection_key, revision)
         VALUES (?1, ?2, 1)
         ON CONFLICT(projection_kind, projection_key)
         DO UPDATE SET revision=revision+1",
        params![kind, key],
    )?;
    let revision = transaction.query_row(
        "SELECT revision FROM projection_revisions
         WHERE projection_kind=?1 AND projection_key=?2",
        params![kind, key],
        |row| row.get::<_, i64>(0),
    )?;
    u64::try_from(revision)
        .map_err(|_| PersistenceError::InvalidRequest("negative projection revision".to_owned()))
}

pub(crate) fn to_sql_u64(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| {
        PersistenceError::InvalidRequest("revision exceeds SQLite integer range".to_owned())
    })
}

pub(crate) const fn workspace_state_name(state: WorkspaceState) -> &'static str {
    match state {
        WorkspaceState::Online => "online",
        WorkspaceState::Offline => "offline",
        WorkspaceState::Archived => "archived",
        WorkspaceState::ReconciliationRequired => "reconciliation_required",
    }
}

pub(crate) fn parse_workspace_state(value: &str) -> Result<WorkspaceState, PersistenceError> {
    match value {
        "online" => Ok(WorkspaceState::Online),
        "offline" => Ok(WorkspaceState::Offline),
        "archived" => Ok(WorkspaceState::Archived),
        "reconciliation_required" => Ok(WorkspaceState::ReconciliationRequired),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "unknown persisted workspace state: {value}"
        ))),
    }
}

pub(crate) const fn catalog_freshness_name(value: CatalogFreshness) -> &'static str {
    match value {
        CatalogFreshness::Unavailable => "unavailable",
        CatalogFreshness::Rebuilding => "rebuilding",
        CatalogFreshness::Fresh => "fresh",
        CatalogFreshness::Stale => "stale",
    }
}

pub(crate) fn parse_catalog_freshness(value: &str) -> Result<CatalogFreshness, PersistenceError> {
    match value {
        "unavailable" => Ok(CatalogFreshness::Unavailable),
        "rebuilding" => Ok(CatalogFreshness::Rebuilding),
        "fresh" => Ok(CatalogFreshness::Fresh),
        "stale" => Ok(CatalogFreshness::Stale),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "unknown persisted catalog freshness: {value}"
        ))),
    }
}
