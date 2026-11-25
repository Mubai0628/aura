use super::*;
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
        let (prepared, exact_values) = {
            let service = Arc::clone(self);
            let batch = admitted.batch.clone();
            let run_id = admitted.batch.invocation_identity.agent_run_id;
            let tools: Vec<String> = call_ids
                .iter()
                .filter_map(|id| admitted.calls.get(id))
                .map(|call| call.operation.content.action_kind.clone())
                .collect();
            self.section(
                StoreStep::run(run_id, "agent.control.preflight"),
                StoreAccess::Read,
                move || {
                    // A completion gate's refusal becomes the line the model reads; a store
                    // failure keeps its type, so the section still classifies it.
                    let journal = service.journal.lock()?;
                    let db = journal.database.connection();
                    let ready = control::prepare(db, &batch, UtcTimestamp::now())
                        .map_err(|error| preflight_refusal(&tools, error))?;
                    let values = if tools.iter().any(|name| name == "normal.complete") {
                        use aura_persistence_v2::agent::{exact_artifacts, turns};
                        exact_artifacts::requirements(
                            db,
                            &turns::load(db, batch.invocation_identity.turn_task_id)?,
                        )?
                    } else {
                        Vec::new()
                    };
                    Ok((ready, values))
                },
            )
            .await?
        };
        if let Some(value) =
            self.unverified_delivery_value(admitted.snapshot.workspace_id, exact_values).await?
        {
            anyhow::bail!(
                "normal.complete: 未核对到交付文件「{}」中用户要求逐字保留的内容。{}",
                value.path.as_str(),
                value.formatting_hint()
            );
        }
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
                .map_err(super::dispatch::backend_failure)?
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
            .map_err(super::dispatch::backend_failure)?;
        let mut reservations = BTreeMap::new();
        for (id, selected) in selected_calls {
            reservations.insert(
                id,
                selected
                    .into_control_reservation()
                    .await
                    .map_err(super::dispatch::backend_failure)?,
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

/// The completion gates' refusals, which name only the gate: what each gate requires.
const REVIEW_REFUSED: &str = "task_lineage.review_completion: the review does not match this \
                              turn's task: lineage_id and expected_lineage_revision must be the \
                              current lineage's, a source_read_receipt_id must come from \
                              task_lineage.read_sources earlier in this turn, and reviewed \
                              intents may cite only that lineage's user messages. Nothing was \
                              changed. Reload task_lineage.list_open, then review again.";
const NORMAL_REFUSED: &str = "normal.complete: the call does not match this turn's task: \
                              lineage_id and expected_lineage_revision must be the current \
                              lineage's, and final_report must not be blank. Nothing was \
                              changed. Reload task_lineage.list_open, then call normal.complete \
                              again with its current ids.";
const PLAN_REFUSED: &str = "plan.complete: the plan is not well formed: each step needs a \
                            unique, non-blank id and a description; depends_on may name only \
                            other steps, each once and without a cycle; a parallel group needs a \
                            name; expected_turn_revision must be above 0. Nothing was changed. \
                            Correct the plan, then call plan.complete again.";
const GOAL_REFUSED: &str = "goal.complete: the goal cannot be completed as called: \
                            expected_goal_revision must be the goal's current revision, the goal \
                            must still be open, and final_report must not be blank. Nothing was \
                            changed. Read the goal with goal.get, then call goal.complete again \
                            if it is still open.";

/// Why the completion group was refused before anything started, as the first line the model
/// reads. A refusal already worded as `code: sentence` is kept; a store failure keeps its type
/// under the step's own context, so the store still classifies it.
fn preflight_refusal(tools: &[String], error: Error) -> anyhow::Error {
    let line = match error {
        Error::Invalid(text) if text.contains(": ") => text,
        Error::Invalid("completion review") => REVIEW_REFUSED,
        Error::Invalid("Normal completion identity") => NORMAL_REFUSED,
        Error::Invalid("Plan completion") => PLAN_REFUSED,
        Error::Invalid("Goal completion gate") => GOAL_REFUSED,
        Error::Conflict => changed(tools),
        other => return anyhow::Error::new(other).context("agent.control.preflight"),
    };
    anyhow::Error::msg(line)
}

/// A revision or identity conflict in the completion group, for the calls it holds. Admission
/// lets plan.complete and goal.complete only alone, and normal.complete only with
/// task_lineage.review_completion.
fn changed(tools: &[String]) -> &'static str {
    let has = |name: &str| tools.iter().any(|tool| tool == name);
    if has("plan.complete") {
        "plan.complete: this turn changed while the call was checked (for example, the user added \
         a message). Nothing was changed. Read what changed, then call plan.complete again with \
         the turn's current revision if the plan still stands."
    } else if has("goal.complete") {
        "goal.complete: this turn changed while the call was checked (for example, the user added \
         a message), or its goal is no longer open. Nothing was changed. Read the goal with \
         goal.get, then call goal.complete again if it is still open."
    } else if has("normal.complete") {
        "normal.complete: this turn's task changed while the call was checked. With \
         task_lineage.review_completion in the same response, expected_lineage_revision must be \
         the revision before that review. Nothing was changed. Reload task_lineage.list_open, \
         then call again with its current ids."
    } else {
        "task_lineage.review_completion: this turn's task changed while the review was checked. \
         Nothing was changed. Reload task_lineage.list_open, then review again with its current \
         ids."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refused completion names the tool and what its gate requires on the first line the model
    /// reads, and says that nothing was changed; a family's own sentence is kept as it is, and a
    /// store failure keeps its type.
    #[test]
    fn a_refused_completion_names_its_gate_first() {
        let none: &[String] = &[];
        let review = ["task_lineage.review_completion".to_owned(), "normal.complete".to_owned()];
        let goal = ["goal.complete".to_owned()];
        let plan = ["plan.complete".to_owned()];
        for (tools, error, tool) in [
            (none, Error::Invalid("completion review"), "task_lineage.review_completion: "),
            (none, Error::Invalid("Normal completion identity"), "normal.complete: "),
            (none, Error::Invalid("Plan completion"), "plan.complete: "),
            (none, Error::Invalid("Goal completion gate"), "goal.complete: "),
            (&review[..], Error::Conflict, "normal.complete: "),
            (&review[..1], Error::Conflict, "task_lineage.review_completion: "),
            (&goal[..], Error::Conflict, "goal.complete: "),
            (&plan[..], Error::Conflict, "plan.complete: "),
        ] {
            let line = preflight_refusal(tools, error).to_string();
            assert!(line.starts_with(tool), "{line}");
            assert!(line.contains(" Nothing was changed. "), "{line}");
            assert!(line.len() <= 512 && !line.contains(['{', '}', '\n']), "{line}");
        }
        let cited = "tool.arguments_invalid: an id in these arguments is not one Aura gave you; \
                     copy ids exactly from the task state";
        assert_eq!(preflight_refusal(none, Error::Invalid(cited)).to_string(), cited);
        let store = preflight_refusal(none, Error::Invalid("Agent state hash"));
        assert!(store.downcast_ref::<Error>().is_some(), "{store}");
    }

    /// The store's own sentence for a completion refused by the turn's mode or its newest message
    /// reaches the model as it is.
    #[test]
    fn a_completion_refused_for_the_turns_state_says_which_state() {
        let plan = ["plan.complete".to_owned()];
        let worded = "plan.complete: this turn is not in plan mode, so there is no plan to \
                      complete. Nothing was changed.";
        assert_eq!(preflight_refusal(&plan, Error::Invalid(worded)).to_string(), worded);
        let changed = preflight_refusal(&plan, Error::Conflict).to_string();
        assert!(changed.contains("the user added a message"), "{changed}");
    }
}
