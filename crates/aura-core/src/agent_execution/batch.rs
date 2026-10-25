//! Runs the admitted DAG. Backend errors are classified by the durable claim,
//! never by assuming that a lost response means no process was started.
use super::*;
use aura_execution_orchestrator::{BatchProgress, DispatchUnit, UnitReadiness};
use aura_persistence_v2::{agent::history, tool};
use std::collections::BTreeMap;
use tokio::sync::watch;

pub(crate) enum BatchOutcome {
    Completed(aura_context_domain::HistoryAtomicGroupPreimageV1),
    /// The proposal is boxed for the same reason the orchestrator boxes it: it
    /// carries the consent facts for a session approval, and an unboxed variant of
    /// that size would make every other outcome pay for it.
    AwaitingApproval {
        call_id: ToolCallId,
        proposal: Box<AuthorizationProposal>,
        backend: Box<BackendDescriptor>,
    },
    Unknown {
        attempts: Vec<AttemptRef>,
    },
}
impl AgentExecution {
    pub async fn execute_batch(
        self: &Arc<Self>,
        admitted: AdmittedBatch,
        commands: Arc<crate::command_execution::CommandExecution>,
        mut external_cancel: watch::Receiver<bool>,
    ) -> anyhow::Result<BatchOutcome> {
        let admitted = Arc::new(admitted);
        let run_id = admitted.batch.invocation_identity.agent_run_id;
        // Recover durable receipts before scheduling. A prior Attempt means
        // reconcile-only, even if the previous caller lost its response.
        let mut unfinished = vec![];
        {
            // Boundary: the durable-receipt reconciliation. It read and (through
            // `materialize`) wrote the ledger while holding the journal on the reactive task that
            // drives this batch; it now runs whole on the one bounded Agent Store owner.
            let service = Arc::clone(self);
            let batch = Arc::clone(&admitted);
            self.section(
                StoreStep::run(run_id, "agent.tool.reconcile_receipts"),
                StoreAccess::Write,
                move || {
                    let mut store = service.journal.lock()?;
                    let mut unfinished = vec![];
                    for c in &batch.batch.calls {
                        let value =
                            tool::execution::call(store.database.connection(), c.tool_call_id)?;
                        if value.result.is_none() && value.attempt_id.is_some() {
                            let attempt = AttemptRef {
                                operation_id: batch.calls[&c.tool_call_id].operation.operation_id,
                                ordinal: std::num::NonZeroU32::MIN,
                            };
                            if matches!(
                                aura_persistence_v2::execution::load(
                                    store.database.connection(),
                                    attempt.operation_id,
                                    1
                                )?
                                .state,
                                AttemptState::Terminal { .. }
                            ) {
                                store.database.unit_of_work(|tx| {
                                    tool::result::materialize(
                                        tx,
                                        batch.batch.tool_batch_id,
                                        c.tool_call_id,
                                    )
                                })?;
                            } else {
                                unfinished.push(attempt);
                            }
                        }
                    }
                    Ok(unfinished)
                },
            )
            .await
            .map(|value| unfinished = value)?;
        }
        if !unfinished.is_empty() {
            return Ok(BatchOutcome::Unknown { attempts: unfinished });
        }
        let committed = {
            // Boundary: the already-committed-results read, one bounded section.
            let service = Arc::clone(self);
            let batch = Arc::clone(&admitted);
            self.section(
                StoreStep::run(run_id, "agent.tool.committed_results"),
                StoreAccess::Read,
                move || {
                    let store = service.journal.lock()?;
                    Ok(batch
                        .batch
                        .calls
                        .iter()
                        .map(|c| {
                            tool::execution::call(store.database.connection(), c.tool_call_id)
                                .map(|v| v.result)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>())
                },
            )
            .await?
        };
        let mut progress = BatchProgress::new(admitted.planned.execution_plan.clone(), &committed)?;
        let mut running = tokio::task::JoinSet::new();
        let (cancel_sender, cancel) = watch::channel(*external_cancel.borrow());
        let bridge = cancel_sender.clone();
        let forward = tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed=external_cancel.changed()=>{
                        if changed.is_err(){break;}
                        if *external_cancel.borrow(){bridge.send_replace(true);break;}
                    }
                    _=bridge.closed()=>break,
                }
            }
        });
        let result: anyhow::Result<BatchOutcome> = async {
            let mut workers = std::collections::HashMap::new();
            let mut paused = None;
            let mut unknown = vec![];
            loop {
                if paused.is_none() && unknown.is_empty() {
                    // The watch borrow is copied out first: a `watch::Ref` held across the awaits
                    // below would make this whole run task non-`Send`.
                    let cancelled_now = *cancel.borrow();
                    for ready in progress.ready(cancelled_now) {
                        match ready {
                            UnitReadiness::Rejected { unit, reason } => {
                                let results = self
                                    .refuse_unit_async(Arc::clone(&admitted), unit.clone(), reason)
                                    .await?;
                                progress.committed(&unit, &results)?;
                            }
                            UnitReadiness::Ready(unit) => {
                                progress.start(&unit)?;
                                let service = self.clone();
                                let batch = admitted.clone();
                                let commands = commands.clone();
                                let cancelled = cancel.clone();
                                let worker_unit = unit.clone();
                                let worker = running.spawn(async move {
                                    service
                                        .execute_unit(batch, worker_unit, commands, cancelled)
                                        .await
                                });
                                workers.insert(worker.id(), unit);
                            }
                        }
                    }
                }
                if running.is_empty() {
                    break;
                }
                let completed = running
                    .join_next_with_id()
                    .await
                    .ok_or_else(|| anyhow::anyhow!("agent.batch_worker_missing"))?;
                let (worker, outcome) = match completed {
                    Ok(value) => value,
                    Err(error) => {
                        (error.id(), Err(anyhow::anyhow!("agent.worker_terminated:{error}")))
                    }
                };
                let unit = workers
                    .remove(&worker)
                    .ok_or_else(|| anyhow::anyhow!("agent.batch_worker_unknown"))?;
                let outcome = match outcome {
                    Ok(v) => v,
                    Err(error) => {
                        // Boundary: recording the dispatch error is one bounded write.
                        let service = Arc::clone(self);
                        let batch = Arc::clone(&admitted);
                        let unit_calls = unit.calls.clone();
                        let message = error.to_string();
                        let refusal = refusal_notice(&message);
                        self.section(
                            StoreStep::run(run_id, "agent.tool.dispatch_error"),
                            StoreAccess::Write,
                            move || {
                                Ok(service.journal.lock()?.database.unit_of_work(|tx| {
                                    tool::execution::record_dispatch_error(
                                        tx,
                                        batch.batch.tool_batch_id,
                                        &unit_calls,
                                        &message,
                                        UtcTimestamp::now(),
                                    )
                                })?)
                            },
                        )
                        .await?;
                        let cancelled_now = *cancel.borrow();
                        self.classify_unit_async(
                            Arc::clone(&admitted),
                            unit.clone(),
                            cancelled_now,
                            refusal,
                        )
                        .await?
                    }
                };
                let mut results = BTreeMap::new();
                for (call_id, outcome) in outcome {
                    match outcome {
                        CallOutcome::Terminal(result) => {
                            results.insert(call_id, *result);
                        }
                        CallOutcome::Denied(reason) => {
                            let reason = if reason == PolicyDenyReason::BackendUnavailable {
                                ToolNotExecutedReasonV1::RuntimeUnavailable
                            } else {
                                ToolNotExecutedReasonV1::PermissionDenied
                            };
                            let refused = self
                                .refuse_unit_async(Arc::clone(&admitted), unit.clone(), reason)
                                .await?;
                            results.extend(refused.into_iter().map(|r| (r.tool_call_id, r)));
                        }
                        CallOutcome::ApprovalRequired { proposal, backend } => {
                            paused =
                                Some(BatchOutcome::AwaitingApproval { call_id, proposal, backend });
                        }
                        CallOutcome::NotStarted { message } => {
                            let refused = self
                                .refuse_with_notice_async(
                                    Arc::clone(&admitted),
                                    unit.clone(),
                                    ToolNotExecutedReasonV1::RuntimeUnavailable,
                                    message.clone(),
                                )
                                .await?;
                            results.extend(refused.into_iter().map(|r| (r.tool_call_id, r)));
                        }
                        CallOutcome::Unknown(attempt) => unknown.push(attempt),
                    }
                }
                if results.len() == unit.calls.len() {
                    let ordered = unit
                        .calls
                        .iter()
                        .map(|id| results.remove(id).expect("complete unit map"))
                        .collect::<Vec<_>>();
                    progress.committed(&unit, &ordered)?;
                }
            }
            if !unknown.is_empty() {
                return Ok(BatchOutcome::Unknown { attempts: unknown });
            }
            if let Some(paused) = paused {
                return Ok(paused);
            }
            anyhow::ensure!(progress.finished(), "agent.batch_stalled");
            // Boundary: completing the batch history is one bounded write.
            let service = Arc::clone(self);
            let batch = Arc::clone(&admitted);
            Ok(BatchOutcome::Completed(
                self.section(
                    StoreStep::run(run_id, "agent.tool.complete_batch"),
                    StoreAccess::Write,
                    move || {
                        Ok(service.journal.lock()?.database.unit_of_work(|tx| {
                            history::complete_batch(
                                tx,
                                batch.batch.tool_batch_id,
                                UtcTimestamp::now(),
                            )
                        })?)
                    },
                )
                .await?,
            ))
        }
        .await;
        // A bookkeeping/worker failure must not drop JoinSet and abort other
        // backend futures while reporting that cancellation has completed.
        if result.is_err() {
            cancel_sender.send_replace(true);
            while running.join_next().await.is_some() {}
        }
        forward.abort();
        result
    }
    /// Contract: takes `journal.lock()` directly. Reached only through
    /// [`Self::refuse_unit_async`], which runs it inside one bounded section.
    fn refuse_unit(
        &self,
        admitted: &AdmittedBatch,
        unit: &DispatchUnit,
        reason: ToolNotExecutedReasonV1,
    ) -> anyhow::Result<Vec<ToolResultV1>> {
        Ok(self.journal.lock()?.database.unit_of_work(|tx| {
            unit.calls
                .iter()
                .map(|id| {
                    tool::result::not_executed(
                        tx,
                        admitted.batch.tool_batch_id,
                        *id,
                        reason,
                        UtcTimestamp::now(),
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })?)
    }
    /// Contract: takes `journal.lock()` directly. Reached only through
    /// [`Self::refuse_with_notice_async`], which runs it inside one bounded section.
    fn refuse_with_notice(
        &self,
        admitted: &AdmittedBatch,
        unit: &DispatchUnit,
        reason: ToolNotExecutedReasonV1,
        notice: &str,
    ) -> anyhow::Result<Vec<ToolResultV1>> {
        Ok(self.journal.lock()?.database.unit_of_work(|tx| {
            unit.calls
                .iter()
                .map(|id| {
                    tool::result::not_executed_with_notice(
                        tx,
                        admitted.batch.tool_batch_id,
                        *id,
                        reason,
                        Some(notice),
                        UtcTimestamp::now(),
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })?)
    }
    async fn execute_unit(
        self: &Arc<Self>,
        admitted: Arc<AdmittedBatch>,
        unit: DispatchUnit,
        commands: Arc<crate::command_execution::CommandExecution>,
        cancel: watch::Receiver<bool>,
    ) -> anyhow::Result<Vec<(ToolCallId, CallOutcome)>> {
        let first = *unit.calls.first().ok_or_else(|| anyhow::anyhow!("agent.empty_unit"))?;
        let call =
            admitted.calls.get(&first).ok_or_else(|| anyhow::anyhow!("agent.unit_call_missing"))?;
        let guard_stopped = {
            // Boundary: the turn-guard read, one bounded section per unit.
            let run_id = admitted.batch.invocation_identity.agent_run_id;
            let service = Arc::clone(self);
            let batch = Arc::clone(&admitted);
            self.section(
                StoreStep::run(run_id, "agent.tool.guard_blocked"),
                StoreAccess::Read,
                move || {
                    let journal = service.journal.lock()?;
                    let turn = aura_persistence_v2::agent::turns::load(
                        journal.database.connection(),
                        batch.batch.invocation_identity.turn_task_id,
                    )?;
                    Ok(aura_persistence_v2::agent::guard::blocked(
                        journal.database.connection(),
                        turn.turn_task_id,
                    )?
                    .is_some())
                },
            )
            .await?
        };
        // An accepted addition belongs to the next model request after this
        // batch settles. It does not cancel the batch the model already started.
        if guard_stopped {
            return Ok(self
                .refuse_unit_async(
                    Arc::clone(&admitted),
                    unit.clone(),
                    ToolNotExecutedReasonV1::CancelledBeforeDispatch,
                )
                .await?
                .into_iter()
                .map(|r| (r.tool_call_id, CallOutcome::Terminal(Box::new(r))))
                .collect());
        }
        let mode_covers = self.persistence.with_session_permission_authority(
            call.snapshot.session_id,
            |mode| {
                if mode.mode == call.snapshot.mode
                    && mode.revision == call.snapshot.session_permission_revision
                {
                    Ok(true)
                } else {
                    self.operation_permitted_now(
                        &call.operation,
                        &call.snapshot,
                        mode.mode,
                        mode.revision,
                    )
                }
            },
        )??;
        if !mode_covers {
            return Ok(vec![(first, CallOutcome::Denied(PolicyDenyReason::ModeDenied))]);
        }
        if operation::is_builtin(&call.validated.definition)
            && operation::entry(&call.validated)?.handler.is_control()
        {
            return self.execute_control(&admitted, &unit.calls, cancel).await;
        }
        anyhow::ensure!(unit.calls.len() == 1, "agent.unrecognized_atomic_group");
        let plan = admitted
            .planned
            .execution_plan
            .calls
            .iter()
            .find(|v| v.tool_call_id == first)
            .ok_or_else(|| anyhow::anyhow!("agent.plan_call_missing"))?;
        let runtime = admitted
            .planned
            .runtime_calls
            .iter()
            .find(|v| v.validated_call.call.tool_call_id == first)
            .ok_or_else(|| anyhow::anyhow!("agent.runtime_call_missing"))?;
        let candidates = if matches!(
            call.operation.content.action_kind.as_str(),
            "shell.powershell" | "shell.cmd"
        ) {
            // Calls wait before selection; cancellation remains independent of permissions.
            let mut preparation_cancel = cancel.clone();
            tokio::select! {
                _ = commands.wait_until_ready() => {},
                _ = preparation_cancel.wait_for(|value| *value) => {},
            }
            commands.candidates_for(call.snapshot.mode)
        } else if call.operation.content.action_kind.starts_with("terminal.") {
            commands.wait_until_ready().await?;
            vec![terminal::backend_with_policy(
                self.clone(),
                call,
                commands.clone(),
                Some((plan.timeout_ms, cancel.clone())),
            )?]
        } else if call.operation.content.action_kind == "shell.stop" {
            vec![self.shell_control_backend(call, commands.clone())?]
        } else if package_tools::program::is_program(&call.operation.content) {
            vec![Arc::new(package_tools::program::PackageProgramBackend {
                descriptor: package_tools::program::native_descriptor(self.runtime),
                port: broker::Port {
                    service: self.clone(),
                    call: call.clone(),
                    commands: Some(commands.clone()),
                },
            }) as Arc<dyn aura_backend_api::ExecutionBackend>]
        } else {
            vec![self.broker_backend(call)?]
        };
        Ok(vec![(
            first,
            self.execute_call(call.clone(), plan, runtime.generation.clone(), &candidates, cancel)
                .await?,
        )])
    }
    /// One bounded section that records "this unit's calls were not executed".
    ///
    /// The synchronous `refuse_unit` below takes `journal.lock()` directly, so it is only
    /// correct on a real blocking thread. This wrapper is the only way the run path reaches it, and
    /// it hands the whole section to the one bounded Agent Store owner.
    async fn refuse_unit_async(
        self: &Arc<Self>,
        admitted: Arc<AdmittedBatch>,
        unit: DispatchUnit,
        reason: ToolNotExecutedReasonV1,
    ) -> anyhow::Result<Vec<ToolResultV1>> {
        let run_id = admitted.batch.invocation_identity.agent_run_id;
        let service = Arc::clone(self);
        self.section(
            StoreStep::run(run_id, "agent.tool.refuse_unit"),
            StoreAccess::Write,
            move || service.refuse_unit(&admitted, &unit, reason),
        )
        .await
    }

    /// The same, for the refusal that carries a user-facing notice.
    async fn refuse_with_notice_async(
        self: &Arc<Self>,
        admitted: Arc<AdmittedBatch>,
        unit: DispatchUnit,
        reason: ToolNotExecutedReasonV1,
        notice: String,
    ) -> anyhow::Result<Vec<ToolResultV1>> {
        let run_id = admitted.batch.invocation_identity.agent_run_id;
        let service = Arc::clone(self);
        self.section(
            StoreStep::run(run_id, "agent.tool.refuse_with_notice"),
            StoreAccess::Write,
            move || service.refuse_with_notice(&admitted, &unit, reason, &notice),
        )
        .await
    }

    /// One bounded section that classifies a unit from the durable ledger.
    async fn classify_unit_async(
        self: &Arc<Self>,
        admitted: Arc<AdmittedBatch>,
        unit: DispatchUnit,
        cancelled: bool,
        refusal: String,
    ) -> anyhow::Result<Vec<(ToolCallId, CallOutcome)>> {
        let run_id = admitted.batch.invocation_identity.agent_run_id;
        let service = Arc::clone(self);
        self.section(
            StoreStep::run(run_id, "agent.tool.classify_unit"),
            StoreAccess::Write,
            move || service.classify_unit(&admitted, &unit, cancelled, &refusal),
        )
        .await
    }

    /// Contract: takes `journal.lock()` directly and holds it across the whole ledger walk.
    /// Never call it from a reactive task; it is reached only through
    /// [`Self::classify_unit_async`], which runs it inside one bounded section.
    fn classify_unit(
        &self,
        admitted: &AdmittedBatch,
        unit: &DispatchUnit,
        cancelled: bool,
        refusal: &str,
    ) -> anyhow::Result<Vec<(ToolCallId, CallOutcome)>> {
        let mut store = self.journal.lock()?;
        let mut outcome = vec![];
        for id in &unit.calls {
            let call = tool::execution::call(store.database.connection(), *id)?;
            if let Some(result) = call.result {
                outcome.push((*id, CallOutcome::Terminal(Box::new(result))));
            } else if call.attempt_id.is_some() {
                let attempt = AttemptRef {
                    operation_id: admitted.calls[id].operation.operation_id,
                    ordinal: std::num::NonZeroU32::MIN,
                };
                // A durable terminal receipt is validated against the exact
                // stored definition. Only genuinely unclosed attempts remain
                // Unknown; no backend is dispatched again during recovery.
                if matches!(
                    aura_persistence_v2::execution::load(
                        store.database.connection(),
                        attempt.operation_id,
                        attempt.ordinal.get()
                    )?
                    .state,
                    AttemptState::Terminal { .. }
                ) {
                    let result = store.database.unit_of_work(|tx| {
                        tool::result::materialize(tx, admitted.batch.tool_batch_id, *id)
                    })?;
                    outcome.push((*id, CallOutcome::Terminal(Box::new(result))));
                } else {
                    outcome.push((*id, CallOutcome::Unknown(attempt)));
                }
            } else {
                let validated = &admitted.calls[id].validated;
                let name = if !operation::is_builtin(&validated.definition) {
                    validated.definition.canonical_name.as_str()
                } else {
                    operation::entry(validated)?.handler.name()
                };
                let reason = if cancelled {
                    ToolNotExecutedReasonV1::CancelledBeforeDispatch
                } else if name.starts_with("task_lineage.") {
                    ToolNotExecutedReasonV1::LineageReconciliation
                } else if unit.atomic_group.is_some() {
                    ToolNotExecutedReasonV1::AtomicGroupNotExecuted
                } else {
                    ToolNotExecutedReasonV1::BatchRejected
                };
                // This call never reached a backend, so it changed nothing; the model is told why
                // instead of a bare refusal. A cancellation explains itself.
                let notice = (!cancelled).then_some(refusal);
                let result = store.database.unit_of_work(|tx| {
                    tool::result::not_executed_with_notice(
                        tx,
                        admitted.batch.tool_batch_id,
                        *id,
                        reason,
                        notice,
                        UtcTimestamp::now(),
                    )
                })?;
                outcome.push((*id, CallOutcome::Terminal(Box::new(result))));
            }
        }
        Ok(outcome)
    }
}

/// What the model is told about calls a dispatch error left unexecuted. It quotes the error's top-level text, bounded.
fn refusal_notice(error: &str) -> String {
    let summary =
        error.chars().map(|c| if c.is_control() { ' ' } else { c }).take(240).collect::<String>();
    format!(
        "tool.dispatch_refused: Aura refused this call before running it ({summary}). Nothing \
         was changed. Check the current state before calling again."
    )
}
