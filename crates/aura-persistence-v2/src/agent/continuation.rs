//! Derive the next boundary from committed facts after wake-up or reconnect.
use super::{model_context, turns, work};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Connection;
pub fn next(db: &Connection, id: AgentRunId) -> StoreResult<AgentContinuation> {
    let run = turns::load_run(db, id)?;
    let turn = turns::load(db, run.turn_task_id)?;
    if run.state != AgentRunState::PreparingContext || turn.state.terminal() {
        return Err(Error::Conflict);
    }
    if run.next_model_ordinal == 1 {
        return Ok(AgentContinuation::Initial { user_event_id: turn.root_user_event_id });
    }
    if let Some(step) = crate::context::remediation::recheck_source(db, id)? {
        return Ok(step.continuation);
    }
    // A step the provider failed for a passing reason is asked again where it stood.
    if let Some(step) = crate::model::transient_retry::source(db, id)? {
        return Ok(step.continuation);
    }
    let steps = model_context::steps(db, id)?;
    let step = steps.last().ok_or(Error::Conflict)?;
    if step.ordinal.checked_add(1) != Some(run.next_model_ordinal) {
        return Err(Error::Conflict);
    }
    match step.state {
        ModelStepState::Completed { tool_batch_id: None, .. } => {
            Ok(AgentContinuation::CompletionReminder { previous_model_step_id: step.model_step_id })
        }
        ModelStepState::Completed { tool_batch_id: Some(batch), .. } => {
            let group = crate::context::history(db, turn.session_id)?
                .into_iter()
                .find(|g| {
                    g.agent_run_id == id
                        && g.model_step_id == step.model_step_id
                        && g.tool_batch_id == batch
                })
                .ok_or(Error::Conflict)?;
            if let Some(q) = work::query(db, turn.turn_task_id)?
                && let UserQueryState::Answered { user_event_id, .. } = q.state
                && group.calls.iter().any(|c| c.tool_call_id == q.tool_call_id)
            {
                return Ok(AgentContinuation::QueryAnswered {
                    query_id: q.query_id,
                    user_event_id,
                });
            }
            Ok(AgentContinuation::ToolBatch {
                tool_batch_id: batch,
                history_group_hash: group.group_ref().map_err(|_| Error::Conflict)?.group_hash,
            })
        }
        _ => Err(Error::Conflict),
    }
}
