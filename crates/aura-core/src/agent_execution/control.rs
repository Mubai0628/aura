use super::*;
use anyhow::Context;
use aura_execution_orchestrator::Selection;
use aura_persistence_core::PersistenceCoreError as Error;
use aura_persistence_v2::{agent::control, tool::execution::CallReservation};
use std::collections::BTreeMap;

impl AgentExecution {
    pub(super) async fn execute_control(
        self: &Arc<Self>,
        admitted: &AdmittedBatch,
        call_ids: &[ToolCallId],
        mut cancel: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<Vec<(ToolCallId, CallOutcome)>> {
        // Domain preflight precedes all lock/Attempt acquisition. These are the
        // exact batch calls, not a partial subset selected by an Agent input.
        // Boundary: the control preflight read is one bounded section.
        let prepared = {
            let service = Arc::clone(self);
            let batch = admitted.batch.clone();
            let run_id = admitted.batch.invocation_identity.agent_run_id;
            self.section(
                StoreStep::run(run_id, "agent.control.preflight"),
                StoreAccess::Read,
                move || {
                    // `control::prepare` answers with `anyhow::Error` already; the section only
                    // adds the operation's own context, so the error class is preserved exactly.
                    control::prepare(
                        service.journal.lock()?.database.connection(),
                        &batch,
                        UtcTimestamp::now(),
                    )
                    .context("agent.control.preflight")
                },
            )
            .await?
        };
        let executable: Vec<_> = prepared.executed_call_ids().collect();
        let mut selected_calls = BTreeMap::new();
        for call_id in &executable {
            let call = admitted
                .calls
                .get(call_id)
                .ok_or_else(|| anyhow::anyhow!("agent.control_call_missing"))?;
            self.verify_source_section(call).await?;
            let backend = self.broker_backend(call)?;
            match self
                .orchestrator
                .select(&call.policy(UtcTimestamp::now(), self.runtime), &[backend], None)
                .await
                .map_err(|e| anyhow::anyhow!("{e:?}"))?
            {
                Selection::Ready(selected) => {
                    selected_calls.insert(*call_id, selected);
                }
                Selection::ApprovalRequired { proposal, backend } => {
                    return Ok(vec![(
                        *call_id,
                        CallOutcome::ApprovalRequired { proposal, backend },
                    )]);
                }
                Selection::Denied { reason } => {
                    return Ok(vec![(*call_id, CallOutcome::Denied(reason))]);
                }
            }
        }
        let first =
            *call_ids.first().ok_or_else(|| anyhow::anyhow!("agent.empty_control_group"))?;
        let plan = admitted
            .planned
            .execution_plan
            .calls
            .iter()
            .find(|c| c.tool_call_id == first)
            .ok_or_else(|| anyhow::anyhow!("agent.control_plan"))?;
        for id in call_ids {
            let other = admitted
                .planned
                .execution_plan
                .calls
                .iter()
                .find(|c| c.tool_call_id == *id)
                .ok_or_else(|| anyhow::anyhow!("agent.control_plan"))?;
            anyhow::ensure!(
                other.resolved_resource_claims.iter().all(|c| plan
                    .resolved_resource_claims
                    .iter()
                    .any(|p| p.canonical_lock_key == c.canonical_lock_key
                        && p.kind == c.kind
                        && p.access == c.access)),
                "agent.control_group_resources"
            );
        }
        let runtime = admitted
            .planned
            .runtime_calls
            .iter()
            .find(|c| c.validated_call.call.tool_call_id == first)
            .ok_or_else(|| anyhow::anyhow!("agent.control_runtime"))?;
        let _permit = self
            .orchestrator
            .acquire_control_permit(
                aura_tool_scheduler::ToolScheduleRequestV1 {
                    plan_call: plan,
                    generation: runtime.generation.clone(),
                    source_lock_key: None,
                },
                &mut cancel,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let mut reservations = BTreeMap::new();
        for (id, selected) in selected_calls {
            reservations.insert(
                id,
                selected.into_control_reservation().await.map_err(|e| anyhow::anyhow!("{e:?}"))?,
            );
        }
        let results = {
            // Boundary: the ordered control commit. It is ONE indivisible section - the
            // session permission authority first, then the journal, with every reservation
            // committed inside one transaction - and it is moved whole onto the one bounded Agent
            // Store owner. The lock order and the indivisibility are unchanged; only the thread
            // that waits is.
            let persistence = Arc::clone(&self.persistence);
            let service = Arc::clone(self);
            let snapshot = admitted.snapshot.clone();
            let batch = admitted.batch.clone();
            let calls = admitted.calls.clone();
            let cancel_state = *cancel.borrow();
            let runtime = self.runtime;
            self.store
                .run_critical(
                    StoreStep::run(
                        admitted.batch.invocation_identity.agent_run_id,
                        "agent.control.commit",
                    ),
                    StoreAccess::Write,
                    move || {
                        persistence
                            .with_session_permission_authority(snapshot.session_id, |mode| {
                                if cancel_state
                                    || mode.mode != snapshot.mode
                                    || mode.revision != snapshot.session_permission_revision
                                {
                                    return Err(Error::Conflict);
                                }
                                let mode_revision = mode.revision;
                                service.journal.lock()?.database.unit_of_work(|tx| {
                                    let mut dispatches = BTreeMap::new();
                                    for (call_id, (request, authority)) in reservations {
                                        let call = &calls[&call_id];
                                        if request.plan.prepared.backend
                                            != broker::descriptor(runtime)
                                        {
                                            return Err(Error::Conflict);
                                        }
                                        let policy = call.policy(UtcTimestamp::now(), runtime);
                                        let backend = request.plan.prepared.backend.clone();
                                        let binding = CallReservation {
                                            batch: batch.clone(),
                                            call_id,
                                            attempt_id: ToolExecutionAttemptId::new(),
                                        };
                                        let dispatch =
                                            crate::security_control::reserve_in_transaction(
                                                tx,
                                                request,
                                                &authority,
                                                &policy,
                                                &backend,
                                                &CancellationState {
                                                    revision: Revision::INITIAL,
                                                    requested: cancel_state,
                                                },
                                                &format!("tool:{call_id}"),
                                                Some(&binding),
                                                mode_revision,
                                            )?;
                                        dispatches.insert(call_id, dispatch);
                                    }
                                    control::commit(tx, prepared, &dispatches, UtcTimestamp::now())
                                })
                            })
                            .map_err(anyhow::Error::from)
                    },
                )
                .await??
        };
        Ok(results
            .into_iter()
            .map(|r| (r.tool_call_id, CallOutcome::Terminal(Box::new(r))))
            .collect())
    }
}
