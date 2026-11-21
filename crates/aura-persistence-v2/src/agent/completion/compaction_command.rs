use super::*;

pub fn finish_compaction_command(
    tx: &Transaction<'_>,
    step_id: ModelStepId,
    at: UtcTimestamp,
) -> StoreResult<bool> {
    let step = turns::step(tx, step_id)?;
    let run = turns::load_run(tx, step.agent_run_id)?;
    if application::purpose(tx, run.agent_run_id)? != AgentTaskPurpose::Compaction {
        return Ok(false);
    }
    let turn = turns::load(tx, run.turn_task_id)?;
    if step.state != ModelStepState::Preparing
        || run.state != AgentRunState::PreparingContext
        || application::cancellation_requested(tx, run.agent_run_id)?
        || inbox::pending_additions(tx, &turn)?
    {
        return Ok(false);
    }
    let Some(request) = crate::context::manual_compaction::record(tx, run.agent_run_id)? else {
        return Ok(false);
    };
    if request.state != aura_context_domain::ManualCompactionRequestState::Completed {
        return Ok(false);
    }
    let report = if request.source_context_snapshot_id.is_some() {
        "会话历史已压缩，新的背景摘要已保存。"
    } else {
        "当前没有需要压缩的历史，已保留现有会话背景。"
    };
    let previous = lineage::load(tx, turn.lineage_id)?;
    // The preparation slot never dispatched a normal model request. Close it with no invented
    // model response, then complete the command's own lineage using the durable summary receipt.
    let mut unused = step.clone();
    unused.state = ModelStepState::Cancelled;
    unused.finished_at = Some(at);
    turns::save_step(tx, &step, &unused)?;
    commit_terminal(tx, &turn, &run, &previous, report, at)?;
    Ok(true)
}
