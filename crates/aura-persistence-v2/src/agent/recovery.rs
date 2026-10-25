//! Stop facts, paired refusals and restart diagnosis. No operation is retried.
use super::{application, approval, facts, history, model_context, session, turns, work};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_execution_contract::AttemptState;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact};
use aura_tool_contract::ToolNotExecutedReasonV1;
use rusqlite::{Connection, Transaction};

/// The runs are read first and each one's configuration is looked up by its owner. Joined
/// the other way, SQLite read every event of the journal for its kind at each start (1.5 GB on the
/// owner's journal before Core was ready).
pub fn active_runs(db: &Connection) -> StoreResult<Vec<AgentRunId>> {
    let mut q = db.prepare(
        "SELECT r.id FROM agent_runs r \
         WHERE r.state NOT IN('completed','cancelled','recovery_required') \
         AND EXISTS(SELECT 1 FROM events e WHERE e.owner=r.id AND e.kind='agent.run_configuration') \
         ORDER BY r.id",
    )?;
    q.query_map([], |r| r.get::<_, String>(0))?
        .map(|v| v?.parse().map_err(|_| Error::Conflict))
        .collect()
}
pub fn startup(tx: &Transaction<'_>, at: UtcTimestamp) -> StoreResult<()> {
    crate::execution::shell::runtime_restarted(tx, at)?;
    for id in active_runs(tx)? {
        // One run that cannot be settled no longer keeps every other session, and the
        // whole Agent, from starting. Its partial writes are taken back and why is kept; the next
        // start, or 停止, tries it again.
        tx.execute_batch("SAVEPOINT aura_startup_run")?;
        match settle_after_restart(tx, id, at) {
            Ok(()) => tx.execute_batch("RELEASE aura_startup_run")?,
            Err(error) => {
                tx.execute_batch("ROLLBACK TO aura_startup_run; RELEASE aura_startup_run")?;
                let reason: String = error.to_string().chars().take(300).collect();
                facts::append(
                    tx,
                    EventId::new(),
                    &id.to_string(),
                    "agent.restart_settlement_deferred",
                    &reason,
                    at,
                )?;
            }
        }
    }
    Ok(())
}
fn settle_after_restart(tx: &Transaction<'_>, id: AgentRunId, at: UtcTimestamp) -> StoreResult<()> {
    let r = turns::load_run(tx, id)?;
    if !application::cancellation_requested(tx, id)?
        && matches!(
            r.state,
            AgentRunState::AwaitingApproval { .. }
                | AgentRunState::AwaitingQuery { .. }
                | AgentRunState::AwaitingContextRemediation { .. }
        )
    {
        return Ok(());
    }
    interrupt(tx, id, "agent.core_restarted", application::cancellation_requested(tx, id)?, at)
}
pub fn episodes(db: &Connection, id: AgentRunId) -> StoreResult<Vec<RecoveryEpisode>> {
    let mut q =
        db.prepare("SELECT payload FROM recovery_episodes WHERE agent_run_id=?1 ORDER BY ordinal")?;
    q.query_map([id.to_string()], |r| r.get::<_, String>(0))?
        .map(|v| Ok(serde_json::from_str(&v?)?))
        .collect()
}
/// Gives one call of an interrupted batch its result. A call whose result cannot be built made every interruption of its run fail, so
/// the turn could never end. Now such a call stays unanswered — an unresolved operation of the
/// turn, as a call still running is — and whatever building it had written is taken back.
fn answered(tx: &Transaction<'_>, batch: ToolBatchId, call: ToolCallId) -> StoreResult<bool> {
    tx.execute_batch("SAVEPOINT aura_interrupt_answer")?;
    let built = crate::tool::result::materialize(tx, batch, call).is_ok();
    tx.execute_batch(if built {
        "RELEASE aura_interrupt_answer"
    } else {
        "ROLLBACK TO aura_interrupt_answer; RELEASE aura_interrupt_answer"
    })?;
    Ok(built)
}

pub fn interrupt(
    tx: &Transaction<'_>,
    run_id: AgentRunId,
    reason: &str,
    cancelled: bool,
    at: UtcTimestamp,
) -> StoreResult<()> {
    interrupt_with_diagnostic(tx, run_id, reason, cancelled, at, None)
}

/// The same interruption, carrying bounded structured evidence for the failed
/// step.
///
/// The diagnostic is stored on the step's failure artifact, which is where a
/// later reader looks for why a step stopped. It is already bounded by the
/// layer that produced it: no Provider response body, argument or credential can
/// reach this record, and a caller without evidence passes `None`.
pub fn interrupt_with_diagnostic(
    tx: &Transaction<'_>,
    run_id: AgentRunId,
    reason: &str,
    cancelled: bool,
    at: UtcTimestamp,
    diagnostic: Option<&serde_json::Value>,
) -> StoreResult<()> {
    let old = turns::load_run(tx, run_id)?;
    let turn = turns::load(tx, old.turn_task_id)?;
    if turn.state.terminal() {
        return Ok(());
    }
    crate::model::recovery::interrupt_run(tx, run_id, at)?;
    crate::context::compaction::interrupt_run(tx, run_id, at)?;
    crate::context::remediation::clear(tx, run_id, true, at)?;
    crate::model::context_window::finish(tx, run_id, None, true, at)?;
    let mut unresolved = vec![];
    let mut not_executed = vec![];
    if let AgentRunState::ToolsRunning { tool_batch_id: batch }
    | AgentRunState::AwaitingApproval { tool_batch_id: batch } = old.state
    {
        if let Some(inputs) = crate::tool::admission::load(tx, batch)? {
            let stored = crate::tool::execution::batch(tx, batch)?;
            let mut all_terminal = true;
            for c in &stored.batch.calls {
                let call = crate::tool::execution::call(tx, c.tool_call_id)?;
                let operation = inputs.operations.get(&c.tool_call_id);
                if call.result.is_none() {
                    if call.attempt_id.is_none() {
                        crate::tool::result::not_executed(
                            tx,
                            batch,
                            c.tool_call_id,
                            if cancelled {
                                ToolNotExecutedReasonV1::CancelledBeforeDispatch
                            } else {
                                ToolNotExecutedReasonV1::RuntimeUnavailable
                            },
                            at,
                        )?;
                        not_executed.push(c.tool_call_id);
                    } else if let Some(operation) = operation {
                        let attempt = crate::execution::load(tx, operation.operation_id, 1)?;
                        if matches!(attempt.state, AttemptState::Terminal { .. }) {
                            if !answered(tx, batch, c.tool_call_id)? {
                                all_terminal = false;
                                unresolved.push(operation.operation_id);
                            }
                        } else if matches!(
                            operation.content.action_kind.as_str(),
                            "shell.powershell" | "shell.cmd"
                        ) {
                            crate::execution::shell::lost(tx, operation.operation_id, at)?;
                            all_terminal &= answered(tx, batch, c.tool_call_id)?;
                            unresolved.push(operation.operation_id);
                        } else {
                            all_terminal = false;
                            unresolved.push(operation.operation_id);
                        }
                    } else {
                        // An attempt without its admitted operation has no identity to settle;
                        // it stays unanswered and the turn still ends.
                        all_terminal = false;
                    }
                }
            }
            if all_terminal {
                history::complete_batch(tx, batch, at)?;
            }
        } else {
            // Provider output committed before admission. Preserve every actual
            // call ID as unexecuted; no identity or execution result is forged.
            let step=model_context::steps(tx,run_id)?.into_iter().find(|s|matches!(s.state,ModelStepState::Completed{tool_batch_id:Some(id),..} if id==batch)).ok_or(Error::Conflict)?;
            not_executed = history::output(tx, step.model_step_id)?
                .tool_calls
                .into_iter()
                .map(|c| c.tool_call_id)
                .collect();
        }
    }
    unresolved.sort();
    unresolved.dedup();
    for id in approval::pending(tx, run_id)? {
        crate::security_approval::close(
            tx,
            id,
            aura_security_contract::ApprovalState::Cancelled,
            at,
        )?;
    }
    if let Some(q) =
        work::query(tx, turn.turn_task_id)?.filter(|q| q.state == UserQueryState::Pending)
    {
        let mut next = q.clone();
        next.revision = Revision::new(q.revision.value() + 1);
        next.state = UserQueryState::Cancelled;
        work::save_query(tx, Some(&q), &next, at)?;
    }
    let mut evidence = vec![];
    for s in model_context::steps(tx, run_id)? {
        if matches!(s.state, ModelStepState::Preparing | ModelStepState::Running { .. }) {
            let mut next = s.clone();
            next.finished_at = Some(at);
            next.state = if cancelled {
                ModelStepState::Cancelled
            } else {
                let id = ArtifactId::new();
                // The step evidence names the reason and, when the caller has it,
                // the bounded structured diagnostic of the refusal that stopped
                // the step. `retry` stays explicit: nothing here replays a step.
                let mut evidence_payload = serde_json::json!({
                    "reason": reason,
                    "retry": "explicit_new_step_required",
                });
                if let Some(diagnostic) = diagnostic {
                    evidence_payload["diagnostic"] = diagnostic.clone();
                }
                artifact::put(
                    tx,
                    id,
                    &s.model_step_id.to_string(),
                    "application/json",
                    &serde_json::to_vec(&evidence_payload)?,
                    at,
                )?;
                evidence.push(id);
                ModelStepState::Failed { error_artifact_id: id }
            };
            turns::save_step(tx, &s, &next)?;
        }
    }
    // Explicit interruption is terminal even if its execution observation is
    // incomplete. The unresolved facts stay in the episode and Tool history.
    let can_cancel = cancelled;
    super::inbox::pause(tx, turn.session_id, at)?;
    let mut t = turn.clone();
    t.transition(
        turn.revision,
        if can_cancel {
            TurnTaskState::Cancelled
        } else {
            TurnTaskState::RecoveryRequired { reason: reason.into() }
        },
        at,
    )
    .map_err(|_| Error::Conflict)?;
    turns::save(tx, &turn, &t)?;
    let mut next = old.clone();
    next.revision = Revision::new(old.revision.value() + 1);
    next.updated_at = at;
    if can_cancel {
        next.state = AgentRunState::Cancelled;
    } else {
        let ordinal = episodes(tx, run_id)?
            .len()
            .checked_add(1)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or(Error::Conflict)?;
        let episode = RecoveryEpisode {
            agent_run_id: run_id,
            ordinal,
            reason: reason.into(),
            unresolved_operation_ids: unresolved,
            not_executed_tool_call_ids: not_executed,
            evidence_refs: evidence,
            created_at: at,
            resolved_at: None,
        };
        tx.execute(
            "INSERT INTO recovery_episodes VALUES(?1,?2,'unresolved',?3)",
            rusqlite::params![run_id.to_string(), ordinal, serde_json::to_string(&episode)?],
        )?;
        next.state = AgentRunState::RecoveryRequired { episode: ordinal };
    }
    turns::save_run(tx, &old, &next)?;
    facts::append(
        tx,
        EventId::new(),
        &run_id.to_string(),
        "agent.stopped",
        &(reason, cancelled),
        at,
    )?;
    let before = session::load(tx, turn.session_id)?;
    if !turn.lineage_resolution_required {
        let previous = super::lineage::load(tx, turn.lineage_id)?;
        if previous.state == TaskLineageState::Active
            && previous.last_active_turn_task_id == turn.turn_task_id
        {
            let mut next = previous.clone();
            next.state = TaskLineageState::InterruptedResumable;
            next.completion_gate = CompletionGate::Unreviewed;
            next.advance_revision(previous.revision, at).map_err(|_| Error::Conflict)?;
            super::lineage::save(tx, &previous, &next)?;
        }
    }
    let mut after = before.clone();
    after.active_turn = None;
    after.active_lineage = None;
    after.revision = Revision::new(before.revision.value() + 1);
    session::save(tx, &before, &after, at)?;
    if turn.mode == AgentTaskMode::Goal {
        let _ = super::mode::snapshot_goal(tx, &turns::load(tx, turn.turn_task_id)?, at);
    }
    Ok(())
}
