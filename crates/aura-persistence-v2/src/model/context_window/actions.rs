use super::*;

pub fn resolved(
    db: &Connection,
    command: CommandInvocationId,
    input: &AgentContextWindowResolveRequest,
) -> StoreResult<bool> {
    let raw: Option<String> = db.query_row(
        "SELECT payload FROM events WHERE owner=?1 AND kind='model.context_window_resolve' LIMIT 1",
        [command.to_string()], |r| r.get(0),
    ).optional()?;
    match raw {
        Some(raw) if serde_json::from_str::<AgentContextWindowResolveRequest>(&raw)? == *input => {
            Ok(true)
        }
        Some(_) => Err(Error::Conflict),
        None => Ok(false),
    }
}
pub fn resolve(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    input: &AgentContextWindowResolveRequest,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if resolved(tx, command, input)? {
        return Ok(());
    }
    let run = active(tx, input.agent_run_id)?;
    let mut record = latest(tx, input.agent_run_id)?.ok_or(Error::Conflict)?;
    if turns::load(tx, run.turn_task_id)?.session_id != input.session_id
        || record.view.revision != input.expected_revision
        || record.view.correction.model_step_id != input.source_model_step_id
        || record.view.state != AgentContextWindowState::Awaiting
        || run.revision != record.view.revision
        || run.state
            != (AgentRunState::AwaitingContextRemediation {
                model_step_id: input.source_model_step_id,
            })
    {
        return Err(Error::Conflict);
    }
    record.view.revision = next_revision(run.revision)?;
    record.view.updated_at = at;
    match input.action {
        AgentContextWindowAction::Recheck => {
            // A user-initiated recheck reads current P05 configuration; it never
            // silently continues a discarded automatic candidate chain.
            record.candidate_windows.clear();
            record.view.state = AgentContextWindowState::Preparing;
        }
        AgentContextWindowAction::TrySmallerWindows => {
            let budget = crate::context::snapshot(tx, record.failed_snapshot.context_snapshot_id)?
                .ok_or(Error::Conflict)?
                .budget;
            if !select_candidate(&mut record) {
                record.view.correction.resolution =
                    manual_resolution(&record, budget.estimated_input_tokens, true);
            }
        }
    }
    let mut next = run.clone();
    next.revision = record.view.revision;
    next.updated_at = at;
    if record.view.state == AgentContextWindowState::Preparing {
        next.state = AgentRunState::PreparingContext;
    }
    turns::save_run(tx, &run, &next)?;
    save(tx, &record, at)?;
    append(tx, &command.to_string(), "model.context_window_resolve", input, at)
}
pub fn recheck_source(db: &Connection, run_id: AgentRunId) -> StoreResult<Option<ModelStep>> {
    let run = turns::load_run(db, run_id)?;
    let Some(record) =
        latest(db, run_id)?.filter(|r| r.view.state == AgentContextWindowState::Preparing)
    else {
        return Ok(None);
    };
    let step = turns::step(db, record.view.correction.model_step_id)?;
    if run.state != AgentRunState::PreparingContext
        || step.agent_run_id != run_id
        || !matches!(step.state, ModelStepState::Failed { .. })
        || step.ordinal.checked_add(1) != Some(run.next_model_ordinal)
        || model_context::steps(db, run_id)?.last() != Some(&step)
    {
        return Ok(None);
    }
    Ok(Some(step))
}
