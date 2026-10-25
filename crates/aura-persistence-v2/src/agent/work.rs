use super::facts;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, Transaction};

pub fn todo(db: &Connection, turn: TurnTaskId) -> StoreResult<TodoState> {
    Ok(facts::latest(db, &turn.to_string(), "agent.todo")?.unwrap_or(TodoState {
        turn_task_id: turn,
        revision: Revision::INITIAL,
        items: vec![],
    }))
}
pub fn save_todo(
    tx: &Transaction<'_>,
    previous: &TodoState,
    next: &TodoState,
    at: UtcTimestamp,
) -> StoreResult<()> {
    next.validate().map_err(|_| Error::Invalid("Todo"))?;
    if previous.turn_task_id != next.turn_task_id
        || super::turns::load(tx, next.turn_task_id)?.state.terminal()
        || todo(tx, previous.turn_task_id)? != *previous
        || next.revision.value()
            != previous.revision.value().checked_add(1).ok_or(Error::Conflict)?
    {
        return Err(Error::Conflict);
    }
    facts::append(tx, EventId::new(), &next.turn_task_id.to_string(), "agent.todo", next, at)
}
pub fn goal(db: &Connection, turn: TurnTaskId) -> StoreResult<GoalState> {
    let mut value: GoalState =
        facts::latest(db, &turn.to_string(), "agent.goal")?.ok_or(Error::Conflict)?;
    if value.turn_task_id != turn {
        return Err(Error::Conflict);
    }
    // Usage is a projection of immutable Provider responses, not a control
    // mutation that invalidates the very Tool arguments that response produced.
    let run = super::turns::load(db, turn)?.agent_run_id;
    let mut q=db.prepare("SELECT event_id,payload FROM events WHERE owner=?1 AND kind='agent.assistant_response' ORDER BY sequence")?;
    let (mut tokens, _) = crate::context::compaction::usage(db, run)?;
    let mut count = 0u64;
    if let Some(inherited) =
        facts::latest::<GoalState>(db, &turn.to_string(), "agent.goal_inherited_usage")?
    {
        if inherited.turn_task_id == turn {
            return Err(Error::Conflict);
        }
        tokens = tokens.checked_add(inherited.consumed_tokens).ok_or(Error::Conflict)?;
        count = inherited.consumed_turns;
    }
    let excluded =
        facts::latest::<ModeJudgmentRecord>(db, &turn.to_string(), "agent.mode_judgment")?
            .map(|record| record.excluded_model_step_ids)
            .unwrap_or_default();
    for row in
        q.query_map([run.to_string()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
    {
        let (event, raw) = row?;
        let output: aura_model_domain::NormalizedModelOutput = serde_json::from_str(&raw)?;
        let step = super::turns::step(db, output.model_step_id)?;
        if step.agent_run_id != run
            || !matches!(step.state,ModelStepState::Completed{assistant_event_id,..} if assistant_event_id.to_string()==event)
        {
            return Err(Error::Conflict);
        }
        if excluded.contains(&output.model_step_id) {
            continue;
        }
        tokens = tokens.checked_add(output.usage.total_tokens).ok_or(Error::Conflict)?;
        count = count.checked_add(1).ok_or(Error::Conflict)?;
    }
    if value.consumed_tokens > tokens || value.consumed_turns > count {
        return Err(Error::Conflict);
    }
    value.consumed_tokens = tokens;
    value.consumed_turns = count;
    Ok(value)
}
pub fn initialize_goal(
    tx: &Transaction<'_>,
    value: &GoalState,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if value.revision != Revision::INITIAL
        || value.completed
        || value.consumed_tokens != 0
        || value.consumed_turns != 0
        || value.budget.max_tokens == Some(0)
        || value.budget.max_turns == Some(0)
        || facts::latest::<GoalState>(tx, &value.turn_task_id.to_string(), "agent.goal")?.is_some()
        || super::turns::load(tx, value.turn_task_id)?.mode != AgentTaskMode::Goal
    {
        return Err(Error::Conflict);
    }
    facts::append(tx, EventId::new(), &value.turn_task_id.to_string(), "agent.goal", value, at)
}
pub fn save_goal(
    tx: &Transaction<'_>,
    previous: &GoalState,
    next: &GoalState,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if next.completed {
        return Err(Error::Invalid("Goal completion requires owning transaction"));
    }
    save_goal_checked(tx, previous, next, at)
}
pub(super) fn complete_goal(
    tx: &Transaction<'_>,
    previous: &GoalState,
    next: &GoalState,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if !next.completed {
        return Err(Error::Conflict);
    }
    save_goal_checked(tx, previous, next, at)
}
fn save_goal_checked(
    tx: &Transaction<'_>,
    previous: &GoalState,
    next: &GoalState,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if previous.turn_task_id != next.turn_task_id
        || goal(tx, previous.turn_task_id)? != *previous
        || previous.completed
        || next.budget != previous.budget
        || next.consumed_tokens != previous.consumed_tokens
        || next.consumed_turns != previous.consumed_turns
        || next.revision.value()
            != previous.revision.value().checked_add(1).ok_or(Error::Conflict)?
    {
        return Err(Error::Conflict);
    }
    facts::append(tx, EventId::new(), &next.turn_task_id.to_string(), "agent.goal", next, at)
}
pub fn update_goal_budget(
    tx: &Transaction<'_>,
    turn: TurnTaskId,
    budget: GoalBudget,
    at: UtcTimestamp,
) -> StoreResult<GoalState> {
    let previous = goal(tx, turn)?;
    if previous.completed {
        return Err(Error::Conflict);
    }
    let mut next = previous.clone();
    next.budget = budget;
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    if previous.turn_task_id != next.turn_task_id
        || next.consumed_tokens != previous.consumed_tokens
        || next.consumed_turns != previous.consumed_turns
    {
        return Err(Error::Conflict);
    }
    facts::append(tx, EventId::new(), &next.turn_task_id.to_string(), "agent.goal", &next, at)?;
    Ok(next)
}
pub fn query(db: &Connection, turn: TurnTaskId) -> StoreResult<Option<UserQuery>> {
    facts::latest(db, &turn.to_string(), "agent.query")
}
pub fn save_query(
    tx: &Transaction<'_>,
    previous: Option<&UserQuery>,
    next: &UserQuery,
    at: UtcTimestamp,
) -> StoreResult<()> {
    next.input.validate().map_err(|_| Error::Invalid("user query"))?;
    if query(tx, next.turn_task_id)?.as_ref() != previous {
        return Err(Error::Conflict);
    }
    let turn = super::turns::load(tx, next.turn_task_id)?;
    if turn.state.terminal() || turn.agent_run_id != next.agent_run_id {
        return Err(Error::Conflict);
    }
    match previous {
        Some(old) if old.state == UserQueryState::Pending => {
            if next.query_id != old.query_id
                || next.tool_call_id != old.tool_call_id
                || next.input != old.input
                || next.revision.value()
                    != old.revision.value().checked_add(1).ok_or(Error::Conflict)?
                || next.state == UserQueryState::Pending
            {
                return Err(Error::Conflict);
            }
        }
        _ if next.revision != Revision::INITIAL || next.state != UserQueryState::Pending => {
            return Err(Error::Conflict);
        }
        _ => {}
    }
    if let UserQueryState::Answered { user_event_id, answers } = &next.state {
        next.input.validate_answers(answers).map_err(|_| Error::Invalid("query answers"))?;
        let user = super::session::user_message(tx, turn.session_id, *user_event_id)?;
        if user.origin != (UserMessageOrigin::QueryAnswer { query_id: next.query_id }) {
            return Err(Error::Conflict);
        }
    }
    facts::append(tx, EventId::new(), &next.turn_task_id.to_string(), "agent.query", next, at)
}
