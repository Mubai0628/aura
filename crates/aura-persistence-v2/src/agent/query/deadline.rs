//! A pending question receives a deadline when shown or interacted with.
use super::super::{application, turns, work};
use super::resolve;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::StoreResult;
use rusqlite::{Connection, Transaction};

/// No deadline applies to a pending question the user has never seen.
pub fn deadline(db: &Connection, query: &UserQuery) -> StoreResult<Option<UtcTimestamp>> {
    if query.state != UserQueryState::Pending {
        return Ok(None);
    }
    super::super::human_wait::recorded_deadline(db, &query.query_id.to_string())
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
