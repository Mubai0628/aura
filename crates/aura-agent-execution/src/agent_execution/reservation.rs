use super::*;
use aura_core_broker::agent_store;
use aura_permission_engine::policy::PolicyInput;
use aura_persistence_v2::{execution, tool::execution::CallReservation};

pub(super) struct Reservation {
    pub service: Arc<AgentExecution>,
    pub call: CallContext,
    pub cancel: tokio::sync::watch::Receiver<bool>,
}
impl CallContext {
    pub fn policy(&self, now: UtcTimestamp, runtime: RuntimeInstanceId) -> PolicyInput<'_> {
        PolicyInput {
            network_caller: aura_security_contract::network_caller_for(
                &self.operation.content.action_kind,
            ),
            operation: &self.operation,
            snapshot: &self.snapshot,
            profile: SecurityProfile::for_permission_mode(self.snapshot.mode),
            source_enabled: true,
            trust: &TrustState::SystemTrusted,
            source_fingerprint: self.operation.content.source_revision_fingerprint.as_ref(),
            hard_boundaries_satisfied: true,
            denied_for_turn: false,
            grants: &[],
            verified_external_grants: &self.verified_external_grants,
            verified_external_capabilities: &self.verified_external_capabilities,
            now,
            expires_at: UtcTimestamp::from_value(now.value() + time::Duration::minutes(5)),
            core_runtime_instance_id: Some(runtime),
        }
    }
}
impl aura_execution_orchestrator::DispatchStore for Reservation {
    fn reserve<'a>(
        &'a self,
        request: DispatchRequest,
        authorization: &'a AuthorizationDecision,
    ) -> aura_backend_api::BackendFuture<'a, AuthorizedDispatch> {
        let attempt = request.binding.attempt.clone();
        let service = Arc::clone(&self.service);
        let call = self.call.clone();
        let cancel = self.cancel.clone();
        // The decision is owned, not borrowed: the section below is `Send + 'static` because the
        // work really moves to a pool thread, and the same decision value is what it uses.
        let authorization = authorization.clone();
        let store = Arc::clone(&self.service.store);
        // The hold keys are built before the section so the section can own `call`; they are only
        // read in an acceptance build, so they are compiled out of every product build.
        #[cfg(feature = "acceptance-barrier")]
        let hold_keys = (
            call.batch.invocation_identity.agent_run_id.to_string(),
            call.validated.call.tool_call_id.to_string(),
        );
        let run_id = call.batch.invocation_identity.agent_run_id;
        Box::pin(async move {
            // Boundary: the reservation is ONE indivisible synchronous section - the live
            // source re-derivation, the executing mark and the authority consumption share one
            // transaction - and it is handed **whole** to the one bounded Agent Store owner. The
            // closure is written inline as that owner's argument so the section boundary is visible
            // in the source rather than hidden behind a local binding. The orchestrator used to
            // poll this from a reactive task; it no longer waits on the journal there.
            let prepared = store
                .run_critical(
                    agent_store::StoreStep::run(run_id, "agent.tool.reserve"),
                    agent_store::StoreAccess::Write,
                    move || -> anyhow::Result<_> {
                        anyhow::ensure!(
                            !*cancel.borrow() && request.operation == call.operation,
                            "agent.cancelled_or_changed"
                        );
                        service.verify_source(&call)?;
                        let external_capabilities = service.external_authority(&call.operation);
                        let external_grants = service.external_authority_slices(&call.operation);
                        let mcp_denied =
                            super::mcp_tools::denied_for_turn(&service, &call.operation)?;
                        let mut current = call.policy(UtcTimestamp::now(), service.runtime);
                        current.denied_for_turn = mcp_denied;
                        current.verified_external_capabilities = &external_capabilities;
                        current.verified_external_grants = &external_grants;
                        let backend = request.plan.prepared.backend.clone();
                        let mut store = service.journal.lock()?;
                        // The scheduler activated a real slot for this call, so this is where it
                        // stops waiting and starts performing. The mark is written in the same
                        // transaction as the reservation, so the projection can never show a
                        // reserved Attempt without the executing fact that explains it.
                        store.database.unit_of_work(|tx| {
                            aura_persistence_v2::tool::preparing::mark_executing(
                                tx,
                                call.batch.tool_batch_id,
                                call.validated.call.tool_call_id,
                                UtcTimestamp::now(),
                            )
                        })?;
                        Ok(crate::security_control::reserve_tool_dispatch(
                            &service.persistence,
                            &mut store,
                            request,
                            &authorization,
                            &current,
                            &backend,
                            &CancellationState {
                                revision: Revision::INITIAL,
                                requested: *cancel.borrow(),
                            },
                            &CallReservation {
                                batch: call.batch.clone(),
                                call_id: call.validated.call.tool_call_id,
                                attempt_id: ToolExecutionAttemptId::new(),
                            },
                        )?)
                    },
                )
                .await;
            // S04-E acceptance hold point: the call's reservation has committed and no backend has
            // been dispatched yet, so a driver can park this exact dispatch and a real interruption
            // here leaves a reserved Attempt with no terminal receipt. Compiled only with the
            // acceptance feature; with no control document this is one `Option::is_none`.
            #[cfg(feature = "acceptance-barrier")]
            if prepared.is_ok() {
                agent_store::acceptance_hold::hold_tool_reservation(&hold_keys.0, &hold_keys.1);
            }
            prepared.map_err(|error| BackendError::DispatchRejected {
                reason: format!("agent.reservation:{}:{error}", attempt.operation_id),
            })
        })
    }
    fn reject_prepared<'a>(
        &'a self,
        request: &'a DispatchRequest,
        reason: &'a str,
    ) -> aura_backend_api::BackendFuture<'a, ExecutionResult> {
        let service = Arc::clone(&self.service);
        let store = Arc::clone(&self.service.store);
        let request = request.clone();
        let unknown = request.binding.attempt.clone();
        let reason = reason.to_owned();
        Box::pin(async move {
            // Boundary: the paired rejection record, one bounded write.
            store
                .run_critical(
                    agent_store::StoreStep::operation("agent.tool.reject_prepared"),
                    agent_store::StoreAccess::Write,
                    move || {
                        service
                            .journal
                            .lock()?
                            .database
                            .unit_of_work(|tx| {
                                execution::reject_prepared(
                                    tx,
                                    &request,
                                    &reason,
                                    UtcTimestamp::now(),
                                )
                            })
                            .map_err(|_| {
                                anyhow::anyhow!("agent.reject_prepared:{}", unknown.operation_id)
                            })
                    },
                )
                .await
                .map_err(|_| BackendError::DispatchUnknown { attempt: unknown })
        })
    }
    fn request_cancel<'a>(
        &'a self,
        attempt: &'a AttemptRef,
    ) -> aura_backend_api::BackendFuture<'a, ()> {
        let service = Arc::clone(&self.service);
        let store = Arc::clone(&self.service.store);
        let attempt = attempt.clone();
        Box::pin(async move {
            // Boundary: the cancellation request, one bounded write.
            store
                .run_critical(
                    agent_store::StoreStep::operation("agent.tool.request_cancel"),
                    agent_store::StoreAccess::Write,
                    move || {
                        Ok(service.journal.lock()?.database.unit_of_work(|tx| {
                            execution::request_cancel(tx, &attempt, UtcTimestamp::now())
                        })?)
                    },
                )
                .await
                .map_err(rejected)
        })
    }
}
