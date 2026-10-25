//! Protocol completion reminder and bounded deterministic correction counters.
use super::{facts, turns};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::NormalizedStopReason;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, Transaction};

const KIND: &str = "agent.turn_guard";

pub fn load(db: &Connection, turn: TurnTaskId) -> StoreResult<TurnGuardState> {
    Ok(facts::latest(db, &turn.to_string(), KIND)?.unwrap_or_else(|| TurnGuardState::fresh(turn)))
}

fn save(
    tx: &Transaction<'_>,
    previous: &TurnGuardState,
    next: &TurnGuardState,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if load(tx, previous.turn_task_id)? != *previous
        || previous.turn_task_id != next.turn_task_id
        || next.revision.value()
            != previous.revision.value().checked_add(1).ok_or(Error::Conflict)?
    {
        return Err(Error::Conflict);
    }
    facts::append(tx, EventId::new(), &next.turn_task_id.to_string(), KIND, next, at)
}

pub fn blocked(db: &Connection, turn: TurnTaskId) -> StoreResult<Option<TurnGuardStopReason>> {
    Ok(load(db, turn)?.stopped)
}

/// C1: a complete EndTurn with no tools is reminded once, then stopped.
pub fn note_end_turn(
    tx: &Transaction<'_>,
    run: AgentRunId,
    stop: NormalizedStopReason,
    has_tools: bool,
    at: UtcTimestamp,
) -> StoreResult<Option<TurnGuardStopReason>> {
    let agent = turns::load_run(tx, run)?;
    let turn = turns::load(tx, agent.turn_task_id)?;
    if has_tools
        || stop != NormalizedStopReason::EndTurn
        || turn.state.terminal()
        || super::inbox::pending_additions(tx, &turn)?
        || matches!(
            turn.state,
            TurnTaskState::AwaitingQuery { .. } | TurnTaskState::AwaitingApproval
        )
        || matches!(
            agent.state,
            AgentRunState::AwaitingQuery { .. }
                | AgentRunState::AwaitingApproval { .. }
                | AgentRunState::AwaitingContextRemediation { .. }
        )
    {
        reset_protocol(tx, turn.turn_task_id, at)?;
        return Ok(None);
    }
    let previous = load(tx, turn.turn_task_id)?;
    if previous.stopped.is_some() {
        return Ok(previous.stopped);
    }
    let mut next = previous.clone();
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    if previous.completion_reminder_sent {
        next.stopped = Some(TurnGuardStopReason::ProtocolNotCompleted);
    } else {
        next.completion_reminder_sent = true;
    }
    save(tx, &previous, &next, at)?;
    Ok(next.stopped)
}

pub fn record_failure(
    tx: &Transaction<'_>,
    turn: TurnTaskId,
    failure: &DeterministicFailure,
    at: UtcTimestamp,
) -> StoreResult<Option<TurnGuardStopReason>> {
    let previous = load(tx, turn)?;
    if previous.stopped.is_some() {
        return Ok(previous.stopped);
    }
    let key = failure.target_key();
    let mut next = previous.clone();
    match next.corrections.iter().position(|s| s.target_key == key) {
        Some(index) if next.corrections[index].feedback_delivered_to_step.is_none() => {
            return Ok(None);
        }
        Some(index) => {
            next.corrections[index].remaining_attempts =
                next.corrections[index].remaining_attempts.saturating_sub(1);
            next.corrections[index].feedback_delivered_to_step = None;
            if next.corrections[index].remaining_attempts == 0 {
                next.stopped = Some(TurnGuardStopReason::DeterministicCorrectionExhausted);
            }
        }
        None => next.corrections.push(CorrectionSequence {
            target_key: key,
            rule_id: failure.rule_id.clone(),
            rule_version: failure.rule_version,
            field: failure.field.clone(),
            feedback_delivered_to_step: None,
            remaining_attempts: 2,
        }),
    }
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    save(tx, &previous, &next, at)?;
    Ok(next.stopped)
}

pub fn mark_feedback_delivered(
    tx: &Transaction<'_>,
    turn: TurnTaskId,
    step: ModelStepId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let previous = load(tx, turn)?;
    if previous.stopped.is_some()
        || !previous.corrections.iter().any(|s| s.feedback_delivered_to_step.is_none())
    {
        return Ok(());
    }
    let mut next = previous.clone();
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    for sequence in &mut next.corrections {
        if sequence.feedback_delivered_to_step.is_none() {
            sequence.feedback_delivered_to_step = Some(step);
        }
    }
    save(tx, &previous, &next, at)
}

pub fn reset_protocol(tx: &Transaction<'_>, turn: TurnTaskId, at: UtcTimestamp) -> StoreResult<()> {
    let previous = load(tx, turn)?;
    if previous.stopped.is_some() || !previous.completion_reminder_sent {
        return Ok(());
    }
    let mut next = previous.clone();
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    next.completion_reminder_sent = false;
    save(tx, &previous, &next, at)
}

pub fn clear_matching(
    tx: &Transaction<'_>,
    turn: TurnTaskId,
    prefix: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let previous = load(tx, turn)?;
    if previous.stopped.is_some()
        || !previous.corrections.iter().any(|s| s.target_key.contains(prefix))
    {
        return Ok(());
    }
    let mut next = previous.clone();
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    next.corrections.retain(|s| !s.target_key.contains(prefix));
    save(tx, &previous, &next, at)
}
