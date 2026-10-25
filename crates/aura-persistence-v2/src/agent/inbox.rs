//! Revisioned input queue, using the existing immutable Event/Outbox store.
//! Pending text is not model history until the owning consumption transaction.
use super::{facts, session, turns};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, Transaction};

pub fn load(db: &Connection, id: SessionId) -> StoreResult<AgentInboxProjection> {
    if crate::workspace::session(db, id)?.is_none() {
        return Err(Error::Conflict);
    }
    let value: AgentInboxProjection =
        facts::latest(db, &owner(id), "agent.inbox")?.unwrap_or(AgentInboxProjection {
            session_id: id,
            revision: Revision::INITIAL,
            paused: false,
            pause_reason: None,
            messages: vec![],
        });
    if value.session_id != id || value.revision.value() == 0 {
        return Err(Error::Conflict);
    }
    Ok(value)
}

fn owner(id: SessionId) -> String {
    format!("agent-inbox:{id}")
}

fn save(
    tx: &Transaction<'_>,
    previous: &AgentInboxProjection,
    mut next: AgentInboxProjection,
    at: UtcTimestamp,
) -> StoreResult<AgentInboxProjection> {
    if load(tx, previous.session_id)? != *previous || next.session_id != previous.session_id {
        return Err(Error::Conflict);
    }
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    if serde_json::to_vec(&next)?.len() > 4 * 1024 * 1024 {
        return Err(Error::Invalid("agent.inbox_full"));
    }
    facts::append(tx, EventId::new(), &owner(next.session_id), "agent.inbox", &next, at)?;
    Ok(next)
}

fn addition_is_active(
    db: &Connection,
    session_id: SessionId,
    turn: TurnTaskId,
) -> StoreResult<bool> {
    let active = session::load(db, session_id)?.active_turn;
    if active != Some(turn) {
        return Ok(false);
    }
    let value = turns::load(db, turn)?;
    Ok(value.session_id == session_id && !value.state.terminal())
}

fn replay<T: serde::Serialize>(
    db: &Connection,
    command: CommandInvocationId,
    input: &T,
) -> StoreResult<bool> {
    let previous: Option<serde_json::Value> =
        facts::latest(db, &command.to_string(), "agent.inbox_command")?;
    match previous {
        Some(old) if old == serde_json::to_value(input)? => Ok(true),
        Some(_) => Err(Error::Conflict),
        None => Ok(false),
    }
}

fn receipt(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    input: &impl serde::Serialize,
    at: UtcTimestamp,
) -> StoreResult<()> {
    facts::append(tx, EventId::new(), &command.to_string(), "agent.inbox_command", input, at)
}

/// Whether this exact authenticated submit command already committed, checked before Core
/// reads selected files or issues their authority. A different payload is a conflict.
pub fn submitted(
    db: &Connection,
    command: CommandInvocationId,
    input: &AgentSubmitRequest,
) -> StoreResult<bool> {
    replay(db, command, &input.receipt())
}

/// Queues one message with no chosen Skills.
pub fn submit(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    input: &AgentSubmitRequest,
    at: UtcTimestamp,
) -> StoreResult<AgentInboxProjection> {
    submit_with_skills(tx, command, input, &[], at)
}

/// Queues one message. `skills` are the Skills chosen with it, already checked by Core; only a
/// message that starts a new turn may carry them.
pub fn submit_with_skills(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    input: &AgentSubmitRequest,
    skills: &[LoadedSkillEntry],
    at: UtcTimestamp,
) -> StoreResult<AgentInboxProjection> {
    submit_message(tx, command, input, skills, &[], at)
}

/// Queues one message with its Skills and the pictures and files Core read and checked
/// for it. A repeated command is compared by its receipt, which holds their digests.
pub fn submit_message(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    input: &AgentSubmitRequest,
    skills: &[LoadedSkillEntry],
    prepared: &[super::attachments::PreparedAttachment],
    at: UtcTimestamp,
) -> StoreResult<AgentInboxProjection> {
    let input = &input.receipt();
    if replay(tx, command, input)? {
        return load(tx, input.session_id);
    }
    if prepared.len() != input.attachments.len() {
        return Err(Error::Invalid("agent.attachments_unprepared"));
    }
    validate_agent_message(&input.text, !prepared.is_empty())
        .map_err(|_| Error::Invalid("agent.input_invalid"))?;
    let previous = load(tx, input.session_id)?;
    // A bounded queue is explicit failure, never implicit loss or consumption.
    if previous.messages.len() >= 256
        || previous.messages.iter().map(|m| m.text.len()).sum::<usize>() + input.text.len()
            > 4 * 1024 * 1024
    {
        return Err(Error::Invalid("agent.inbox_full"));
    }
    let destination = match input.destination {
        AgentInputLane::Addition { turn_task_id }
            if addition_is_active(tx, input.session_id, turn_task_id)? =>
        {
            input.destination.clone()
        }
        // A stale UI cannot inject into a different task or manufacture an answer.
        _ => AgentInputLane::NextTurn,
    };
    if !skills.is_empty() && destination != AgentInputLane::NextTurn {
        return Err(Error::Invalid("agent.skills_need_new_turn"));
    }
    let mut next = previous.clone();
    // A pause protects input that is already queued. With nothing pending there
    // is no queue to preserve, and the next authenticated send is a new explicit
    // user action: it must start instead of inheriting a stale pause left by a
    // restart, stop or failure. Messages that are still queued keep the pause.
    if previous.paused && previous.messages.is_empty() {
        next.paused = false;
        next.pause_reason = None;
    }
    next.messages.push(AgentPendingMessage {
        message_id: EventId::new(),
        revision: Revision::INITIAL,
        text: input.text.clone(),
        destination,
        created_at: at,
        skills: skills.to_vec(),
        reasoning_effort: input.reasoning_effort,
        attachments: super::attachments::keep(tx, input.session_id, prepared, at)?,
    });
    let next = save(tx, &previous, next, at)?;
    receipt(tx, command, input, at)?;
    Ok(next)
}

pub fn mutate(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    input: &AgentInboxMutation,
    at: UtcTimestamp,
) -> StoreResult<AgentInboxProjection> {
    if replay(tx, command, input)? {
        return load(tx, input.session_id);
    }
    let previous = load(tx, input.session_id)?;
    if previous.revision != input.expected_revision {
        return Err(Error::Conflict);
    }
    let mut next = previous.clone();
    match &input.action {
        AgentInboxAction::Pause => next.paused = true,
        AgentInboxAction::Resume => {
            next.paused = false;
            next.pause_reason = None;
        }
        AgentInboxAction::Remove { message_id } => {
            let index = next
                .messages
                .iter()
                .position(|m| m.message_id == *message_id)
                .ok_or(Error::Conflict)?;
            let removed = next.messages.remove(index);
            super::attachments::release(tx, input.session_id, &removed.attachments)?;
        }
        action => {
            let id = match action {
                AgentInboxAction::Edit { message_id, .. }
                | AgentInboxAction::Append { message_id, .. }
                | AgentInboxAction::Revoke { message_id } => *message_id,
                _ => return Err(Error::Conflict),
            };
            let message =
                next.messages.iter_mut().find(|m| m.message_id == id).ok_or(Error::Conflict)?;
            message.revision =
                Revision::new(message.revision.value().checked_add(1).ok_or(Error::Conflict)?);
            match action {
                AgentInboxAction::Edit { text, .. } => {
                    validate_agent_message(text, !message.attachments.is_empty())
                        .map_err(|_| Error::Invalid("agent.input_invalid"))?;
                    message.text = text.clone();
                }
                AgentInboxAction::Append { turn_task_id, .. } => {
                    // Chosen Skills belong to a new turn; they cannot join a running one.
                    if !message.skills.is_empty() {
                        return Err(Error::Invalid("agent.skills_need_new_turn"));
                    }
                    if message.destination != AgentInputLane::NextTurn
                        || !addition_is_active(tx, input.session_id, *turn_task_id)?
                    {
                        return Err(Error::Conflict);
                    }
                    message.destination = AgentInputLane::Addition { turn_task_id: *turn_task_id };
                }
                AgentInboxAction::Revoke { .. } => {
                    if message.destination == AgentInputLane::NextTurn {
                        return Err(Error::Conflict);
                    }
                    message.destination = AgentInputLane::NextTurn;
                }
                _ => return Err(Error::Conflict),
            }
        }
    }
    if next.messages.iter().map(|m| m.text.len()).sum::<usize>() > 4 * 1024 * 1024 {
        return Err(Error::Invalid("agent.inbox_full"));
    }
    let next = save(tx, &previous, next, at)?;
    receipt(tx, command, input, at)?;
    Ok(next)
}

pub fn pending_additions(db: &Connection, turn: &TurnTask) -> StoreResult<bool> {
    Ok(load(db, turn.session_id)?
        .messages
        .iter()
        .any(|m| m.destination == (AgentInputLane::Addition { turn_task_id: turn.turn_task_id })))
}

/// Called inside the transaction that starts/updates the owning Turn. The
/// caller first freezes and persists every message with its original identity.
pub fn consume(
    tx: &Transaction<'_>,
    previous: &AgentInboxProjection,
    turn: TurnTaskId,
    lane: AgentInputLane,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let target = turns::load(tx, turn)?;
    if target.session_id != previous.session_id || target.state.terminal() {
        return Err(Error::Conflict);
    }
    if lane == AgentInputLane::NextTurn && previous.paused {
        return Err(Error::Conflict);
    }
    let ids: Vec<_> =
        previous.messages.iter().filter(|m| m.destination == lane).map(|m| m.message_id).collect();
    if ids.is_empty() {
        return Ok(());
    }
    for id in &ids {
        session::user_message(tx, previous.session_id, *id)?;
    }
    let mut next = previous.clone();
    next.messages.retain(|m| !ids.contains(&m.message_id));
    save(tx, previous, next, at)?;
    let message_revisions = previous
        .messages
        .iter()
        .filter(|m| ids.contains(&m.message_id))
        .map(|m| (m.message_id, m.revision))
        .collect();
    facts::append(
        tx,
        EventId::new(),
        &turn.to_string(),
        "agent.input_consumed",
        &AgentConsumedInput {
            turn_task_id: turn,
            user_event_ids: ids,
            message_revisions,
            destination: lane,
            consumed_at: at,
        },
        at,
    )
}

pub fn consumed(db: &Connection, turn: TurnTaskId) -> StoreResult<Vec<AgentConsumedInput>> {
    let mut q = db.prepare("SELECT payload FROM events WHERE owner=?1 AND kind='agent.input_consumed' ORDER BY sequence")?;
    q.query_map([turn.to_string()], |r| r.get::<_, String>(0))?
        .map(|raw| Ok(serde_json::from_str(&raw?)?))
        .collect()
}

pub fn pause(tx: &Transaction<'_>, id: SessionId, at: UtcTimestamp) -> StoreResult<()> {
    pause_with_reason(tx, id, None, at)
}
pub fn pause_with_reason(
    tx: &Transaction<'_>,
    id: SessionId,
    reason: Option<String>,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let old = load(tx, id)?;
    let mut next = old.clone();
    next.paused = true;
    next.pause_reason = reason;
    // A terminated task cannot receive additions later. Preserve the input for
    // explicit next-turn execution instead of dropping it or changing history.
    for m in &mut next.messages {
        if m.destination != AgentInputLane::NextTurn {
            m.destination = AgentInputLane::NextTurn;
            m.revision = Revision::new(m.revision.value().checked_add(1).ok_or(Error::Conflict)?);
        }
    }
    if old != next {
        save(tx, &old, next, at)?;
    }
    Ok(())
}

pub fn startup(tx: &Transaction<'_>, at: UtcTimestamp) -> StoreResult<()> {
    // Every inbox is an owner of this prefix: a range of the index, not the journal.
    let mut q = tx.prepare(
        "SELECT DISTINCT owner FROM events \
         WHERE owner>='agent-inbox:' AND owner<'agent-inbox;' AND kind='agent.inbox'",
    )?;
    let owners = q.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    for name in owners {
        let id = name
            .strip_prefix("agent-inbox:")
            .ok_or(Error::Conflict)?
            .parse()
            .map_err(|_| Error::Conflict)?;
        pause(tx, id, at)?;
    }
    Ok(())
}
