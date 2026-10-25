//! An explicit user action starts a new associated turn. The original
//! plan revision, interrupted turn and unknown effects are never rewritten.
use super::{facts, recovery, turns, work};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, Transaction};

pub struct RelatedTurnStart {
    pub text: String,
    pub mode: AgentTaskMode,
    pub goal_budget: Option<GoalBudget>,
}

pub fn submitted(
    db: &Connection,
    command: CommandInvocationId,
    input: &AgentRelatedTurnRequest,
) -> StoreResult<Option<AgentRunId>> {
    let previous: Option<(AgentRelatedTurnRequest, AgentRunId)> =
        facts::latest(db, &command.to_string(), "agent.related_command")?;
    match previous {
        Some((request, run)) if request == *input => Ok(Some(run)),
        Some(_) => Err(Error::Conflict),
        None => Ok(None),
    }
}

pub fn validate(
    db: &Connection,
    request: &AgentRelatedTurnRequest,
) -> StoreResult<RelatedTurnStart> {
    let source = turns::load(db, request.source_turn_task_id)?;
    if source.session_id != request.session_id || source.revision != request.expected_revision {
        return Err(Error::Conflict);
    }
    if request.goal_budget.is_some()
        && (request.kind != AgentRelatedTurnKind::Recover || source.mode != AgentTaskMode::Goal)
    {
        return Err(Error::Conflict);
    }
    match request.kind {
        AgentRelatedTurnKind::ImplementPlan => {
            let plan: Option<PlanRecord> =
                facts::latest(db, &source.turn_task_id.to_string(), "agent.plan_completed")?;
            if source.mode != AgentTaskMode::Plan
                || !matches!(source.state, TurnTaskState::Completed { .. })
                || plan.is_none()
            {
                return Err(Error::Conflict);
            }
            Ok(RelatedTurnStart {
                text: format!(
                    "实施已确认的计划（任务 {}，修订 {}）。",
                    source.turn_task_id,
                    source.revision.value()
                ),
                mode: AgentTaskMode::Normal,
                goal_budget: None,
            })
        }
        AgentRelatedTurnKind::Recover => Err(Error::Invalid("agent.recovery_entry_removed")),
    }
}
pub fn record(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    request: &AgentRelatedTurnRequest,
    target: &TurnTask,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if submitted(tx, command, request)?.is_some() {
        return Err(Error::Conflict);
    }
    let start = validate(tx, request)?;
    if target.session_id != request.session_id
        || target.mode != start.mode
        || target.turn_task_id == request.source_turn_task_id
    {
        return Err(Error::Conflict);
    }
    facts::append(
        tx,
        EventId::new(),
        &target.turn_task_id.to_string(),
        "agent.related_turn",
        request,
        at,
    )?;
    facts::append(
        tx,
        EventId::new(),
        &command.to_string(),
        "agent.related_command",
        &(request, target.agent_run_id),
        at,
    )
}

/// Actual prior state is model input. The Core never declares which parts of
/// the interrupted work succeeded or chooses the semantic lineage on its behalf.
pub fn context(db: &Connection, turn: TurnTaskId) -> StoreResult<Option<serde_json::Value>> {
    let Some(source) =
        facts::latest::<AgentRelatedTurnRequest>(db, &turn.to_string(), "agent.related_turn")?
    else {
        return Ok(None);
    };
    let original = turns::load(db, source.source_turn_task_id)?;
    Ok(Some(serde_json::json!({
        "request": source,
        "turn": original,
        "todo": work::todo(db, original.turn_task_id)?,
        "goal": if original.mode == AgentTaskMode::Goal { Some(work::goal(db, original.turn_task_id)?) } else { None },
        "plan": facts::latest::<PlanRecord>(db, &original.turn_task_id.to_string(), "agent.plan_completed")?,
        "recovery_episodes": recovery::episodes(db, original.agent_run_id)?,
    })))
}
