mod editor;
use std::{collections::BTreeMap, sync::Arc, time::Duration, time::Instant};

use anyhow::{Context, Result};
use aura_config::{
    ConfigModule, ConfigValidationError, ModuleConfigDocument, ModuleConfigValidator,
    SettingsFacade,
};
use aura_domain_common::{CorrelationId, ErrorCategory, ErrorCode};
use aura_fs_tools::{FsToolError, FsToolPolicy, FsToolService};
use aura_ipc::method;
use aura_persistence::{PersistenceCoordinator, PersistenceError};
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use aura_workspace::{CatalogOptions, WorkspaceError, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::WorkspaceRestoreCommand;
use aura_workspace_domain::{
    BeginWorkspaceLifecycleInput, BufferEditCommand, BufferOpenCommand, BufferSaveCommand,
    EditorLifecycleTransitionAck, FsApplyChangesetRequest, FsApplyPatchRequest, FsCopyRequest,
    FsCreateDirectoryRequest, FsDeleteRequest, FsFindPathsRequest, FsGlobRequest, FsHashRequest,
    FsInspectRequest, FsListRequest, FsMoveRequest, FsReadBinaryRangeRequest, FsReadTextRequest,
    FsSearchTextRequest, FsStatRequest, FsWriteFileRequest, IssueDiscardConfirmationInput,
    LargeFileReadMode, RendererLifecycleUnavailableInput, ResolveWorkspaceLifecycleInput,
    WorkspaceLifecycleOperationQuery, WorkspaceLifecyclePrepareDeliveredInput,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

const WORKSPACE_FILES_CONFIG_KEY: &str = "workspace_files";
#[cfg(test)]
#[path = "p03_resource_wait_tests.rs"]
mod resource_wait_tests;
pub(crate) const FILE_RESOURCE_BUSY_CODE: &str = "fs.resource_busy";

/// The external destination's live-short-commit code, owned by the P04 publisher.
///
/// Kept here as the one place the dispatcher asks "is this a live refusal I may wait on", so the two
/// entries cannot drift apart: a code added for one path is recognised by the same predicate as the
/// other, and no other error can accidentally become "waitable".
fn is_live_resource_refusal(code: &str) -> bool {
    code == FILE_RESOURCE_BUSY_CODE || code == crate::p04::EXTERNAL_RESOURCE_BUSY_CODE
}

/// The answer a request gets when its cancellation arrived while it was waiting for a resource.
///
/// It is a **cancellation**, not a conflict: the request did not fail to get its resource, it was
/// told to stop. Reporting it as a resource error would invite a caller to retry the very thing it
/// cancelled, and reporting it as a failure would hide that the request never ran at all. The
/// terminal is `Cancelled`, so the ledger's own settlement records that the intent was honoured
/// rather than ignored.
pub(crate) fn cancelled_envelope(correlation_id: CorrelationId) -> IpcErrorEnvelope {
    let mut envelope = envelope(
        "fs.search_cancelled",
        ErrorCategory::Cancelled,
        "the request was cancelled while it waited for a file resource; nothing was written and \
         the request never started"
            .to_owned(),
        false,
        correlation_id,
    );
    envelope.terminal = IpcRequestTerminal::Cancelled;
    envelope
}
pub(crate) type MutationRecheck = Arc<dyn Fn() -> Result<(), IpcErrorEnvelope> + Send + Sync>;

#[derive(Clone)]
struct DispatchOptions {
    retain: bool,
    recheck: Option<MutationRecheck>,
    observer: Option<aura_fs_tools::FileExecutionObserver>,
}

fn elapsed_ms(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub struct P03Services {
    pub fs: Arc<FsToolService>,
    pub workspace: Arc<WorkspaceRuntime>,
    journal: Arc<crate::broker::BrokerJournal>,
    /// Bounded admission for the synchronous calls an async handler has to make, so a
    /// slow catalog walk occupies a pool worker instead of a Tokio reactor thread. See
    /// [`crate::blocking_pool`].
    blocking: crate::blocking_pool::BlockingPool,
}

/// What one bounded P03 dispatch ended as.
///
/// The three cases are kept apart on purpose: a completed call carries the real result
/// of the real handler, a refusal means no handler ran at all, and `Unknown` means the
/// handler may still be running and may still have an effect.
#[derive(Debug)]
pub enum P03Dispatch {
    /// The real handler ran. The value is exactly what the synchronous entry returns,
    /// including an unavailable route.
    Completed(Result<Value, IpcErrorEnvelope>),
    /// No blocking worker was free inside the budget. The handler did **not** run.
    Refused { waited_ms: u64 },
    /// The handler started and did not answer inside the budget. It may still be
    /// running and may still have an effect.
    Unknown { waited_ms: u64 },
}

impl P03Dispatch {
    /// What a bounded dispatch that did not complete looks like at a route.
    ///
    /// Both non-completions are `Unavailable`, and they are deliberately different codes:
    /// a refusal means no worker was free and the handler **did not run**, so a caller may
    /// retry; an unknown answer means the handler started and may still have an effect, so
    /// the caller must query the original Operation instead of resending it. Keeping this
    /// in one place is what makes the file route and the workspace route agree on the
    /// classification.
    pub(crate) fn into_route_result(
        self,
        correlation_id: CorrelationId,
        command_invocation_id: Option<aura_domain_common::CommandInvocationId>,
    ) -> Result<Value, IpcErrorEnvelope> {
        match self {
            Self::Completed(result) => result,
            Self::Refused { waited_ms } => Err(envelope(
                "persistence.busy",
                ErrorCategory::Unavailable,
                format!(
                    "the workspace service had no free worker after {waited_ms} ms; the command was not run"
                ),
                true,
                correlation_id,
            )),
            // The handler started and did not answer. The answer keeps the existing contract's
            // recovery field and the original invocation so a reader can tell it from a definite
            // failure without reading the message, and it deliberately no longer claims the
            // CHANNEL was lost: the connection was never the thing that failed.
            Self::Unknown { waited_ms } => {
                Err(crate::command_settlement::outcome_unknown_envelope(
                    correlation_id,
                    command_invocation_id,
                    format!(
                        "the workspace command did not answer inside its budget after {waited_ms} ms; its outcome is unknown and it must not be retried automatically"
                    ),
                ))
            }
        }
    }
}

impl P03Services {
    pub(crate) fn publication_pool(&self) -> crate::blocking_pool::BlockingPool {
        self.blocking.clone()
    }
    #[must_use]
    pub fn handles_main_command(method_name: &str) -> bool {
        is_p03_main_command(method_name)
    }

    /// The bounded blocking pool's counters, for the isolation tests and the acceptance
    /// driver that has to prove a refusal really happened. No product path reads it.
    #[must_use]
    #[allow(dead_code)]
    pub fn blocking_counters(&self) -> crate::blocking_pool::BlockingPoolCounters {
        self.blocking.counters()
    }

    /// The bounded blocking pool's fixed worker count, so a refusal can name the limit it
    /// hit instead of only saying that it hit one.
    #[must_use]
    #[allow(dead_code)]
    pub fn blocking_capacity(&self) -> usize {
        self.blocking.capacity()
    }

    /// The async production entry for a P03 Main command.
    ///
    /// `dispatch_main_command` stays synchronous because the work under it is, and it
    /// remains the entry a synchronous caller uses. This one moves that same work to a
    /// bounded blocking pool: the call is not re-implemented, re-authorized or
    /// re-routed, it just runs somewhere that is allowed to block. Admission is bounded
    /// by the caller's `deadline`, and a deadline that expires before admission is a
    /// real "nothing ran" answer.
    pub async fn dispatch_main_command_bounded(
        self: &Arc<Self>,
        method_name: &str,
        params: &Value,
        correlation_id: CorrelationId,
        deadline: Instant,
    ) -> P03Dispatch {
        self.dispatch_async(method_name, params, correlation_id, deadline, false).await
    }

    /// An independently admitted completion task owns the handler's eventual value. Admission
    /// keeps the same deadline and four-worker limit; its caller applies its own timed wait.
    pub(crate) async fn dispatch_main_command_owned(
        self: &Arc<Self>,
        method_name: &str,
        params: &Value,
        correlation_id: CorrelationId,
        deadline: Instant,
        observer: Option<aura_fs_tools::FileExecutionObserver>,
        recheck: MutationRecheck,
    ) -> P03Dispatch {
        self.dispatch_async_rechecking(
            method_name,
            params,
            correlation_id,
            deadline,
            DispatchOptions { retain: true, recheck: Some(recheck), observer },
        )
        .await
    }

    async fn dispatch_async(
        self: &Arc<Self>,
        method_name: &str,
        params: &Value,
        correlation_id: CorrelationId,
        deadline: Instant,
        retain: bool,
    ) -> P03Dispatch {
        self.dispatch_async_rechecking(
            method_name,
            params,
            correlation_id,
            deadline,
            DispatchOptions { retain, recheck: None, observer: None },
        )
        .await
    }

    async fn dispatch_async_rechecking(
        self: &Arc<Self>,
        method_name: &str,
        params: &Value,
        correlation_id: CorrelationId,
        deadline: Instant,
        options: DispatchOptions,
    ) -> P03Dispatch {
        let resource_deadline = deadline.min(Instant::now() + self.fs.mutation_wait_budget());
        // **The wait is reported to the connection as a wait, not as execution.**
        //
        // The task-local carries the ticket this request's handler was started with. Reporting
        // through it is what lets the read loop free this request's execution slot while it is
        // parked in the admission below — the separation that stops related waiters from exhausting
        // the connection. A caller outside a handler (a component test, a direct service call) has no
        // ticket and simply does not report, which is the honest answer.
        let waits = crate::ipc_session::current_resource_wait_handler();
        struct CancelCleanup {
            id: CorrelationId,
            retired: std::sync::atomic::AtomicBool,
        }
        impl Drop for CancelCleanup {
            fn drop(&mut self) {
                aura_fs_tools::clear_resource_wait_cancel(self.id);
            }
        }
        struct CancelOwner(Arc<CancelCleanup>);
        impl Drop for CancelOwner {
            fn drop(&mut self) {
                self.0.retired.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        let cancellation = Arc::new(CancelCleanup {
            id: correlation_id,
            retired: std::sync::atomic::AtomicBool::new(false),
        });
        let _owner = CancelOwner(Arc::clone(&cancellation));
        let mut waited = false;
        loop {
            if waited && aura_fs_tools::resource_wait_cancelled(correlation_id) {
                return P03Dispatch::Completed(Err(cancelled_envelope(correlation_id)));
            }
            let mut attempt_options = options.clone();
            if waited {
                let authority = attempt_options.recheck.take();
                let cancellation = Arc::clone(&cancellation);
                attempt_options.recheck = Some(Arc::new(move || {
                    // An admitted blocking closure can outlive its caller. Its shared flag
                    // survives dropping that caller, and its cancellation mark is only cleared
                    // when this closure also retires, before a queued retry could touch a file.
                    if cancellation.retired.load(std::sync::atomic::Ordering::Acquire)
                        || aura_fs_tools::resource_wait_cancelled(correlation_id)
                    {
                        return Err(cancelled_envelope(correlation_id));
                    }
                    let result = authority.as_ref().map_or(Ok(()), |check| check());
                    if cancellation.retired.load(std::sync::atomic::Ordering::Acquire)
                        || aura_fs_tools::resource_wait_cancelled(correlation_id)
                    {
                        return Err(cancelled_envelope(correlation_id));
                    }
                    result
                }));
            }
            let outcome = self
                .dispatch_once(method_name, params, correlation_id, deadline, attempt_options)
                .await;
            // **Both live-short-commit codes are waits; nothing else is.**
            //
            // `fs.resource_busy` is the in-workspace file resource refusal and
            // `file.external_resource_busy` is the external destination's — the same fact about a
            // different entry. Every other code, including `file.version_conflict` and
            // `file.external_resource_held`, is a real answer that waiting cannot improve.
            if !matches!(&outcome, P03Dispatch::Completed(Err(error))
                if is_live_resource_refusal(error.code.as_str()))
            {
                return outcome;
            }
            if let Some(waits) = &waits {
                waits.entered(resource_deadline);
            }
            waited = true;
            // **A cancellation recorded while this request waited stops it from starting.**
            //
            // Nothing has been written or journalled at this point — that is what made the attempt
            // refuse — so honouring the intent costs nothing and is exactly the fact the requirement
            // asks for: after a cancellation, releasing the holder must not produce a late write.
            // The check is here *and* immediately before the next attempt, so an intent that arrives
            // in the gap between them cannot be missed.
            if aura_fs_tools::resource_wait_cancelled(correlation_id) {
                return P03Dispatch::Completed(Err(cancelled_envelope(correlation_id)));
            }
            let remaining = resource_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return outcome;
            }
            // The refused attempt started no journal or file effect and has relinquished its
            // blocking worker. Awaiting here is cancelled when this owning future is dropped.
            // Re-entering the real handler checks the original expected state and live authority.
            tokio::time::sleep(remaining.min(std::time::Duration::from_millis(20))).await;
            if aura_fs_tools::resource_wait_cancelled(correlation_id) {
                return P03Dispatch::Completed(Err(cancelled_envelope(correlation_id)));
            }
            if Instant::now() >= resource_deadline {
                return outcome;
            }
            if let Some(waits) = &waits {
                let granted = tokio::select! {
                    granted = waits.resume(resource_deadline) => granted,
                    _ = async {
                        while !aura_fs_tools::resource_wait_cancelled(correlation_id) {
                            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        }
                    } => return P03Dispatch::Completed(Err(cancelled_envelope(correlation_id))),
                };
                if !granted {
                    return outcome;
                }
            }
        }
    }

    async fn dispatch_once(
        self: &Arc<Self>,
        method_name: &str,
        params: &Value,
        correlation_id: CorrelationId,
        deadline: Instant,
        options: DispatchOptions,
    ) -> P03Dispatch {
        let method = method_name.to_owned();
        let params = params.clone();
        // The service handle is cloned, never rebuilt: the handler that runs on the
        // blocking thread is this same service with its same persistence, catalog and
        // resolver, so nothing about authorization or routing is re-created here.
        let services = Arc::clone(self);
        let recheck = options.recheck.clone();
        let activity = options.observer.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let admission = self
            .blocking
            .run_settled(
                deadline,
                move || {
                    if let Some(recheck) = recheck
                        && let Err(error) = recheck()
                    {
                        return Some(Err(error));
                    }
                    aura_fs_tools::with_file_execution_observer(activity, || {
                        let _read_activity = if method.starts_with("fs.")
                            && !crate::P04Services::gates_authenticated_main_workspace_mutation(
                                &method,
                            ) {
                            aura_fs_tools::observe_file_execution(&[])
                        } else {
                            None
                        };
                        services.dispatch_main_command(&method, &params, correlation_id)
                    })
                },
                move |value: &Option<Result<Value, IpcErrorEnvelope>>| {
                    let _ = sender.send(value.clone());
                },
            )
            .await;
        let admission = if options.retain
            && matches!(admission, crate::blocking_pool::BlockingAdmission::Unknown { .. })
        {
            match receiver.await {
                Ok(value) => crate::blocking_pool::BlockingAdmission::Completed(value),
                Err(_) => admission,
            }
        } else {
            admission
        };
        match admission {
            crate::blocking_pool::BlockingAdmission::Completed(result) => {
                P03Dispatch::Completed(result.unwrap_or_else(|| {
                    Err(workspace_error(WorkspaceError::CatalogUnavailable, correlation_id))
                }))
            }
            crate::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
                crate::append_core_log(
                    "workspace_command_refused",
                    serde_json::json!({
                        "method": method_name,
                        "waited_ms": waited_ms,
                        "blocking_workers": self.blocking.capacity(),
                        "available": self.blocking.available(),
                        "counters": {
                            "admitted": self.blocking.counters().admitted,
                            "waited": self.blocking.counters().waited,
                            "refused": self.blocking.counters().refused,
                            "unknown": self.blocking.counters().unknown,
                            "completed": self.blocking.counters().completed,
                        },
                    }),
                );
                P03Dispatch::Refused { waited_ms }
            }
            crate::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
                P03Dispatch::Unknown { waited_ms }
            }
        }
    }

    pub fn open(
        persistence: Arc<PersistenceCoordinator>,
        journal: Arc<crate::broker::BrokerJournal>,
        settings: &SettingsFacade,
        activity_index: aura_activity_runtime::ActivityIndex,
        activity_observer: aura_activity_runtime::ActivityObserverHandle,
        runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    ) -> Result<Self> {
        let started = std::time::Instant::now();
        // This used to `?` on the Tools snapshot, so an unreadable
        // `tools.json` aborted `P03Services::open`, which aborted
        // `initialize_services_at`, which took the whole Core runtime down —
        // sessions, status, stop and repair with it. `classify_startup` turned
        // that into `RecoveryRequired`, so one damaged configuration domain
        // disabled the entire product.
        //
        // The honest reading is narrower: what is lost is P03's **Workspace Files
        // settings** — the catalog, read and watcher bounds that come from
        // `tools.json`. Those are genuinely needed to size the workspace runtime,
        // so this capability is refused with its real reason, and Core still
        // starts so every unrelated capability and every control path survives.
        //
        // No limit is invented: `P03Services` is simply not constructed, and the
        // caller records which capability is gone. A default policy substituted
        // here would be the same fabrication class as the tool-settings default
        // this round removed from `lib.rs`.
        let config = WorkspaceFilesConfig::from_settings(settings)?;
        let config_ms = elapsed_ms(started);
        let started = std::time::Instant::now();
        let workspace = WorkspaceRuntime::open_with_activity_publisher(
            Arc::clone(&persistence),
            WorkspaceRuntimeOptions {
                catalog: CatalogOptions {
                    max_partition_files: config.catalog.max_partition_files,
                    max_partition_text_bytes: config.catalog.max_partition_text_bytes,
                    max_index_file_bytes: config.catalog.max_index_file_bytes,
                },
                watcher_coalesce: Duration::from_millis(config.watcher.coalesce_milliseconds),
                watcher_poll_interval: Duration::from_millis(
                    config.watcher.poll_interval_milliseconds,
                ),
                watcher_rescan_attempts: config.watcher.rescan_attempts,
                ..WorkspaceRuntimeOptions::default()
            },
            // The buffer store publishes its **real commits** through this hook: once
            // before its write window opens and once when that window really settled or failed.
            // The hook is a plain closure over the process's one index, so the store gains no
            // dependency on the information module and a broken index still cannot affect a save:
            // the return value is discarded and the closure runs outside every store gate.
            Some(std::sync::Arc::new(move |fact: aura_workspace::BufferCommitFact<'_>| {
                crate::agent_activity::publish_buffer_commit_activity(
                    &fact,
                    runtime_instance_id,
                    &activity_index,
                    &activity_observer,
                );
            })),
        )
        .context("failed to initialize Workspace runtime")?;
        let workspace_ms = elapsed_ms(started);
        let started = std::time::Instant::now();
        let fs = FsToolService::new(
            Arc::clone(&workspace),
            persistence,
            FsToolPolicy {
                large_file_read_mode: config.read.large_file_read_mode,
                full_text_read_limit_bytes: config.read.full_text_read_limit_bytes,
                small_range_line_limit: config.read.small_range_line_limit,
                small_range_character_limit: config.read.small_range_character_limit,
                default_binary_range_bytes: config.read.default_binary_range_bytes,
                max_binary_range_bytes: config.read.max_binary_range_bytes,
                max_changeset_operations: config.limits.max_changeset_operations,
                max_search_results: config.limits.max_search_results,
                mutation_wait: std::time::Duration::from_secs(5),
            },
        )
        .context("failed to initialize fs.* tools")?;
        // Sub-stage attribution for the startup stage this open dominates: the
        // whole block used to be one opaque "p03_services_open" boundary, so a
        // slow cold open could not be told apart from a slow catalog load.
        crate::append_core_log(
            "p03_services_open_detail",
            serde_json::json!({
                "config_ms": config_ms,
                "workspace_open_ms": workspace_ms,
                "fs_tools_ms": elapsed_ms(started),
                "workspace_open": workspace
                    .open_report()
                    .and_then(|report| serde_json::to_value(report).ok()),
            }),
        );
        Ok(Self { fs, workspace, journal, blocking: crate::blocking_pool::BlockingPool::default() })
    }

    pub fn dispatch_main_command(
        &self,
        method_name: &str,
        params: &Value,
        correlation_id: CorrelationId,
    ) -> Option<Result<Value, IpcErrorEnvelope>> {
        Self::handles_main_command(method_name)
            .then(|| self.dispatch_known_main_command(method_name, params, correlation_id))
    }

    pub(crate) fn dispatch_known_main_command(
        &self,
        method_name: &str,
        params: &Value,
        correlation_id: CorrelationId,
    ) -> Result<Value, IpcErrorEnvelope> {
        match method_name {
            method::WORKSPACE_CREATE => {
                let request = decode(params, correlation_id)?;
                encode(
                    self.workspace
                        .create(request, correlation_id)
                        .map_err(|error| workspace_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::WORKSPACE_RESTORE => {
                let request: WorkspaceRestoreCommand = decode(params, correlation_id)?;
                encode(
                    self.workspace
                        .restore(request.workspace_id, correlation_id)
                        .map_err(|error| workspace_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::WORKSPACE_LIFECYCLE_BEGIN => {
                let request: BeginWorkspaceLifecycleInput = decode(params, correlation_id)?;
                encode(
                    self.workspace
                        .begin_lifecycle(request, correlation_id)
                        .map_err(|error| workspace_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::WORKSPACE_LIFECYCLE_PREPARE => {
                let request: WorkspaceLifecycleOperationQuery = decode(params, correlation_id)?;
                encode(
                    self.workspace
                        .prepare_lifecycle_transition(request.operation_id)
                        .map_err(|error| workspace_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::WORKSPACE_LIFECYCLE_PREPARE_DELIVERED => {
                let request: WorkspaceLifecyclePrepareDeliveredInput =
                    decode(params, correlation_id)?;
                encode(
                    self.workspace
                        .mark_lifecycle_prepare_delivered(request)
                        .map_err(|error| workspace_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::WORKSPACE_LIFECYCLE_ACK => {
                let request: EditorLifecycleTransitionAck = decode(params, correlation_id)?;
                encode(
                    self.workspace
                        .acknowledge_lifecycle(request)
                        .map_err(|error| workspace_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::WORKSPACE_LIFECYCLE_ISSUE_DISCARD_CONFIRMATION => {
                let request: IssueDiscardConfirmationInput = decode(params, correlation_id)?;
                encode(
                    self.workspace
                        .issue_discard_confirmation(request)
                        .map_err(|error| workspace_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::WORKSPACE_LIFECYCLE_RESOLVE => {
                self.resolve_editor_lifecycle(decode(params, correlation_id)?, correlation_id)
            }
            method::WORKSPACE_LIFECYCLE_RENDERER_UNAVAILABLE => {
                let request: RendererLifecycleUnavailableInput = decode(params, correlation_id)?;
                encode(
                    self.workspace
                        .renderer_unavailable(request.renderer_instance_id)
                        .map_err(|error| workspace_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::BUFFER_OPEN => {
                let request: BufferOpenCommand = decode(params, correlation_id)?;
                encode(
                    self.workspace
                        .open_buffer(request)
                        .map_err(|error| workspace_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::BUFFER_EDIT => {
                let request: BufferEditCommand = decode(params, correlation_id)?;
                encode(
                    self.workspace
                        .edit_buffer(request, correlation_id)
                        .map_err(|error| workspace_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::BUFFER_SAVE => {
                self.save_editor_buffer(decode(params, correlation_id)?, correlation_id)
            }
            method::FS_STAT => {
                fs_call(params, correlation_id, |request: FsStatRequest| self.fs.stat(request))
            }
            method::FS_LIST => {
                fs_call(params, correlation_id, |request: FsListRequest| self.fs.list(request))
            }
            method::FS_FIND_PATHS => {
                fs_call(params, correlation_id, |request: FsFindPathsRequest| {
                    self.fs.find_paths(request)
                })
            }
            method::FS_GLOB => {
                fs_call(params, correlation_id, |request: FsGlobRequest| self.fs.glob(request))
            }
            method::FS_READ_TEXT => {
                fs_call(params, correlation_id, |request: FsReadTextRequest| {
                    self.fs.read_text(request)
                })
            }
            method::FS_SEARCH_TEXT => {
                fs_call(params, correlation_id, |request: FsSearchTextRequest| {
                    let cancel = aura_fs_tools::search_flight(correlation_id).unwrap_or_default();
                    self.fs.search_text_with_cancel(request, &cancel).map(|(result, _)| result)
                })
            }
            method::FS_HASH => {
                fs_call(params, correlation_id, |request: FsHashRequest| self.fs.hash(request))
            }
            method::FS_INSPECT => fs_call(params, correlation_id, |request: FsInspectRequest| {
                self.fs.inspect(request)
            }),
            method::FS_READ_BINARY_RANGE => {
                fs_call(params, correlation_id, |request: FsReadBinaryRangeRequest| {
                    self.fs.read_binary_range(request)
                })
            }
            method::FS_WRITE_FILE => {
                fs_call(params, correlation_id, |request: FsWriteFileRequest| {
                    self.fs.write_file(request, correlation_id)
                })
            }
            method::FS_APPLY_PATCH => {
                fs_call(params, correlation_id, |request: FsApplyPatchRequest| {
                    self.fs.apply_patch(request, correlation_id)
                })
            }
            method::FS_CREATE_DIRECTORY => {
                fs_call(params, correlation_id, |request: FsCreateDirectoryRequest| {
                    self.fs.create_directory(request, correlation_id)
                })
            }
            method::FS_COPY => fs_call(params, correlation_id, |request: FsCopyRequest| {
                self.fs.copy(request, correlation_id)
            }),
            method::FS_MOVE => fs_call(params, correlation_id, |request: FsMoveRequest| {
                self.fs.move_path(request, correlation_id)
            }),
            method::FS_DELETE => fs_call(params, correlation_id, |request: FsDeleteRequest| {
                self.fs.delete(request, correlation_id)
            }),
            method::FS_APPLY_CHANGESET => {
                fs_call(params, correlation_id, |request: FsApplyChangesetRequest| {
                    self.fs.apply_changeset(request, correlation_id)
                })
            }
            _ => unreachable!("P03 Electron Main command was checked before dispatch"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct P03ToolsConfigValidator;

impl ModuleConfigValidator for P03ToolsConfigValidator {
    fn validate(
        &self,
        module: ConfigModule,
        document: &ModuleConfigDocument,
    ) -> Result<(), ConfigValidationError> {
        if module != ConfigModule::Tools {
            return Ok(());
        }
        WorkspaceFilesConfig::from_document(document)
            .map(|_| ())
            .map_err(|error| ConfigValidationError::Semantic(error.to_string()))
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct WorkspaceFilesConfig {
    catalog: CatalogConfig,
    limits: LimitsConfig,
    read: ReadConfig,
    watcher: WatcherConfig,
}

impl WorkspaceFilesConfig {
    /// Reads the Workspace Files settings, or reports the real configuration
    /// fault with the domain that caused it.
    ///
    /// The settings are a genuine dependency of the workspace runtime, so
    /// this refuses rather than inventing bounds. What changed is that the
    /// refusal names the domain and the stable fault code, which is what lets the
    /// caller degrade **one capability** instead of the whole runtime.
    fn from_settings(settings: &SettingsFacade) -> Result<Self> {
        let snapshot = settings.snapshot(ConfigModule::Tools).map_err(|error| {
            anyhow::anyhow!(
                "tools.json Workspace Files settings are unavailable ({}, domain {}): {error}",
                crate::config_degradation::config_fault_code(&error),
                error.module().map_or("unknown", ConfigModule::key),
            )
        })?;
        Self::from_document(&snapshot.document)
    }

    fn from_document(document: &ModuleConfigDocument) -> Result<Self> {
        let config = document.values.get(WORKSPACE_FILES_CONFIG_KEY).map_or_else(
            || Ok(Self::default()),
            |value| serde_json::from_value(value.clone()).context("invalid workspace_files"),
        )?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.catalog.max_partition_files > 0,
            "max_partition_files must be positive"
        );
        anyhow::ensure!(
            self.catalog.max_partition_text_bytes > 0,
            "max_partition_text_bytes must be positive"
        );
        anyhow::ensure!(
            self.catalog.max_index_file_bytes > 0,
            "max_index_file_bytes must be positive"
        );
        anyhow::ensure!(
            self.watcher.coalesce_milliseconds > 0,
            "watcher coalesce must be positive"
        );
        anyhow::ensure!(
            self.watcher.poll_interval_milliseconds > 0,
            "watcher poll interval must be positive"
        );
        anyhow::ensure!(
            self.watcher.rescan_attempts > 0,
            "watcher rescan attempts must be positive"
        );
        FsToolPolicy {
            large_file_read_mode: self.read.large_file_read_mode,
            full_text_read_limit_bytes: self.read.full_text_read_limit_bytes,
            small_range_line_limit: self.read.small_range_line_limit,
            small_range_character_limit: self.read.small_range_character_limit,
            default_binary_range_bytes: self.read.default_binary_range_bytes,
            max_binary_range_bytes: self.read.max_binary_range_bytes,
            max_changeset_operations: self.limits.max_changeset_operations,
            max_search_results: self.limits.max_search_results,
            mutation_wait: std::time::Duration::from_secs(5),
        }
        .validate()
        .map_err(anyhow::Error::msg)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CatalogConfig {
    max_partition_files: u64,
    max_partition_text_bytes: u64,
    max_index_file_bytes: u64,
}

impl Default for CatalogConfig {
    fn default() -> Self {
        let defaults = CatalogOptions::default();
        Self {
            max_partition_files: defaults.max_partition_files,
            max_partition_text_bytes: defaults.max_partition_text_bytes,
            max_index_file_bytes: defaults.max_index_file_bytes,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ReadConfig {
    large_file_read_mode: LargeFileReadMode,
    full_text_read_limit_bytes: u64,
    small_range_line_limit: u64,
    small_range_character_limit: u64,
    default_binary_range_bytes: u32,
    max_binary_range_bytes: u32,
}

impl Default for ReadConfig {
    fn default() -> Self {
        let defaults = FsToolPolicy::default();
        Self {
            large_file_read_mode: defaults.large_file_read_mode,
            full_text_read_limit_bytes: defaults.full_text_read_limit_bytes,
            small_range_line_limit: defaults.small_range_line_limit,
            small_range_character_limit: defaults.small_range_character_limit,
            default_binary_range_bytes: defaults.default_binary_range_bytes,
            max_binary_range_bytes: defaults.max_binary_range_bytes,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LimitsConfig {
    max_changeset_operations: usize,
    max_search_results: usize,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        let defaults = FsToolPolicy::default();
        Self {
            max_changeset_operations: defaults.max_changeset_operations,
            max_search_results: defaults.max_search_results,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct WatcherConfig {
    coalesce_milliseconds: u64,
    poll_interval_milliseconds: u64,
    rescan_attempts: u32,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        let defaults = WorkspaceRuntimeOptions::default();
        Self {
            coalesce_milliseconds: u64::try_from(defaults.watcher_coalesce.as_millis())
                .expect("default coalesce fits u64"),
            poll_interval_milliseconds: u64::try_from(defaults.watcher_poll_interval.as_millis())
                .expect("default poll interval fits u64"),
            rescan_attempts: defaults.watcher_rescan_attempts,
        }
    }
}

fn is_p03_main_command(value: &str) -> bool {
    aura_ipc::routes::owned_by(value, aura_ipc::routes::RouteOwner::Workspace)
}

fn fs_call<Request, Response>(
    params: &Value,
    correlation_id: CorrelationId,
    operation: impl FnOnce(Request) -> Result<Response, FsToolError>,
) -> Result<Value, IpcErrorEnvelope>
where
    Request: DeserializeOwned,
    Response: serde::Serialize,
{
    let request = decode(params, correlation_id)?;
    let response = operation(request).map_err(|error| fs_error(error, correlation_id))?;
    encode(response, correlation_id)
}

fn decode<T: DeserializeOwned>(
    params: &Value,
    correlation_id: CorrelationId,
) -> Result<T, IpcErrorEnvelope> {
    serde_json::from_value(params.clone()).map_err(|error| {
        envelope(
            "ipc.invalid_params",
            ErrorCategory::Validation,
            format!("Invalid P03 request parameters: {error}"),
            false,
            correlation_id,
        )
    })
}

fn encode<T: serde::Serialize>(
    value: T,
    correlation_id: CorrelationId,
) -> Result<Value, IpcErrorEnvelope> {
    serde_json::to_value(value).map_err(|error| {
        envelope(
            "ipc.response_encoding_failed",
            ErrorCategory::Internal,
            format!("Failed to encode P03 response: {error}"),
            false,
            correlation_id,
        )
    })
}

fn fs_error(error: FsToolError, correlation_id: CorrelationId) -> IpcErrorEnvelope {
    let message = error.to_string();
    let (code, category, retryable) = match error {
        FsToolError::Workspace(error) => return workspace_error(error, correlation_id),
        FsToolError::Persistence(error) => return persistence_error(error, correlation_id),
        FsToolError::ScopeUnavailable => {
            ("file.scope_unavailable", ErrorCategory::Permission, false)
        }
        FsToolError::InvalidRequest(_) => ("fs.invalid_request", ErrorCategory::Validation, false),
        FsToolError::LargeFileRangeRequired => {
            ("fs.range_required", ErrorCategory::Validation, false)
        }
        FsToolError::TextRangeTooLarge => ("fs.range_too_large", ErrorCategory::Validation, false),
        FsToolError::BinaryRangeTooLarge => {
            ("fs.binary_range_too_large", ErrorCategory::Validation, false)
        }
        FsToolError::ExpectedAbsent { .. }
        | FsToolError::ExpectedDirectory { .. }
        | FsToolError::ExpectedHash { .. } => {
            ("fs.expected_state_conflict", ErrorCategory::Conflict, false)
        }
        // An expected-state conflict means the same thing outside the workspace, so
        // it keeps the same code: the caller has to re-read the file it is about to
        // overwrite rather than retry the same write.
        FsToolError::ExternalExpectedState { .. } => {
            ("fs.expected_state_conflict", ErrorCategory::Conflict, false)
        }
        // The publication was attempted and the previous content is back, so the target is
        // exactly as the caller left it: retrying is a decision the caller can make.
        FsToolError::ExternalPublishRolledBack { .. } => {
            ("fs.external_publish_rolled_back", ErrorCategory::Conflict, true)
        }
        FsToolError::DeferredByEditorActivity => {
            ("fs.deferred_by_editor_activity", ErrorCategory::Conflict, false)
        }
        FsToolError::ResourceBusy(_) => (FILE_RESOURCE_BUSY_CODE, ErrorCategory::Conflict, true),
        FsToolError::InvalidPatch(_) | FsToolError::PatchTargetMismatch { .. } => {
            ("fs.invalid_patch", ErrorCategory::Validation, false)
        }
        FsToolError::ChangesetFailed { .. } => {
            ("fs.changeset_failed", ErrorCategory::Conflict, false)
        }
        FsToolError::ReconciliationRequired(_) => {
            ("workspace.reconciliation_required", ErrorCategory::Conflict, false)
        }
        FsToolError::ProjectionFailed { .. } => {
            ("projection.publish_failed", ErrorCategory::Internal, false)
        }
        FsToolError::Io { .. } => ("fs.io_failed", ErrorCategory::Internal, false),
        // The authorized external file is no longer the file that was verified, so
        // the caller has to re-derive the grant instead of retrying the same read.
        FsToolError::ExternalFileReplaced { .. } => {
            ("fs.external_file_replaced", ErrorCategory::Conflict, false)
        }
        FsToolError::Internal(_) => ("fs.search_failed", ErrorCategory::Internal, false),
        FsToolError::Cancelled => {
            let mut envelope = envelope(
                "fs.search_cancelled",
                ErrorCategory::Cancelled,
                message,
                false,
                correlation_id,
            );
            envelope.terminal = IpcRequestTerminal::Cancelled;
            return envelope;
        }
    };
    envelope(code, category, message, retryable, correlation_id)
}

fn workspace_error(error: WorkspaceError, correlation_id: CorrelationId) -> IpcErrorEnvelope {
    let message = error.to_string();
    let (code, category, retryable) = match error {
        WorkspaceError::Persistence(error) => return persistence_error(error, correlation_id),
        WorkspaceError::InvalidRoot => ("workspace.invalid_root", ErrorCategory::Validation, false),
        WorkspaceError::Offline => ("workspace.offline", ErrorCategory::Unavailable, true),
        WorkspaceError::Archived => ("workspace.archived", ErrorCategory::Conflict, false),
        WorkspaceError::Busy { .. } => ("workspace.busy", ErrorCategory::Conflict, false),
        WorkspaceError::RelocateConfirmationRequired => {
            ("workspace.relocate_confirmation_required", ErrorCategory::Conflict, false)
        }
        WorkspaceError::RootIdentityChanged => {
            ("workspace.root_identity_changed", ErrorCategory::Conflict, false)
        }
        WorkspaceError::ScopeUnavailable { .. } => {
            ("file.scope_unavailable", ErrorCategory::Permission, false)
        }
        WorkspaceError::NotFound { .. } => ("file.not_found", ErrorCategory::NotFound, false),
        WorkspaceError::WrongKind { .. } => ("file.wrong_kind", ErrorCategory::Validation, false),
        WorkspaceError::EditorReserved { .. } => {
            ("workspace.editor_reserved", ErrorCategory::Conflict, false)
        }
        WorkspaceError::BufferRevisionConflict { .. } => {
            ("workspace.buffer_revision_conflict", ErrorCategory::Conflict, false)
        }
        WorkspaceError::FileVersionConflict { .. } => {
            ("file.version_conflict", ErrorCategory::Conflict, false)
        }
        // A live short commit is **retryable on its own budget**: the object may still be exactly
        // what the caller decided about, and the holder is another execution that will finish. Every
        // other workspace conflict keeps its non-retryable reading, because waiting cannot change a
        // stale revision or a stale on-disk version.
        WorkspaceError::FileResourceBusy { .. } => {
            (FILE_RESOURCE_BUSY_CODE, ErrorCategory::Conflict, true)
        }
        WorkspaceError::BufferNotFound => {
            ("workspace.buffer_not_found", ErrorCategory::NotFound, false)
        }
        WorkspaceError::BufferReadOnly => {
            ("workspace.buffer_read_only", ErrorCategory::Conflict, false)
        }
        WorkspaceError::LifecycleTransitionInProgress => {
            ("workspace.lifecycle_in_progress", ErrorCategory::Conflict, false)
        }
        WorkspaceError::LifecycleNotFound => {
            ("workspace.lifecycle_not_found", ErrorCategory::NotFound, false)
        }
        WorkspaceError::LifecycleRevisionConflict { .. } => {
            ("workspace.lifecycle_revision_conflict", ErrorCategory::Conflict, false)
        }
        WorkspaceError::WorkspaceRevisionConflict { .. } => {
            ("workspace.revision_conflict", ErrorCategory::Conflict, false)
        }
        WorkspaceError::LifecycleRendererMismatch { .. } => {
            ("workspace.lifecycle_renderer_mismatch", ErrorCategory::Permission, false)
        }
        WorkspaceError::EditorPrepareAckTimeout => {
            ("workspace.editor_prepare_ack_timeout", ErrorCategory::Unavailable, true)
        }
        WorkspaceError::EditorStateUnconfirmed => {
            ("workspace.editor_state_unconfirmed", ErrorCategory::Conflict, false)
        }
        WorkspaceError::LifecycleEditorBlockersRemain => {
            ("workspace.lifecycle_editor_blockers_remain", ErrorCategory::Conflict, false)
        }
        WorkspaceError::ReservationRevoked { .. } => {
            ("editor.reservation_revoked", ErrorCategory::Conflict, false)
        }
        WorkspaceError::ReservationNotFound { .. } => {
            ("editor.reservation_not_found", ErrorCategory::NotFound, false)
        }
        WorkspaceError::DiscardConfirmationStale => {
            ("workspace.discard_confirmation_stale", ErrorCategory::Conflict, false)
        }
        WorkspaceError::DiscardConfirmationRequired => {
            ("workspace.discard_confirmation_required", ErrorCategory::Conflict, false)
        }
        WorkspaceError::DiscardConfirmationExpired => {
            ("workspace.discard_confirmation_expired", ErrorCategory::Conflict, false)
        }
        WorkspaceError::DiscardConfirmationInvalid => {
            ("workspace.discard_confirmation_invalid", ErrorCategory::Permission, false)
        }
        WorkspaceError::LifecycleTargetChanged => {
            ("workspace.lifecycle_target_changed", ErrorCategory::Conflict, false)
        }
        WorkspaceError::EncodingAmbiguous { .. } => {
            ("file.encoding_ambiguous", ErrorCategory::Validation, false)
        }
        WorkspaceError::CatalogUnavailable | WorkspaceError::IndexBuilding { .. } => {
            ("workspace.catalog_unavailable", ErrorCategory::Unavailable, true)
        }
        // Another index update committed while this one was being planned. Nothing was
        // written and the filesystem facts the caller described are still current, so
        // repeating the same bounded update is the answer; it is not a data error.
        WorkspaceError::CatalogPlanSuperseded { .. } => {
            ("workspace.catalog_superseded", ErrorCategory::Conflict, true)
        }
        WorkspaceError::PathResolutionFailed { .. } => {
            ("workspace.path_resolution_failed", ErrorCategory::Unavailable, false)
        }
        WorkspaceError::Io { .. }
        | WorkspaceError::LockPoisoned(_)
        | WorkspaceError::Watcher(_)
        | WorkspaceError::Randomness(_)
        | WorkspaceError::RevisionExhausted => {
            ("workspace.internal", ErrorCategory::Internal, false)
        }
    };
    let mut envelope = envelope(code, category, message, retryable, correlation_id);
    if code == crate::command_settlement::PERSISTENCE_OUTCOME_UNKNOWN_CODE {
        // "The write was admitted and the writer had not answered" is an unconfirmed outcome, so
        // the answer carries the contract's own recovery instruction: query the original
        // invocation, never resend. The code and the category do not change; what changes is that
        // a reader no longer has to infer the meaning from the code alone, and the Core's own
        // settlement path recognises it as unconfirmed rather than as a definite failure.
        envelope.details.insert(
            crate::command_settlement::OUTCOME_RECOVERY_DETAIL.to_owned(),
            serde_json::json!(crate::command_settlement::OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY),
        );
    }
    envelope
}

fn persistence_error(error: PersistenceError, correlation_id: CorrelationId) -> IpcErrorEnvelope {
    let message = error.to_string();
    let (code, category, retryable) = match error {
        PersistenceError::WorkspaceNotFound => {
            ("workspace.not_found", ErrorCategory::NotFound, false)
        }
        PersistenceError::WorkspaceRootConflict => {
            ("workspace.root_conflict", ErrorCategory::Conflict, false)
        }
        PersistenceError::WorkspaceBusy => ("workspace.busy", ErrorCategory::Conflict, false),
        PersistenceError::MutationOwnedByOtherSession => {
            ("workspace.mutation_owned_by_other_session", ErrorCategory::Conflict, false)
        }
        PersistenceError::ArtifactNotFound => {
            ("artifact.not_found", ErrorCategory::NotFound, false)
        }
        PersistenceError::ArtifactAccessDenied | PersistenceError::QuarantineAccessRequired => {
            ("artifact.access_denied", ErrorCategory::Permission, false)
        }
        PersistenceError::ArtifactRangeTooLarge => {
            ("artifact.range_too_large", ErrorCategory::Validation, false)
        }
        PersistenceError::InvalidRequest(_) => {
            ("persistence.invalid_request", ErrorCategory::Validation, false)
        }
        PersistenceError::WriterUnavailable | PersistenceError::WriterResponseClosed => {
            ("persistence.unavailable", ErrorCategory::Unavailable, true)
        }
        // Admission refused the call: the work never ran, so this is an honest
        // "try again shortly" and not a data error.
        PersistenceError::WriterBusy { .. } => {
            ("persistence.busy", ErrorCategory::Unavailable, true)
        }
        // The deadline expired after the work was admitted or while it was running. The
        // outcome is unknown, so the caller must not be told the write did not happen.
        PersistenceError::WriterUnresponsive { .. } => {
            ("persistence.outcome_unknown", ErrorCategory::Unavailable, false)
        }
        // No read connection was free in the budget. A real unavailable answer: the
        // caller must never read this as "the row is absent".
        PersistenceError::ReadUnavailable { .. } => {
            ("persistence.read_unavailable", ErrorCategory::Unavailable, true)
        }
        // The read had a connection and was stopped at its own total budget. The call is
        // over and nothing outlives it, so this is a definite "no answer inside the
        // budget" - the opposite of the unknown a write's deadline leaves behind - and a
        // fresh read is the way to answer it.
        PersistenceError::ReadDeadlineExceeded { .. } => {
            ("persistence.read_deadline_exceeded", ErrorCategory::Unavailable, true)
        }
        PersistenceError::ArtifactIntegrity => {
            ("artifact.integrity_failed", ErrorCategory::Internal, false)
        }
        PersistenceError::ToolMutationCommandIdempotencyConflict => {
            ("tool.draft.idempotency_conflict", ErrorCategory::Conflict, false)
        }
        PersistenceError::ToolMutationCommandJournalStateConflict
        | PersistenceError::ToolMutationCommandJournalIntegrity => {
            ("tool.draft.journal_failed", ErrorCategory::Conflict, false)
        }
        PersistenceError::Sqlite(_)
        | PersistenceError::Io { .. }
        | PersistenceError::Json(_)
        | PersistenceError::Id(_)
        | PersistenceError::ContentHash(_)
        | PersistenceError::ReadLockPoisoned
        | PersistenceError::SubscriptionLockPoisoned
        | PersistenceError::PermissionStateNotFound
        | PersistenceError::SessionModelBindingNotFound
        | PersistenceError::SessionModelBindingRevisionConflict
        | PersistenceError::CredentialRecordNotFound
        | PersistenceError::CredentialSlotConflict
        | PersistenceError::CredentialRowRevisionConflict
        | PersistenceError::CredentialMutationLeaseConflict
        | PersistenceError::CredentialStateConflict
        | PersistenceError::CredentialJournalNotFound
        | PersistenceError::CredentialJournalStateConflict
        | PersistenceError::MutationJournalNotFound
        | PersistenceError::MutationJournalKindConflict
        | PersistenceError::MutationJournalStateConflict
        | PersistenceError::MutationLeaseConflict
        | PersistenceError::MutationRecoveryHandlerConflict
        | PersistenceError::MutationRecoveryHandlerMissing { .. }
        | PersistenceError::CredentialUnavailable
        | PersistenceError::CredentialBindingMismatch
        | PersistenceError::ModelDiscoveryRunNotFound
        | PersistenceError::ModelDiscoveryRunStateConflict
        | PersistenceError::ModelAvailabilityTestNotFound
        | PersistenceError::ModelAvailabilityTestStateConflict
        | PersistenceError::PermissionRevisionConflict
        | PersistenceError::PermissionSnapshotNotFound
        | PersistenceError::PermissionGrantNotFound
        | PersistenceError::ExternalGrantNotFound
        | PersistenceError::ExternalGrantSessionMismatch
        | PersistenceError::ExternalGrantStateConflict
        | PersistenceError::ApprovalNotFound
        | PersistenceError::ApprovalStale
        | PersistenceError::ApprovalAlreadyResolved
        | PersistenceError::ApprovalDecisionNotAllowed
        | PersistenceError::TrustRecordNotFound
        | PersistenceError::SandboxExecutionNotFound
        | PersistenceError::SandboxJournalNotFound
        | PersistenceError::SandboxStateConflict
        | PersistenceError::HostPreparationRevisionConflict
        | PersistenceError::HostPreparationOperationNotFound
        | PersistenceError::HostPreparationStateConflict
        | PersistenceError::NetworkGrantNotFound
        | PersistenceError::NetworkAuditNotFound
        | PersistenceError::NetworkAuditAlreadyFinished
        | PersistenceError::QuarantineNotFound
        | PersistenceError::QuarantineReleaseNotApproved
        | PersistenceError::ToolRegistryIntegrity
        | PersistenceError::ToolSearchCursorIntegrity
        | PersistenceError::ToolActivityCursorIntegrity
        | PersistenceError::ToolActivityCursorUnavailable
        | PersistenceError::ToolActivityCursorMismatch
        | PersistenceError::ToolActivityCursorExpired
        | PersistenceError::ToolEntityNotFound
        | PersistenceError::ToolEntityRevisionConflict
        | PersistenceError::ToolEntityStateConflict
        | PersistenceError::ToolPackageKeyConflict
        | PersistenceError::ToolPackageVersionContentConflict
        | PersistenceError::ToolPackagePublishEvidenceConflict
        | PersistenceError::ToolPackagePayloadIntegrity
        | PersistenceError::ToolPackageMutationRecoveryRequired
        | PersistenceError::ToolPackageDeleteApprovalInvalid
        | PersistenceError::ToolPackageDeleteImpactChanged
        | PersistenceError::ToolPackageDeleteEvidenceConflict
        | PersistenceError::ToolLongOperationNotCancellable
        | PersistenceError::ToolEntityIntegrity
        | PersistenceError::ToolExecutionRecoveryRequired
        | PersistenceError::ToolSandboxDispatchBindingNotFound
        | PersistenceError::ToolSandboxDispatchBindingMismatch
        | PersistenceError::ToolSandboxDispatchBindingUnavailable
        | PersistenceError::ToolConformanceSandboxDispatchBindingNotFound
        | PersistenceError::ToolConformanceSandboxDispatchBindingMismatch
        | PersistenceError::ToolConformanceSandboxDispatchBindingUnavailable
        | PersistenceError::BuiltinCoreHandlerDispatchBindingNotFound
        | PersistenceError::BuiltinCoreHandlerDispatchBindingMismatch
        | PersistenceError::BuiltinCoreHandlerDispatchBindingUnavailable
        | PersistenceError::HistoricalContextProjectionSnapshot
        | PersistenceError::ContextProjectionIntegrity => {
            ("persistence.internal", ErrorCategory::Internal, false)
        }
    };
    envelope(code, category, message, retryable, correlation_id)
}

fn envelope(
    code: &str,
    category: ErrorCategory,
    message: String,
    retryable: bool,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("static P03 error code is valid"),
        category,
        message,
        retryable,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use aura_config::ModuleConfigDocument;
    use aura_domain_common::{CorrelationId, ErrorCategory};
    use aura_persistence::PersistenceError;
    use serde_json::json;

    use super::{WorkspaceFilesConfig, persistence_error};

    /// A bounded wait and a permanent data error are different answers.
    ///
    /// `persistence.busy` says "the storage layer refused this call and the work did not
    /// run"; `persistence.outcome_unknown` says "it may have run and I have no answer";
    /// `persistence.read_unavailable` says "no read connection was free". None of them may
    /// be reported with a data-error code, because a caller that retries a data error
    /// loops forever and a caller that gives up on a busy error loses real work.
    #[test]
    fn bounded_storage_answers_are_not_data_errors() {
        let correlation = CorrelationId::new();
        let busy = persistence_error(PersistenceError::WriterBusy { class: "index" }, correlation);
        assert_eq!(busy.code.as_str(), "persistence.busy");
        assert_eq!(busy.category, ErrorCategory::Unavailable);
        assert!(busy.retryable, "a refusal before the work ran is retryable");

        let unknown =
            persistence_error(PersistenceError::WriterUnresponsive { class: "index" }, correlation);
        assert_eq!(unknown.code.as_str(), "persistence.outcome_unknown");
        assert_eq!(unknown.category, ErrorCategory::Unavailable);
        assert!(!unknown.retryable, "an unknown outcome must never invite an automatic retry");

        let read =
            persistence_error(PersistenceError::ReadUnavailable { waited_ms: 5 }, correlation);
        assert_eq!(read.code.as_str(), "persistence.read_unavailable");
        assert_eq!(read.category, ErrorCategory::Unavailable);

        // A real data error keeps its own code, so the two cannot be confused.
        let data = persistence_error(PersistenceError::WorkspaceNotFound, correlation);
        assert_eq!(data.code.as_str(), "workspace.not_found");
        assert_eq!(data.category, ErrorCategory::NotFound);
        assert_ne!(data.code, busy.code);
    }

    #[test]
    fn confirmed_p03_defaults_are_stable() {
        let config = WorkspaceFilesConfig::from_document(&ModuleConfigDocument::empty())
            .expect("default P03 tools config");
        assert_eq!(config.read.full_text_read_limit_bytes, 2 * 1024 * 1024);
        assert_eq!(config.catalog.max_index_file_bytes, 1024 * 1024);
        assert_eq!(config.read.small_range_line_limit, 500);
        assert_eq!(config.read.small_range_character_limit, 65_536);
        assert_eq!(config.limits.max_changeset_operations, 64);
        assert_eq!(config.limits.max_search_results, 100);
        assert_eq!(config.watcher.coalesce_milliseconds, 250);
        assert_eq!(config.watcher.poll_interval_milliseconds, 5_000);
        assert_eq!(config.watcher.rescan_attempts, 3);
    }

    #[test]
    fn tools_json_overrides_are_typed_and_unknown_fields_are_rejected() {
        let mut document = ModuleConfigDocument::empty();
        document.values.insert(
            "workspace_files".to_owned(),
            json!({
                "read": { "full_text_read_limit_bytes": 4 * 1024 * 1024 },
                "limits": { "max_search_results": 75 }
            }),
        );
        let config = WorkspaceFilesConfig::from_document(&document).expect("valid override");
        assert_eq!(config.read.full_text_read_limit_bytes, 4 * 1024 * 1024);
        assert_eq!(config.limits.max_search_results, 75);
        assert_eq!(config.watcher.rescan_attempts, 3);

        document.values.insert(
            "workspace_files".to_owned(),
            json!({ "read": { "private_unreviewed_limit": 1 } }),
        );
        assert!(WorkspaceFilesConfig::from_document(&document).is_err());
    }

    #[test]
    fn invalid_zero_watcher_retry_count_is_rejected() {
        let mut document = ModuleConfigDocument::empty();
        document
            .values
            .insert("workspace_files".to_owned(), json!({ "watcher": { "rescan_attempts": 0 } }));
        assert!(WorkspaceFilesConfig::from_document(&document).is_err());
    }
}
