//! Uses the ControlledWeb application's own claim/activity/receipt protocol.
//! It cannot borrow the authenticated Main HTTP grant or double-claim a call.
use super::*;
use crate::agent_store::{StoreAccess, StoreStep};
use aura_backend_api::{BackendFuture, ExecutionBackend};

/// The run this call belongs to, for the bounded sections' identity.
fn run_id(call: &CallContext) -> AgentRunId {
    call.batch.invocation_identity.agent_run_id
}

pub(super) struct WebBackend {
    pub service: Arc<AgentExecution>,
    pub call: CallContext,
}
impl ExecutionBackend for WebBackend {
    fn descriptor(&self) -> BackendDescriptor {
        broker::descriptor(self.service.runtime)
    }
    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        let service = Arc::clone(&self.service);
        let call = self.call.clone();
        Box::pin(async move {
            if request.operation != call.operation
                || !broker::descriptor(service.runtime)
                    .supports_operation(&request.requirement, &request.operation)
            {
                return Err(preparation("agent.web_unsupported"));
            }
            // Boundary: `verify_source` takes the journal lock directly, so it runs
            // whole on the one bounded Agent Store owner instead of on the reactive task.
            service.verify_source_section(&call).await.map_err(preparation)?;
            service.web.preflight_network(&request.operation).await.map_err(preparation)?;
            if request.operation.content.action_kind == "http.download" {
                let checker = Arc::clone(&service);
                let target =
                    network::download_target(&request.operation.content).map_err(preparation)?;
                let snapshot = call.snapshot.clone();
                service
                    .section(
                        StoreStep::run(run_id(&call), "agent.http.download_preflight"),
                        StoreAccess::Read,
                        move || {
                            Ok(checker.fs.preflight_download(
                                &target,
                                snapshot.workspace_id,
                                snapshot.session_id,
                            )?)
                        },
                    )
                    .await
                    .map_err(preparation)?;
            }
            // Boundary: `ControlledWeb::inspect` reads the live mode, the workspace
            // projection and the stored web document through the journal. It is a synchronous
            // section with no `journal.lock()` literal at this call site, so it runs whole on
            // the bounded owner rather than on the task polling this backend future.
            let inspected = service
                .web
                .inspect_section(run_id(&call), &request.operation)
                .await
                .map_err(preparation)?;
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint.clone(),
                backend: broker::descriptor(service.runtime),
                preparation: inspected,
            })
            .map_err(preparation)
        })
    }
    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        let service = Arc::clone(&self.service);
        let call = self.call.clone();
        Box::pin(async move {
            let request = dispatch.request();
            if request.operation != call.operation
                || request.plan.prepared.backend != broker::descriptor(service.runtime)
            {
                return Err(rejected("agent.web_binding_changed"));
            }
            service.verify_source_section(&call).await.map_err(rejected)?;
            let attempt = request.binding.attempt.clone();
            // Network failure is a durable failed result; only absence of that
            // result after entering execute is dispatch-unknown.
            let outcome = service.web.execute(&dispatch, &service, &call).await;
            // Boundary: the durable attempt read on the unknown path. The first read
            // below is the same `journal.reconcile` section a counterexample drove; it
            // has no `journal.lock()` literal here, so it runs on the bounded owner too.
            let observed = {
                let reader = Arc::clone(&service);
                let read_attempt = attempt.clone();
                service
                    .section(
                        StoreStep::run(run_id(&call), "agent.web.reconcile"),
                        StoreAccess::Read,
                        move || reader.journal.reconcile(read_attempt),
                    )
                    .await
                    .map_err(rejected)?
            };
            if !matches!(observed, ReconcileResult::Completed { .. }) {
                // Boundary: the durable attempt read on the unknown path, one bounded read.
                let reader = Arc::clone(&service);
                let read_attempt = attempt.clone();
                let current = service
                    .store
                    .run_critical(
                        crate::agent_store::StoreStep::run(
                            call.batch.invocation_identity.agent_run_id,
                            "agent.web.attempt_state",
                        ),
                        crate::agent_store::StoreAccess::Read,
                        move || {
                            Ok(aura_persistence_v2::execution::load(
                                reader.journal.lock()?.database.connection(),
                                read_attempt.operation_id,
                                read_attempt.ordinal.get(),
                            )?)
                        },
                    )
                    .await
                    .map_err(rejected)?;
                if current.state == AttemptState::Prepared && current.dispatched_at.is_none() {
                    return Err(rejected(
                        outcome
                            .err()
                            .map_or_else(|| "agent.web_no_claim".into(), |e| e.to_string()),
                    ));
                }
                return Err(BackendError::DispatchUnknown { attempt });
            }
            Ok(ExecutionHandle {
                attempt,
                dispatch_fingerprint: aura_domain_common::jcs::canonical_sha256(&request.binding)
                    .map_err(rejected)?,
            })
        })
    }
    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            // Boundary: `verify_handle` locks the journal inside
            // `broker/journal.rs`, so it runs whole on the bounded owner.
            {
                let verifier = Arc::clone(&service);
                let verified = handle.clone();
                service
                    .section(
                        StoreStep::operation("agent.web.cancel_verify"),
                        StoreAccess::Read,
                        move || verifier.journal.verify_handle(&verified),
                    )
                    .await
                    .map_err(rejected)?;
            }
            // Boundary: the cancellation request transaction, one bounded write.
            {
                let canceller = Arc::clone(&service);
                let cancel_handle = handle.clone();
                service
                    .store
                    .run_critical(
                        crate::agent_store::StoreStep::operation("agent.web.cancel"),
                        crate::agent_store::StoreAccess::Write,
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
            service.activities.cancel(handle.attempt.operation_id).map_err(rejected)?;
            // Boundary: the post-cancel attempt read, one bounded read.
            let observed = {
                let reader = Arc::clone(&service);
                let read_attempt = handle.attempt.clone();
                service
                    .section(
                        StoreStep::operation("agent.web.cancel_observe"),
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
            // Boundary: the same control entry a counterexample drove for the
            // broker, on the web backend's own control surface.
            let reader = Arc::clone(&service);
            service
                .section(
                    StoreStep::operation("agent.web.reconcile"),
                    StoreAccess::Read,
                    move || reader.journal.reconcile(attempt),
                )
                .await
                .map_err(rejected)
        })
    }
}
