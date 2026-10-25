//! A question nobody touches ends as "no response" — its idle time and its
//! countdown after it was asked, or after the user last touched it. Touching it never changes the
//! question's revision, so an answer being written at the same moment still applies.
use super::super::{application, facts, turns, work};
use super::resolve;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, Transaction};

const TOUCH: &str = "agent.query_touch";

/// When this question ends as "no response"; `None` once it has ended.
pub fn deadline(db: &Connection, query: &UserQuery) -> StoreResult<Option<UtcTimestamp>> {
    if query.state != UserQueryState::Pending {
        return Ok(None);
    }
    let touched: Option<UtcTimestamp> = facts::latest(db, &query.query_id.to_string(), TOUCH)?;
    Ok(Some(query_deadline(query.created_at, touched)))
}

/// The deadline of the question this run waits on, if it waits on one.
pub fn run_deadline(db: &Connection, run: AgentRunId) -> StoreResult<Option<UtcTimestamp>> {
    let run = turns::load_run(db, run)?;
    let AgentRunState::AwaitingQuery { query_id } = run.state else {
        return Ok(None);
    };
    match work::query(db, run.turn_task_id)?.filter(|query| query.query_id == query_id) {
        Some(query) => deadline(db, &query),
        None => Ok(None),
    }
}

/// The run's next wait deadline: an approval's expiry or its question's "no response".
pub fn wait_deadline(db: &Connection, run: AgentRunId) -> StoreResult<Option<UtcTimestamp>> {
    let approval = super::super::approval::deadline(db, run)?;
    let question = run_deadline(db, run)?;
    Ok(match (approval, question) {
        (Some(approval), Some(question)) => Some(approval.min(question)),
        (approval, question) => approval.or(question),
    })
}

/// The user touched the question: its countdown starts again from `at`.
pub fn touch(
    tx: &Transaction<'_>,
    input: &AgentQueryTouchRequest,
    at: UtcTimestamp,
) -> StoreResult<UtcTimestamp> {
    let run = turns::load_run(tx, input.agent_run_id)?;
    let turn = turns::load(tx, run.turn_task_id)?;
    let query = work::query(tx, turn.turn_task_id)?.ok_or(Error::Conflict)?;
    if turn.session_id != input.session_id
        || query.query_id != input.query_id
        || query.state != UserQueryState::Pending
        || run.state != (AgentRunState::AwaitingQuery { query_id: query.query_id })
    {
        return Err(Error::Conflict);
    }
    facts::append(tx, EventId::new(), &query.query_id.to_string(), TOUCH, &at, at)?;
    Ok(query_deadline(query.created_at, Some(at)))
}

/// Ends the run's question as "no response" once its time has run out; `false` while there is
/// nothing to end.
pub fn expire(tx: &Transaction<'_>, run_id: AgentRunId, now: UtcTimestamp) -> StoreResult<bool> {
    let run = turns::load_run(tx, run_id)?;
    let AgentRunState::AwaitingQuery { query_id } = run.state else {
        return Ok(false);
    };
    if application::cancellation_requested(tx, run_id)? {
        return Ok(false);
    }
    let turn = turns::load(tx, run.turn_task_id)?;
    let Some(query) = work::query(tx, turn.turn_task_id)?.filter(|q| q.query_id == query_id) else {
        return Ok(false);
    };
    if !deadline(tx, &query)?.is_some_and(|at| at <= now) {
        return Ok(false);
    }
    let answers = query
        .input
        .questions
        .iter()
        .map(|question| QueryAnswer {
            question_id: question.id.clone(),
            value: QueryAnswerValue::NoResponse,
        })
        .collect::<Vec<_>>();
    resolve(tx, &run, &turn, &query, &answers, now)?;
    Ok(true)
}

/// After a restart, a question still waiting is shown again: its countdown starts over from `at`.
pub fn restart_countdown(
    tx: &Transaction<'_>,
    run_id: AgentRunId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let run = turns::load_run(tx, run_id)?;
    let AgentRunState::AwaitingQuery { query_id } = run.state else {
        return Ok(());
    };
    if work::query(tx, run.turn_task_id)?
        .is_some_and(|query| query.query_id == query_id && query.state == UserQueryState::Pending)
    {
        facts::append(tx, EventId::new(), &query_id.to_string(), TOUCH, &at, at)?;
    }
    Ok(())
}
