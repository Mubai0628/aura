use super::files::invocation::{FsInvocation, invoke_file_attempt};
use super::*;
use aura_backend_api::BackendFuture;
use aura_backend_core::BrokerPort;
use aura_core_broker::agent_store::{self, StoreAccess, StoreStep};
use aura_domain_common::jcs::canonical_sha256;
use serde_json::{Value, json};

pub fn descriptor(runtime: RuntimeInstanceId) -> BackendDescriptor {
    let mut descriptor = crate::broker::descriptor(runtime);
    descriptor.id = BackendId("agent-core-broker-v1".into());
    descriptor.protocol_fingerprint =
        canonical_sha256(&"aura.agent-core-broker.v1").expect("literal");
    descriptor
        .capabilities
        .extend([ResolvedCapability::AgentTaskRead, ResolvedCapability::AgentTaskWrite]);
    descriptor
}
pub struct Port {
    pub service: Arc<AgentExecution>,
    pub call: CallContext,
    pub commands: Option<Arc<crate::command_execution::CommandExecution>>,
}
impl Port {
    /// The preparation ([`Port::inspect_domain`]).
    fn inspect_sync(&self) -> anyhow::Result<Value> {
        Ok(self.inspect_domain()?.0)
    }

    /// Whether the facts taken now still allow the call prepared with `prepared`: the same facts,
    /// or, for a call on workspace files, different only in what its start also allows - folders
    /// that appeared on a new file's path, the content of a file it reads. Another call of the same
    /// response creates those folders at any moment between this call's preparation and its start.
    fn still_prepared_sync(&self, prepared: &Value) -> anyhow::Result<bool> {
        #[cfg(any(test, feature = "test-hooks"))]
        self.create_folders_due()?;
        let (fresh, workspace_files) = self.inspect_domain()?;
        if fresh == *prepared {
            return Ok(true);
        }
        let (Some(now), Some(then)) = (fresh.as_object(), prepared.as_object()) else {
            return Ok(false);
        };
        let same_frame = now.len() == then.len()
            && now.iter().all(|(key, value)| key == "domain" || then.get(key) == Some(value));
        Ok(same_frame
            && workspace_files
            && files::still_hold(&self.service, &self.call, &then["domain"], &now["domain"])?)
    }

    /// The preparation, and whether its domain facts are the workspace files' ([`files::inspect`]).
    ///
    /// Contract: the `verify_source` call inside takes `journal.lock()` directly, so this
    /// whole synchronous preparation is only correct on a real blocking thread. [`Port::inspect`]
    /// reaches it through one bounded section; [`Port::invoke`]'s fs branch reaches it inside the
    /// pre-existing `tokio::task::spawn_blocking` closure.
    fn inspect_domain(&self) -> anyhow::Result<(Value, bool)> {
        self.service.verify_source(&self.call)?;
        let mode =
            self.service.persistence.session_permission_state(self.call.snapshot.session_id)?;
        if mode.mode != self.call.snapshot.mode
            || mode.revision != self.call.snapshot.session_permission_revision
        {
            anyhow::ensure!(
                is_file_call(&self.call.operation.content.action_kind)
                    && self.service.operation_permitted_now(
                        &self.call.operation,
                        &self.call.snapshot,
                        mode.mode,
                        mode.revision,
                    )?,
                "agent.mode_changed"
            );
        }
        let mut workspace_files = false;
        let domain = if super::package_tools::program::is_program(&self.call.operation.content) {
            super::package_tools::program::inspect(&self.service, &self.call)?
        } else if super::mcp_tools::is_mcp(&self.call.operation.content) {
            super::mcp_tools::inspect(&self.service, &self.call)?
        } else if self.call.operation.content.action_kind.starts_with("memory.") {
            super::memory::inspect(&self.service, &self.call)?
        } else if self.call.operation.content.action_kind.starts_with("git.") {
            super::git::inspect(&self.service, &self.call)?
        } else if self.call.operation.content.action_kind.starts_with("media.") {
            super::media::inspect(&self.service, &self.call)?
        } else if is_file_call(&self.call.operation.content.action_kind) {
            workspace_files = true;
            files::inspect(&self.service, &self.call)?
        } else {
            super::task::inspect(&self.service, &self.call)?
        };
        let preparation = json!({"schema":"agent-broker-preparation-v1",
            "mode":self.call.snapshot.mode,
            "revision":self.call.snapshot.session_permission_revision,"domain":domain});
        Ok((preparation, workspace_files))
    }
}
/// Folders of a session's workspace that a test creates just before one of its calls' prepared
/// facts are taken again - what another write of the same response can do at that moment.
#[cfg(any(test, feature = "test-hooks"))]
pub static CREATE_BEFORE_RECHECK: std::sync::Mutex<Vec<(SessionId, WorkspacePath)>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(any(test, feature = "test-hooks"))]
impl Port {
    fn create_folders_due(&self) -> anyhow::Result<()> {
        let session = self.call.snapshot.session_id;
        let due = {
            let mut folders = CREATE_BEFORE_RECHECK.lock().expect("folders due");
            let (due, rest) = folders.drain(..).partition::<Vec<_>, _>(|(of, _)| *of == session);
            *folders = rest;
            due
        };
        let resolver = self.service.workspace.resolver(self.call.snapshot.workspace_id)?;
        for (_, folder) in due {
            std::fs::create_dir_all(resolver.canonical_root().join(folder.as_str()))?;
        }
        Ok(())
    }
}

/// Calls that take the file path: the `fs.*` tools, `/init`'s AGENTS.md write and the
/// workspace command tools.
fn is_file_call(action_kind: &str) -> bool {
    action_kind.starts_with("fs.")
        || action_kind.starts_with("media.")
        || action_kind == super::workspace_init::ACTION
        || super::command_tools::is_command_tool(action_kind)
}
impl BrokerPort for Port {
    fn inspect(&self, operation: &SecuredOperation) -> BackendFuture<'_, Value> {
        let operation = operation.clone();
        let service = Arc::clone(&self.service);
        let call = self.call.clone();
        Box::pin(async move {
            if operation != call.operation {
                return Err(preparation("agent.operation_changed"));
            }
            // Boundary: the whole synchronous preparation (source re-derivation, live mode
            // check, domain inspection) runs on the one bounded Agent Store owner instead of on the
            // reactive task that polls this backend future.
            let store = Arc::clone(&service.store);
            let port = Port { service, call, commands: None };
            store
                .run_critical(
                    agent_store::StoreStep::run(
                        port.call.batch.invocation_identity.agent_run_id,
                        "agent.broker.inspect",
                    ),
                    agent_store::StoreAccess::Read,
                    move || port.inspect_sync(),
                )
                .await
                .map_err(preparation)
        })
    }
    fn still_prepared<'a>(
        &'a self,
        operation: &'a SecuredOperation,
        prepared: &'a Value,
    ) -> BackendFuture<'a, bool> {
        let operation = operation.clone();
        let prepared = prepared.clone();
        let service = Arc::clone(&self.service);
        let call = self.call.clone();
        Box::pin(async move {
            if operation != call.operation {
                return Err(preparation("agent.operation_changed"));
            }
            // Boundary: the same bounded section as `inspect`, comparing what it takes again.
            let store = Arc::clone(&service.store);
            let port = Port { service, call, commands: None };
            store
                .run_critical(
                    agent_store::StoreStep::run(
                        port.call.batch.invocation_identity.agent_run_id,
                        "agent.broker.inspect",
                    ),
                    agent_store::StoreAccess::Read,
                    move || port.still_prepared_sync(&prepared),
                )
                .await
                .map_err(preparation)
        })
    }
    fn claim(&self, dispatch: &AuthorizedDispatch) -> BackendFuture<'_, ()> {
        let request = dispatch.request().clone();
        let service = Arc::clone(&self.service);
        let call = self.call.clone();
        Box::pin(async move {
            // Boundary: the claim is ONE indivisible ordered section - the session
            // permission authority first, then the journal, with the live-call verification, the
            // dispatch authority and the dispatched mark in one transaction. It is moved whole onto
            // the one bounded Agent Store owner; `inspect_sync` inside it is therefore on a real
            // blocking thread, which is what its own contract requires.
            let store = Arc::clone(&service.store);
            let port = Port { service: Arc::clone(&service), call: call.clone(), commands: None };
            let persistence = Arc::clone(&service.persistence);
            let session_id = call.snapshot.session_id;
            store
                .run_critical(
                    agent_store::StoreStep::run(
                        call.batch.invocation_identity.agent_run_id,
                        "agent.broker.claim",
                    ),
                    agent_store::StoreAccess::Write,
                    move || {
                        persistence
                            .with_session_permission_authority(
                                session_id,
                                |_| -> anyhow::Result<()> {
                                    anyhow::ensure!(
                                        port.still_prepared_sync(
                                            &request.plan.prepared.preparation
                                        )?,
                                        "agent.resource_changed"
                                    );
                                    port.service.journal.lock()?.database.unit_of_work(|tx| {
                                        aura_persistence_v2::agent::checkpoint::verify_live_call(
                                            tx,
                                            &port.call.batch,
                                            port.call.validated.call.tool_call_id,
                                        )?;
                                        aura_persistence_v2::execution::dispatch_authority(
                                            tx,
                                            &request,
                                            UtcTimestamp::now(),
                                        )?;
                                        aura_persistence_v2::execution::mark_dispatched(
                                            tx,
                                            request.operation.operation_id,
                                            request.binding.attempt.ordinal.get(),
                                            UtcTimestamp::now(),
                                        )
                                    })?;
                                    Ok(())
                                },
                            )
                            .map_err(anyhow::Error::from)
                    },
                )
                .await
                .map_err(rejected)?
                .map_err(rejected)
        })
    }
    fn invoke(&self, request: &DispatchRequest) -> BackendFuture<'_, ()> {
        let service = self.service.clone();
        let call = self.call.clone();
        let request = request.clone();
        let commands = self.commands.clone();
        Box::pin(async move {
            // **The resource wait happens outside the blocking worker and outside the
            // long session authority.**
            //
            // File execution is synchronous, so it runs on a blocking worker. Writes hold
            // mode authority through publication; reads release it after verified start. The pre-fix shape entered that
            // worker *and* that authority before asking the common owner for the resource, so a call
            // that could not be admitted at all — no journal row, no side effect, no decision taken —
            // still occupied a worker and the session's long write authority for the whole waiting
            // time. Eight such waiters starved unrelated file work and an ordinary mode change.
            //
            // So each attempt asks for the resource inside the worker and reports a **live short
            // conflict** back as [`FsInvocation::AdmissionWaiting`] rather than as a tool error. The
            // worker and the authority are then gone, and this reactive task does the waiting,
            // bounded by the configured wait budget. A **long editor claim** is not a wait and keeps
            // its original `DeferredByEditorActivity` semantics.
            //
            // Input mutation and permission changes share this authority. A
            // reservation is not a start; recheck at the actual invocation.
            //
            // The lease belongs to **this session** and is released at the real
            // start below, not at the tool's terminal receipt. Holding the process-wide
            // read side for a whole tool was what made an unrelated session's ordinary
            // mode change and inbox write wait behind it.
            let mut start_lease =
                service.persistence.permission_start_lease(call.snapshot.session_id).await;
            if request.operation.content.action_kind.starts_with("memory.") {
                return super::memory::invoke(service, call, request, start_lease)
                    .await
                    .map_err(rejected);
            }
            if request.operation.content.action_kind.starts_with("git.") {
                return super::git::invoke(service, call, request, start_lease)
                    .await
                    .map_err(rejected);
            }
            if super::mcp_tools::is_mcp(&request.operation.content) {
                return super::mcp_tools::invoke(service, call, request, start_lease)
                    .await
                    .map_err(rejected);
            }
            if super::package_tools::program::is_program(&request.operation.content) {
                return super::package_tools::program::invoke(
                    service,
                    call,
                    request,
                    start_lease,
                    commands,
                )
                .await
                .map_err(rejected);
            }
            if request.operation.content.action_kind.starts_with("browser.") {
                let lifetime = service
                    .active_calls
                    .retain_worker(call.validated.call.tool_call_id)
                    .map_err(rejected)?;
                let result = super::browser::invoke(service, call, request, start_lease)
                    .await
                    .map_err(rejected);
                drop(lifetime);
                return result;
            }
            if request.operation.content.action_kind.starts_with("chrome.")
                && request.operation.content.action_kind != "chrome.tabs"
            {
                let lifetime = service
                    .active_calls
                    .retain_worker(call.validated.call.tool_call_id)
                    .map_err(rejected)?;
                let result = super::chrome::invoke(service, call, request, start_lease)
                    .await
                    .map_err(rejected);
                drop(lifetime);
                return result;
            }
            if request.operation.content.action_kind.starts_with("media.") {
                let lifetime = service
                    .active_calls
                    .retain_worker(call.validated.call.tool_call_id)
                    .map_err(rejected)?;
                let result = super::media::invoke(service, call, request, start_lease)
                    .await
                    .map_err(rejected);
                drop(lifetime);
                return result;
            }
            if request.operation.content.action_kind.starts_with("terminal.") {
                let commands = commands.ok_or_else(|| rejected("terminal.controller_missing"))?;
                let lifetime = service
                    .active_calls
                    .retain_worker(call.validated.call.tool_call_id)
                    .map_err(rejected)?;
                // The durable start rechecks current authority; the long output
                // wait does not hold the session's input/mode barrier.
                let verified =
                    super::terminal::invoke(service, call, request, commands, start_lease);
                let result = verified.await.map_err(rejected);
                drop(lifetime);
                return result;
            }
            if request.operation.content.action_kind == "shell.stop" {
                {
                    // Boundary: the start-operation verification, one bounded read.
                    let verify_request = request.clone();
                    let verifier = Arc::clone(&service);
                    service
                        .store
                        .run_critical(
                            agent_store::StoreStep::run(
                                call.batch.invocation_identity.agent_run_id,
                                "agent.broker.verify_start",
                            ),
                            agent_store::StoreAccess::Read,
                            move || {
                                let store = verifier.journal.lock()?;
                                Ok(aura_persistence_v2::agent::checkpoint::verify_start_operation(
                                    store.database.connection(),
                                    verify_request.operation.operation_id,
                                )?)
                            },
                        )
                        .await
                        .map_err(rejected)?;
                }
                let commands = commands.ok_or_else(|| rejected("shell.controller_missing"))?;
                let input: aura_windows_user_contract::ShellStopInput =
                    serde_json::from_value(request.operation.content.action.clone())
                        .map_err(rejected)?;
                let _activity = crate::agent_activity::execution::observe_current_execution();
                // An id that names no Shell call of this session stops nothing; the ledger step
                // below records that refusal as this call's result instead of leaving it open.
                if let Err(error) =
                    commands.stop_call(call.snapshot.session_id, input.tool_call_id).await
                    && !error.chain().any(|cause| {
                        cause
                            .to_string()
                            .contains(aura_persistence_v2::execution::shell::NOT_A_SHELL_CALL)
                    })
                {
                    return Err(rejected(error));
                }
                // Boundary: the shell-stop ledger transaction, one bounded write.
                {
                    let stopper = Arc::clone(&service);
                    let stop_request = request.clone();
                    let stop_call = call.clone();
                    service
                        .store
                        .run_critical(
                            agent_store::StoreStep::run(
                                stop_call.batch.invocation_identity.agent_run_id,
                                "agent.broker.shell_stop",
                            ),
                            agent_store::StoreAccess::Write,
                            move || {
                                Ok(stopper.journal.lock()?.database.unit_of_work(|tx| {
                                    aura_persistence_v2::agent::tool_state::execute_with(
                                        tx,
                                        &stop_request,
                                        &stop_call.batch,
                                        stop_call.validated.call.tool_call_id,
                                        UtcTimestamp::now(),
                                        |tx, _, _| {
                                            Ok(serde_json::to_value(
                                                aura_persistence_v2::execution::shell_result::model_result(
                                                    tx,
                                                    stop_call.snapshot.session_id,
                                                    input.tool_call_id,
                                                )?,
                                            )?)
                                        },
                                    )
                                })?)
                            },
                        )
                        .await
                        .map_err(rejected)?;
                }
                return Ok(());
            }
            // A non-file Agent tool has no file resource to be admitted to, so it takes the
            // original path unchanged: one blocking worker, the start lease held for the call, and
            // the task executor's own real terminal.
            if !is_file_call(&call.operation.content.action_kind) {
                let observer = crate::agent_activity::execution::current_file_observer();
                let lifetime = service
                    .active_calls
                    .retain_worker(call.validated.call.tool_call_id)
                    .map_err(rejected)?;
                return tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    let _lifetime = lifetime;
                    let _start_lease = start_lease;
                    aura_fs_tools::with_file_execution_observer(observer, || {
                        super::task::execute(&service, &call, &request)
                    })
                })
                .await
                .map_err(|error| rejected(format!("agent.task_join: {error}")))?
                .map_err(rejected);
            }
            // **The resource wait happens outside the blocking worker and outside the
            // long session authority.**
            //
            // The file execution is synchronous and holds the mode reader barrier to its real
            // terminal point, so it must run on a blocking worker. The pre-fix shape entered that
            // worker *and* that authority before asking the common owner for the resource, so a call
            // that could not be admitted at all — no journal row, no side effect, no decision taken —
            // still occupied a worker and the session's long write authority for the whole waiting
            // time. Eight such waiters starved unrelated file work and an ordinary mode change.
            //
            // So each attempt asks for the resource inside the worker and reports a **live short
            // conflict** back as [`FsInvocation::AdmissionWaiting`] rather than as a tool error. The
            // worker and the authority are then gone, and this reactive task does the waiting,
            // bounded by the configured wait budget. A **long editor claim** is not a wait and keeps
            // its original `DeferredByEditorActivity` semantics. When a wait ends the **whole
            // verified start is re-entered** — original mode and revision, start operation,
            // cancellation intent, source object and the plan's `expected` — and nothing from the
            // refused attempt is reused. Only the typed pre-effect ResourceBusy refusal is retried;
            // every other outcome from `files::invoke` is terminal and never rerun.
            let observer = crate::agent_activity::execution::current_file_observer();
            let admission_budget = service.fs.mutation_wait_budget();
            let admission_deadline = std::time::Instant::now() + admission_budget;
            let mut interval = std::time::Duration::from_millis(1);
            loop {
                let attempt_service = Arc::clone(&service);
                let attempt_call = call.clone();
                let attempt_request = request.clone();
                let attempt_observer = observer.clone();
                let lifetime = service
                    .active_calls
                    .retain_worker(call.validated.call.tool_call_id)
                    .map_err(rejected)?;
                let outcome =
                    tokio::task::spawn_blocking(move || -> anyhow::Result<FsInvocation> {
                        let _lifetime = lifetime;
                        aura_fs_tools::with_file_execution_observer(attempt_observer, || {
                            invoke_file_attempt(
                                &attempt_service,
                                &attempt_call,
                                &attempt_request,
                                start_lease,
                            )
                        })
                    })
                    .await
                    .map_err(|error| rejected(format!("agent.file_attempt_join: {error}")))?;
                match outcome.map_err(rejected)? {
                    FsInvocation::Done => return Ok(()),
                    FsInvocation::AdmissionWaiting => {}
                }
                let now = std::time::Instant::now();
                if now >= admission_deadline {
                    return Err(rejected(
                        "agent.resource_wait_exhausted: another execution still held this file \
                         resource after the configured wait budget; nothing was written and the \
                         tool did not start",
                    ));
                }
                let remaining = admission_deadline.saturating_duration_since(now);
                let executing_capacity =
                    aura_tool_scheduler::pre_effect_wait::suspend_after_resource_refusal()
                        .map_err(rejected)?;
                tokio::time::sleep(interval.min(remaining)).await;
                interval = (interval * 2).min(std::time::Duration::from_millis(20));
                if let Some(capacity) = executing_capacity {
                    tokio::time::timeout_at(admission_deadline.into(), capacity.resume())
                        .await
                        .map_err(|_| rejected("agent.resource_wait_exhausted"))?
                        .map_err(rejected)?;
                }
                // The lease is re-taken for the next attempt, because the previous attempt really
                // released it: that release is what lets a tightening of this session and an
                // unrelated session's mode change through while this call is doing nothing.
                start_lease = tokio::time::timeout_at(
                    admission_deadline.into(),
                    service.persistence.permission_start_lease(call.snapshot.session_id),
                )
                .await
                .map_err(|_| rejected("agent.resource_wait_exhausted"))?;
            }
        })
    }
    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        let service = Arc::clone(&self.service);
        // Git and package programs stop their own Job through the call's cancel relay; a browser
        // call withdraws its command from the host the same way.
        let git_call = (self.call.operation.content.action_kind.starts_with("git.")
            || self.call.operation.content.action_kind.starts_with("browser.")
            || super::package_tools::program::is_program(&self.call.operation.content)
            || super::mcp_tools::is_mcp(&self.call.operation.content))
        .then(|| self.call.clone());
        Box::pin(async move {
            // Boundary: `verify_handle` takes `journal.lock()` directly. It has no
            // `journal.lock()` literal at this call site, so the text classifier cannot see
            // it: it runs whole on the one bounded Agent Store owner instead of on the
            // reactive task that polls this backend future.
            {
                let verifier = Arc::clone(&service);
                let verified = handle.clone();
                service
                    .section(
                        StoreStep::operation("agent.broker.cancel_verify"),
                        StoreAccess::Read,
                        move || verifier.journal.verify_handle(&verified),
                    )
                    .await
                    .map_err(rejected)?;
            }
            if let Some(call) = git_call {
                for entry in
                    service.active_calls.session(call.snapshot.session_id).map_err(rejected)?
                {
                    if entry.call.validated.call.tool_call_id == call.validated.call.tool_call_id {
                        entry.cancel.send_replace(true);
                    }
                }
            }
            // Boundary: the cancellation request transaction, one bounded write.
            {
                let canceller = Arc::clone(&service);
                let cancel_handle = handle.clone();
                service
                    .store
                    .run_critical(
                        agent_store::StoreStep::operation("agent.broker.cancel"),
                        agent_store::StoreAccess::Write,
                        move || {
                            Ok(canceller.journal.lock()?.database.unit_of_work(|tx| {
                                aura_persistence_v2::execution::request_cancel(
                                    tx,
                                    &cancel_handle.attempt,
                                    UtcTimestamp::now(),
                                )
                            })?)
                        },
                    )
                    .await
                    .map_err(rejected)?;
            }
            let observed = {
                // Boundary: the post-cancel attempt read, one bounded read. It
                // reaches `broker/journal.rs::reconcile`'s own `self.lock()` and SQL.
                let reader = Arc::clone(&service);
                let read_attempt = handle.attempt.clone();
                service
                    .section(
                        StoreStep::operation("agent.broker.cancel_observe"),
                        StoreAccess::Read,
                        move || reader.journal.reconcile(read_attempt),
                    )
                    .await
                    .map_err(rejected)?
            };
            let termination = match observed {
                ReconcileResult::Completed { result } => result.termination,
                _ => TerminationEvidence::Unknown,
            };
            Ok(CancelReceipt { handle, termination })
        })
    }
    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            // Boundary: this is the real control entry a counterexample drove.
            // `journal.reconcile` locks the journal and reads SQLite; it is not a
            // `journal.lock()` literal here, so it runs whole on the bounded owner.
            let reader = Arc::clone(&service);
            service
                .section(
                    StoreStep::operation("agent.broker.reconcile"),
                    StoreAccess::Read,
                    move || reader.journal.reconcile(attempt),
                )
                .await
                .map_err(rejected)
        })
    }
}
