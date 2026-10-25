//! The Store completes the turn, lineage, run and active projection together.
//! The caller must already have committed the current Tool results in this same
//! transaction. A rollback therefore removes every success fact together.
use super::{
    application, desktop_messages, facts, inbox, lineage, session, source_coverage, turns, work,
};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::{NormalizedModelOutput, NormalizedStopReason};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_tool_contract::*;
use rusqlite::{Connection, Transaction};

/// A Normal turn ended by the model's own final response.
///
/// Every precondition is read and re-checked inside the caller's transaction and
/// the shared terminal transaction then commits everything together; nothing
/// fabricates a completion tool call, tool result or user message. `Ok(None)`
/// means "not eligible", so the existing protocol reminder/stop protection still
/// governs that turn: truncation, an interrupted stream, a refusal, an empty
/// answer, pending Tool calls, a formal wait or an unconsumed accepted addition
/// never completes a turn here.
pub fn natural_from_response(
    tx: &Transaction<'_>,
    output: &NormalizedModelOutput,
    at: UtcTimestamp,
) -> StoreResult<Option<NaturalCompletionRecord>> {
    natural(tx, output, at, true)
}

/// Whether this response would end the turn now, without ending it, so a
/// `turn.before_complete` Hook can be asked first.
pub fn natural_eligible(
    tx: &Transaction<'_>,
    output: &NormalizedModelOutput,
    at: UtcTimestamp,
) -> StoreResult<bool> {
    Ok(natural(tx, output, at, false)?.is_some())
}

fn natural(
    tx: &Transaction<'_>,
    output: &NormalizedModelOutput,
    at: UtcTimestamp,
    commit: bool,
) -> StoreResult<Option<NaturalCompletionRecord>> {
    if output.stop_reason != NormalizedStopReason::EndTurn
        || !output.tool_calls.is_empty()
        || output.refusal.is_some()
    {
        return Ok(None);
    }
    let step = turns::step(tx, output.model_step_id)?;
    let ModelStepState::Completed { output_artifact_id, tool_batch_id: None, .. } = step.state
    else {
        return Ok(None);
    };
    let run = turns::load_run(tx, step.agent_run_id)?;
    let turn = turns::load(tx, run.turn_task_id)?;
    // Only an adjudicated Normal turn ends this way; Plan/Goal keep their own
    // explicit completion tools, and an unresolved mode decision is never skipped.
    if turn.mode != AgentTaskMode::Normal
        || turn.lineage_resolution_required
        || run.parent_agent_run_id.is_some()
        || run.state != AgentRunState::PreparingContext
        || turn.state.terminal()
        || application::cancellation_requested(tx, run.agent_run_id)?
        || inbox::pending_additions(tx, &turn)?
        || matches!(
            turn.state,
            TurnTaskState::AwaitingQuery { .. } | TurnTaskState::AwaitingApproval
        )
        || matches!(
            run.state,
            AgentRunState::AwaitingQuery { .. }
                | AgentRunState::AwaitingApproval { .. }
                | AgentRunState::AwaitingContextRemediation { .. }
        )
        || super::mode::load(tx, turn.session_id)?
            .is_some_and(|episode| episode.is_active_special() || episode.disposition_required)
    {
        return Ok(None);
    }
    let previous = lineage::load(tx, turn.lineage_id)?;
    if previous.state != TaskLineageState::Active
        || previous.last_active_turn_task_id != turn.turn_task_id
    {
        return Ok(None);
    }
    // The response must have covered every source this turn already accepted, so
    // an addition accepted after the request was built can never be absorbed into
    // an older answer.
    let execution = crate::model::step_snapshot(tx, step.model_step_id)?;
    let Some(snapshot) = crate::context::snapshot(tx, execution.context_snapshot_id)? else {
        return Ok(None);
    };
    for source in
        previous.sources.iter().filter(|source| source.membership == SourceMembership::Active)
    {
        if !source_coverage::source_covered(tx, &snapshot, source)? {
            return Ok(None);
        }
    }
    // The full user-visible answer, joined exactly like the desktop projection;
    // Reasoning is not an answer and a bounded preview is never the final report.
    let (final_report, _, _) = desktop_messages::sealed_text(tx, step.model_step_id)?;
    if final_report.trim().is_empty() {
        return Ok(None);
    }
    let record = NaturalCompletionRecord {
        record_id: NaturalCompletionRecordId::new(),
        turn_task_id: turn.turn_task_id,
        agent_run_id: run.agent_run_id,
        lineage_id: previous.lineage_id,
        model_step_id: step.model_step_id,
        output_artifact_id,
        basis: NaturalCompletionBasis::EndTurn,
        final_report: final_report.clone(),
        at,
    };
    if !commit {
        return Ok(Some(record));
    }
    facts::append(
        tx,
        EventId::from_bytes(record.record_id.to_bytes()),
        &turn.turn_task_id.to_string(),
        "agent.natural_completed",
        &record,
        at,
    )?;
    commit_terminal(tx, &turn, &run, &previous, &final_report, at)?;
    Ok(Some(record))
}

pub fn normal(
    tx: &Transaction<'_>,
    step_id: ModelStepId,
    batch_id: ToolBatchId,
    input: &NormalCompleteInput,
    same_batch: bool,
    at: UtcTimestamp,
) -> StoreResult<NormalCompletionRecord> {
    normal_with_id(tx, step_id, batch_id, input, same_batch, NormalCompletionRecordId::new(), at)
}
#[allow(clippy::too_many_arguments)] // Private atomic control transaction supplies a fixed receipt identity.
pub(super) fn normal_with_id(
    tx: &Transaction<'_>,
    step_id: ModelStepId,
    batch_id: ToolBatchId,
    input: &NormalCompleteInput,
    same_batch: bool,
    record_id: NormalCompletionRecordId,
    at: UtcTimestamp,
) -> StoreResult<NormalCompletionRecord> {
    let (turn, run) =
        completion_authority(tx, step_id, batch_id, "normal.complete", input, same_batch)?;
    let previous = lineage::load(tx, turn.lineage_id)?;
    // An optional review in this exact atomic pair advances the revision once.
    // Validate its actual input rather than accepting a stale model revision.
    let mut checked = input.clone();
    if same_batch {
        let batch = crate::tool::execution::batch(tx, batch_id)?;
        let registry =
            crate::tool::registry(tx, batch.batch.registry_snapshot_ref.tool_registry_snapshot_id)?
                .ok_or(Error::Conflict)?;
        let review_call = batch
            .batch
            .calls
            .iter()
            .find(|call| {
                registry.catalog_entries.iter().any(|entry| {
                    call.definition_ref.as_ref().is_some_and(|r| entry.tool_id == r.tool_id)
                        && entry.canonical_name.as_str() == "task_lineage.review_completion"
                })
            })
            .ok_or(Error::Conflict)?;
        let NormalizedToolCallInputStateV1::Ready { arguments, .. } = &review_call.input else {
            return Err(Error::Conflict);
        };
        let review_input: TaskLineageReviewCompletionInput =
            serde_json::from_value(arguments.clone())?;
        if review_input.expected_lineage_revision != input.expected_lineage_revision
            || input.expected_lineage_revision.value().checked_add(1)
                != Some(previous.revision.value())
        {
            return Err(Error::Conflict);
        }
        checked.expected_lineage_revision = previous.revision;
    }
    let outcome = previous
        .check_normal_completion(
            &checked,
            &CompletionContext { turn: &turn, run: &run, model_step_id: step_id, at },
        )
        .map_err(|_| Error::Invalid("Normal completion gate"))?;
    let review_id = match &previous.completion_gate {
        CompletionGate::ReviewedReady { review_id, .. }
        | CompletionGate::ReviewedNotReady { review_id, .. }
            if same_batch =>
        {
            Some(*review_id)
        }
        _ => None,
    };
    let record = NormalCompletionRecord {
        record_id,
        turn_task_id: turn.turn_task_id,
        agent_run_id: run.agent_run_id,
        lineage_id: previous.lineage_id,
        review_id,
        outcome,
        final_report: input.final_report.clone(),
        disclosed_limitations: input.disclosed_limitations.clone(),
        at,
    };
    facts::append(
        tx,
        EventId::from_bytes(record.record_id.to_bytes()),
        &turn.turn_task_id.to_string(),
        "agent.normal_completed",
        &record,
        at,
    )?;
    commit_terminal(tx, &turn, &run, &previous, &input.final_report, at)?;
    Ok(record)
}
pub fn plan(
    tx: &Transaction<'_>,
    step_id: ModelStepId,
    batch_id: ToolBatchId,
    input: &PlanCompleteInput,
    at: UtcTimestamp,
) -> StoreResult<PlanRecord> {
    let (turn, run) = completion_authority(tx, step_id, batch_id, "plan.complete", input, false)?;
    input.validate().map_err(|_| Error::Invalid("Plan completion"))?;
    if turn.mode != AgentTaskMode::Plan || turn.revision != input.expected_turn_revision {
        return Err(Error::Conflict);
    }
    let lineage = lineage::load(tx, turn.lineage_id)?;
    let user = session::user_message(tx, turn.session_id, turn.root_user_event_id)?;
    let plan = PlanRecord {
        turn_task_id: turn.turn_task_id,
        revision: Revision::INITIAL,
        original_user_goal: user.text,
        plan: input.clone(),
        at,
    };
    facts::append(
        tx,
        EventId::new(),
        &turn.turn_task_id.to_string(),
        "agent.plan_completed",
        &plan,
        at,
    )?;
    let report = serde_json::to_string_pretty(&plan)?;
    commit_terminal(tx, &turn, &run, &lineage, &report, at)?;
    end_mode(tx, turn.session_id, ModeEpisodeEndReason::PlanCompleted, at)?;
    Ok(plan)
}
pub fn goal(
    tx: &Transaction<'_>,
    step_id: ModelStepId,
    batch_id: ToolBatchId,
    input: &GoalCompleteInput,
    at: UtcTimestamp,
) -> StoreResult<GoalState> {
    let (turn, run) = completion_authority(tx, step_id, batch_id, "goal.complete", input, false)?;
    if turn.mode != AgentTaskMode::Goal {
        return Err(Error::Conflict);
    }
    let previous = work::goal(tx, turn.turn_task_id)?;
    previous.check_completion(input).map_err(|_| Error::Invalid("Goal completion gate"))?;
    super::evidence::verify(tx, &input.evidence_refs, &turn.agent_run_id.to_string())?;
    let mut next = previous.clone();
    next.completed = true;
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    work::complete_goal(tx, &previous, &next, at)?;
    facts::append(
        tx,
        EventId::new(),
        &turn.turn_task_id.to_string(),
        "agent.goal_completed",
        input,
        at,
    )?;
    commit_terminal(
        tx,
        &turn,
        &run,
        &lineage::load(tx, turn.lineage_id)?,
        &input.final_report,
        at,
    )?;
    end_mode(tx, turn.session_id, ModeEpisodeEndReason::GoalCompleted, at)?;
    Ok(next)
}
fn end_mode(
    tx: &Transaction<'_>,
    session: SessionId,
    reason: ModeEpisodeEndReason,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if super::mode::load(tx, session)?.is_some_and(|episode| episode.is_active_special()) {
        super::mode::end(tx, session, reason, at)?;
    }
    Ok(())
}
fn completion_authority<T>(
    db: &Connection,
    step: ModelStepId,
    batch: ToolBatchId,
    name: &str,
    input: &T,
    same_batch: bool,
) -> StoreResult<(TurnTask, AgentRun)>
where
    T: serde::de::DeserializeOwned + PartialEq,
{
    let step = turns::step(db, step)?;
    if !matches!(step.state, ModelStepState::Completed { tool_batch_id: Some(id), .. } if id == batch)
    {
        return Err(Error::Conflict);
    }
    let run = turns::load_run(db, step.agent_run_id)?;
    let turn = turns::load(db, run.turn_task_id)?;
    if super::inbox::pending_additions(db, &turn)? {
        return Err(Error::Invalid("agent.pending_addition"));
    }
    if run.parent_agent_run_id.is_some()
        || run.state != (AgentRunState::ToolsRunning { tool_batch_id: batch })
        || turn.state.terminal()
        || turn.lineage_resolution_required
    {
        return Err(Error::Conflict);
    }
    let batch = crate::tool::execution::batch(db, batch)?;
    let result = batch.result.as_ref().ok_or(Error::Conflict)?;
    let history = crate::context::history(db, turn.session_id)?;
    if !history.iter().any(|group| {
        group.tool_batch_id == batch.batch.tool_batch_id
            && group.model_step_id == step.model_step_id
            && group.agent_run_id == run.agent_run_id
            && aura_domain_common::jcs::canonical_sha256(result)
                .is_ok_and(|hash| hash == group.batch_summary_hash)
    }) {
        return Err(Error::Conflict);
    }
    if batch.batch.invocation_identity.session_id != turn.session_id
        || batch.batch.invocation_identity.turn_task_id != turn.turn_task_id
        || batch.batch.invocation_identity.agent_run_id != run.agent_run_id
        || batch.batch.invocation_identity.model_step_id != step.model_step_id
    {
        return Err(Error::Conflict);
    }
    let registry =
        crate::tool::registry(db, batch.batch.registry_snapshot_ref.tool_registry_snapshot_id)?
            .ok_or(Error::Conflict)?;
    let exact_entry = |call: &&NormalizedToolCallV1| {
        registry.catalog_entries.iter().find(|entry| {
            call.definition_ref
                .as_ref()
                .is_some_and(|r| entry.tool_id == r.tool_id && entry.tool_version == r.tool_version)
                && entry.source == ToolSourceKindV1::BuiltIn
        })
    };
    let mut completion = batch.batch.calls.iter().filter(|call| {
        exact_entry(call).is_some_and(|entry| entry.canonical_name.as_str() == name)
    });
    let call = completion.next().ok_or(Error::Conflict)?;
    if completion.next().is_some()
        || !result.results.iter().any(|r| {
            r.tool_call_id == call.tool_call_id && r.outcome == ToolResultOutcomeV1::Succeeded
        })
    {
        return Err(Error::Conflict);
    }
    // The admitted arguments, read as the same type. An optional field the model left out is the
    // same input with its default, not another call: in an evaluation baseline, normal.complete without
    // disclosed_limitations was refused here as a conflict.
    if !matches!(&call.input, NormalizedToolCallInputStateV1::Ready { arguments, .. }
        if serde_json::from_value::<T>(arguments.clone()).is_ok_and(|admitted| admitted == *input))
    {
        return Err(Error::Conflict);
    }
    let names = batch
        .batch
        .calls
        .iter()
        .map(|call| exact_entry(&call).map(|entry| entry.canonical_name.as_str()))
        .collect::<Option<std::collections::BTreeSet<_>>>()
        .ok_or(Error::Conflict)?;
    if same_batch {
        if batch.batch.calls.len() != 2
            || names
                != std::collections::BTreeSet::from([
                    "normal.complete",
                    "task_lineage.review_completion",
                ])
        {
            return Err(Error::Conflict);
        }
    } else if batch.batch.calls.len() != 1 {
        return Err(Error::Conflict);
    }
    Ok((turn, run))
}
fn commit_terminal(
    tx: &Transaction<'_>,
    turn: &TurnTask,
    run: &AgentRun,
    previous: &TaskLineage,
    report: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let session = session::load(tx, turn.session_id)?;
    if session.active_turn != Some(turn.turn_task_id)
        || session.active_lineage != Some(previous.lineage_id)
        || previous.state != TaskLineageState::Active
    {
        return Err(Error::Conflict);
    }
    let mut next_lineage = previous.clone();
    next_lineage.advance_revision(previous.revision, at).map_err(|_| Error::Conflict)?;
    next_lineage.state = TaskLineageState::Completed;
    next_lineage.completed_at = Some(at);
    lineage::save(tx, previous, &next_lineage)?;
    let mut next_turn = turn.clone();
    next_turn
        .transition(turn.revision, TurnTaskState::Completed { final_report: report.into() }, at)
        .map_err(|_| Error::Conflict)?;
    turns::save_completed(tx, turn, &next_turn)?;
    facts::append(
        tx,
        EventId::new(),
        &turn.turn_task_id.to_string(),
        "agent.completion_digest",
        &serde_json::json!({
            "digest": ContentHash::blake3(report.as_bytes()).to_string(),
            "total_bytes": report.len() as u64,
        }),
        at,
    )?;
    let mut next_run = run.clone();
    next_run.state = AgentRunState::Completed;
    next_run.revision = Revision::new(run.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    next_run.updated_at = at;
    turns::save_run(tx, run, &next_run)?;
    crate::context::manual_compaction::cancel_run(
        tx,
        run.agent_run_id,
        "context.task_completed_before_compaction",
        at,
    )?;
    let mut next_session = session.clone();
    next_session.active_turn = None;
    next_session.active_lineage = None;
    next_session.open_lineages.retain(|id| *id != previous.lineage_id);
    next_session.revision =
        Revision::new(session.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    session::save(tx, &session, &next_session, at)?;
    if turn.mode == AgentTaskMode::Goal {
        let _ = super::mode::snapshot_goal(tx, &turns::load(tx, turn.turn_task_id)?, at);
    }
    Ok(())
}
