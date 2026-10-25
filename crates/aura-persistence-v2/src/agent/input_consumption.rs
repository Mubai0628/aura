//! Safe-point publication of independent user messages into the owning Turn.
use super::{attachments, inbox, lineage, session, turns};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Transaction;

pub fn start_batch(
    tx: &Transaction<'_>,
    pending: &AgentInboxProjection,
    turn: &TurnTask,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if inbox::load(tx, turn.session_id)? != *pending || pending.paused {
        return Err(Error::Conflict);
    }
    for message in pending.messages.iter().filter(|m| {
        m.destination == AgentInputLane::NextTurn && m.message_id != turn.root_user_event_id
    }) {
        let user = attachments::freeze(tx, message, turn.session_id, at)?;
        session::add_user_message(tx, &user)?;
        attachments::record(tx, user.event_id, &message.text, &message.attachments, at)?;
        attach(tx, turn, &user, at)?;
    }
    inbox::consume(tx, pending, turn.turn_task_id, AgentInputLane::NextTurn, at)
}

fn attach(
    tx: &Transaction<'_>,
    turn: &TurnTask,
    user: &OriginalUserMessage,
    at: UtcTimestamp,
) -> StoreResult<()> {
    // A pre-existing open lineage is resolved by the model, never guessed from
    // text. All input remains in history and is attached after that decision.
    if turn.lineage_resolution_required {
        return Ok(());
    }
    let previous = lineage::load(tx, turn.lineage_id)?;
    if previous.sources.iter().any(|s| s.user_event_id == user.event_id) {
        return Ok(());
    }
    let mut next = previous.clone();
    next.attach_source(user, turn.turn_task_id, vec![], previous.revision, at)
        .map_err(|_| Error::Conflict)?;
    lineage::save(tx, &previous, &next)
}

pub fn attach_consumed(tx: &Transaction<'_>, turn: &TurnTask, at: UtcTimestamp) -> StoreResult<()> {
    for group in inbox::consumed(tx, turn.turn_task_id)? {
        for id in group.user_event_ids {
            if id != turn.root_user_event_id {
                attach(tx, turn, &session::user_message(tx, turn.session_id, id)?, at)?;
            }
        }
    }
    Ok(())
}

pub fn additions(tx: &Transaction<'_>, run: AgentRunId, at: UtcTimestamp) -> StoreResult<()> {
    let agent = turns::load_run(tx, run)?;
    if agent.state != AgentRunState::PreparingContext {
        return Err(Error::Conflict);
    }
    let turn = turns::load(tx, agent.turn_task_id)?;
    let previous = inbox::load(tx, turn.session_id)?;
    let lane = AgentInputLane::Addition { turn_task_id: turn.turn_task_id };
    let messages: Vec<_> = previous.messages.iter().filter(|m| m.destination == lane).collect();
    if messages.is_empty() {
        return Ok(());
    }
    for message in messages {
        let user = attachments::freeze(tx, message, turn.session_id, at)?;
        session::add_user_message(tx, &user)?;
        attachments::record(tx, user.event_id, &message.text, &message.attachments, at)?;
        attach(tx, &turn, &user, at)?;
    }
    inbox::consume(tx, &previous, turn.turn_task_id, lane, at)?;
    super::guard::reset_protocol(tx, turn.turn_task_id, at)?;
    let mut next = turn.clone();
    next.transition(turn.revision, turn.state.clone(), at).map_err(|_| Error::Conflict)?;
    turns::save(tx, &turn, &next)
}
