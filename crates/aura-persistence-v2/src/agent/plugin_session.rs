//! What a plugin reads of the conversation it was given: what happened since its cursor, what
//! the finished requests used, the newest request's context and the newest turn's to-do list.
//!
//! The cursor is the journal's own order. A turn starting and ending is read from the
//! conversation's state record, which is written once per change of its turn and found by its
//! owner in the index; an approval, a request to the model and its answer, and a compaction
//! starting and ending are read from the records added since the cursor, a range of the
//! journal's key. A read costs what happened since the one before it and never the whole
//! journal, and the first read starts now rather than at the beginning.
use super::{desktop_messages, session::AgentSessionState, turns, work};
use crate::context::compaction::COMPACTION_STATE;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use rusqlite::{Connection, OptionalExtension, params};

/// How far past its cursor one read looks.
pub const EVENTS_WINDOW: u64 = 5_000;
/// The most of why a turn failed or needs recovery that a plugin is told.
pub const REASON_MAX_CHARS: usize = 500;
/// How many of the newest requests are looked at for one whose context was measured.
const CONTEXT_STEPS: i64 = 16;

/// The journal's newest position.
pub fn head(db: &Connection) -> StoreResult<u64> {
    let value: i64 =
        db.query_row("SELECT COALESCE(MAX(sequence), 0) FROM events", [], |row| row.get(0))?;
    Ok(u64::try_from(value).unwrap_or_default())
}

/// What happened in the conversation after `after`, up to `window` positions further.
pub fn events(
    db: &Connection,
    session: SessionId,
    after: u64,
    window: u64,
) -> StoreResult<PluginSessionEvents> {
    let head = head(db)?;
    let after = after.min(head);
    let until = head.min(after.saturating_add(window.max(1)));
    let (low, high) = (position(after)?, position(until)?);
    let owner = format!("agent-session:{session}");
    let mut active = db
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.session_state' \
             AND version=1 AND sequence<=?2 ORDER BY sequence DESC LIMIT 1",
            params![owner, low],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|raw| record::decode::<AgentSessionState>(&raw))
        .transpose()?
        .and_then(|state| state.active_turn);
    let mut events = Vec::new();
    let mut states = db.prepare(
        "SELECT sequence, payload FROM events WHERE owner=?1 AND kind='agent.session_state' \
         AND version=1 AND sequence>?2 AND sequence<=?3 ORDER BY sequence",
    )?;
    let rows = states
        .query_map(params![owner, low, high], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (sequence, raw) in rows {
        let state: AgentSessionState = record::decode(&raw)?;
        if state.active_turn == active {
            continue;
        }
        let sequence = unsigned(sequence)?;
        if let Some(ended) = active {
            events.push(PluginSessionEvent { sequence, event: ended_turn(db, ended)? });
        }
        if let Some(started) = state.active_turn {
            events.push(PluginSessionEvent {
                sequence,
                event: PluginSessionEventKind::TurnStarted { turn_task_id: started },
            });
        }
        active = state.active_turn;
    }
    let mut approvals = db.prepare(
        "SELECT sequence, owner, json_extract(payload, '$.approval.approval_id') FROM events \
         WHERE sequence>?1 AND sequence<=?2 AND kind='agent.execution_approval' AND version=1",
    )?;
    let rows = approvals
        .query_map(params![low, high], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (sequence, run, approval) in rows {
        let (Ok(run), Ok(approval_id)) =
            (run.parse::<AgentRunId>(), approval.parse::<ApprovalRequestId>())
        else {
            continue;
        };
        let Some(turn) = found(turns::load_run(db, run))?
            .map(|run| found(turns::load(db, run.turn_task_id)))
            .transpose()?
            .flatten()
        else {
            continue;
        };
        if turn.session_id == session {
            events.push(PluginSessionEvent {
                sequence: unsigned(sequence)?,
                event: PluginSessionEventKind::ApprovalWaiting {
                    turn_task_id: turn.turn_task_id,
                    approval_id,
                },
            });
        }
    }
    steps(db, session, (low, high), &mut events)?;
    compactions(db, session, (low, high), &mut events)?;
    // Stable: what one record tells keeps its order.
    events.sort_by_key(|event| event.sequence);
    Ok(PluginSessionEvents { events, cursor: until, more: until < head })
}

/// How a turn that left the conversation ended: its final reply when it completed, why when it
/// failed or needs recovery.
fn ended_turn(db: &Connection, turn: TurnTaskId) -> StoreResult<PluginSessionEventKind> {
    let state = found(turns::load(db, turn))?.map(|loaded| loaded.state);
    let why = |reason: String| Some(reason.chars().take(REASON_MAX_CHARS).collect::<String>());
    let (outcome, final_reply, reason) = match state {
        Some(TurnTaskState::Completed { final_report }) => {
            (Some(AgentTurnOutcome::Completed), Some(final_report), None)
        }
        Some(TurnTaskState::Failed { reason }) => {
            (Some(AgentTurnOutcome::Failed), None, why(reason))
        }
        Some(TurnTaskState::Cancelled) => (Some(AgentTurnOutcome::Cancelled), None, None),
        Some(TurnTaskState::RecoveryRequired { reason }) => {
            (Some(AgentTurnOutcome::RecoveryRequired), None, why(reason))
        }
        _ => (None, None, None),
    };
    Ok(PluginSessionEventKind::TurnEnded { turn_task_id: turn, outcome, final_reply, reason })
}

/// The conversation's requests to the model in the journal range `(low, high]`: each as it goes
/// out, told by the record that freezes what it may do, and as it is answered, told by the
/// answer's record, with what it used and the tools it calls.
fn steps(
    db: &Connection,
    session: SessionId,
    (low, high): (i64, i64),
    events: &mut Vec<PluginSessionEvent>,
) -> StoreResult<()> {
    let mut started = db.prepare(
        "SELECT sequence, json_extract(payload, '$.identity.turn_task_id'), \
         json_extract(payload, '$.identity.model_step_id') FROM events \
         WHERE sequence>?1 AND sequence<=?2 AND kind='agent.model_boundary' AND version=1 \
         AND json_extract(payload, '$.identity.session_id')=?3",
    )?;
    let rows = started
        .query_map(params![low, high, session.to_string()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (sequence, turn, step) in rows {
        let (Ok(turn_task_id), Ok(model_step_id)) = (turn.parse(), step.parse()) else {
            continue;
        };
        events.push(PluginSessionEvent {
            sequence: unsigned(sequence)?,
            event: PluginSessionEventKind::StepStarted { turn_task_id, model_step_id },
        });
    }
    let mut answered = db.prepare(
        "SELECT e.sequence, r.turn_task_id, json_extract(e.payload, '$.model_step_id'), \
         coalesce(json_extract(e.payload, '$.usage.input_tokens'), 0), \
         coalesce(json_extract(e.payload, '$.usage.cached_input_tokens'), 0), \
         coalesce(json_extract(e.payload, '$.usage.output_tokens'), 0) \
         FROM events e JOIN agent_runs r ON r.id=e.owner JOIN turn_tasks t ON t.id=r.turn_task_id \
         WHERE e.sequence>?1 AND e.sequence<=?2 AND e.kind='agent.assistant_response' \
         AND e.version=1 AND t.session_id=?3",
    )?;
    let rows = answered
        .query_map(params![low, high, session.to_string()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                [row.get::<_, i64>(3)?, row.get::<_, i64>(4)?, row.get::<_, i64>(5)?],
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let count = |value: i64| u64::try_from(value).unwrap_or_default();
    for (sequence, turn, step, [input, cached, output]) in rows {
        let (Ok(turn_task_id), Ok(model_step_id)) = (turn.parse(), step.parse()) else {
            continue;
        };
        let usage = PluginStepUsage {
            input_tokens: count(input),
            cached_input_tokens: count(cached),
            output_tokens: count(output),
        };
        let at = unsigned(sequence)?;
        events.push(PluginSessionEvent {
            sequence: at,
            event: PluginSessionEventKind::StepEnded { turn_task_id, model_step_id, usage },
        });
        for (tool_call_id, tool) in tool_calls(db, sequence, model_step_id)? {
            events.push(PluginSessionEvent {
                sequence: at,
                event: PluginSessionEventKind::ToolCall {
                    turn_task_id,
                    model_step_id,
                    tool_call_id,
                    tool,
                },
            });
        }
    }
    Ok(())
}

/// The tools the answer at `sequence` calls, in its order, by their names in Aura: the request
/// it answers named each tool it offered under the name the Provider was given.
fn tool_calls(
    db: &Connection,
    sequence: i64,
    step: ModelStepId,
) -> StoreResult<Vec<(ToolCallId, String)>> {
    let mut calls = db.prepare(
        "SELECT json_extract(c.value, '$.tool_call_id'), json_extract(c.value, '$.tool_name') \
         FROM events e, json_each(e.payload, '$.tool_calls') c WHERE e.sequence=?1 \
         ORDER BY c.key",
    )?;
    let rows = calls
        .query_map([sequence], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut named = db.prepare(
        "SELECT json_extract(b.value, '$.canonical_name') FROM model_steps m \
         JOIN events e ON e.owner=m.snapshot_id AND e.kind='model.request_evidence' \
         AND e.version=1, json_each(e.payload, '$.tool_name_bindings') b \
         WHERE m.id=?1 AND json_extract(b.value, '$.provider_wire_name')=?2 \
         ORDER BY e.sequence DESC LIMIT 1",
    )?;
    let mut tools = Vec::with_capacity(rows.len());
    for (id, wire) in rows {
        let Ok(id) = id.parse() else { continue };
        let name = named
            .query_row(params![step.to_string(), wire], |row| row.get::<_, String>(0))
            .optional()?;
        tools.push((id, name.unwrap_or(wire)));
    }
    Ok(tools)
}

/// The compactions of the conversation's history in the journal range `(low, high]`, told by the
/// record each writes as it starts and as it ends.
fn compactions(
    db: &Connection,
    session: SessionId,
    (low, high): (i64, i64),
    events: &mut Vec<PluginSessionEvent>,
) -> StoreResult<()> {
    let mut facts = db.prepare(
        "SELECT sequence, owner, json_extract(payload, '$.state') FROM events \
         WHERE sequence>?1 AND sequence<=?2 AND kind=?3 AND version=1 \
         AND json_extract(payload, '$.session_id')=?4",
    )?;
    let rows = facts
        .query_map(params![low, high, COMPACTION_STATE, session.to_string()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (sequence, owner, state) in rows {
        let Ok(compaction_id) = owner.parse() else { continue };
        let ended = |outcome| PluginSessionEventKind::CompactionEnded { compaction_id, outcome };
        let event = match state.as_str() {
            "running" => PluginSessionEventKind::CompactionStarted { compaction_id },
            "completed" => ended(PluginCompactionOutcome::Completed),
            "failed" => ended(PluginCompactionOutcome::Failed),
            "cancelled" => ended(PluginCompactionOutcome::Cancelled),
            "interrupted" => ended(PluginCompactionOutcome::Interrupted),
            _ => continue,
        };
        events.push(PluginSessionEvent { sequence: unsigned(sequence)?, event });
    }
    Ok(())
}

/// What the conversation's finished requests used, as their Providers reported. Each finished
/// step names the response it recorded; the usage is read from there.
pub fn usage(db: &Connection, session: SessionId) -> StoreResult<PluginSessionUsage> {
    let (requests, input, cached, output): (i64, i64, i64, i64) = db.query_row(
        "SELECT count(*), \
         coalesce(sum(json_extract(e.payload, '$.usage.input_tokens')), 0), \
         coalesce(sum(json_extract(e.payload, '$.usage.cached_input_tokens')), 0), \
         coalesce(sum(json_extract(e.payload, '$.usage.output_tokens')), 0) \
         FROM turn_tasks t JOIN agent_runs r ON r.turn_task_id=t.id \
         JOIN model_steps m ON m.agent_run_id=r.id AND m.state='completed' \
         JOIN events e ON e.event_id=json_extract(m.payload, '$.state.assistant_event_id') \
         WHERE t.session_id=?1",
        [session.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    let count = |value: i64| u64::try_from(value).unwrap_or_default();
    Ok(PluginSessionUsage {
        requests: count(requests),
        input_tokens: count(input),
        cached_input_tokens: count(cached),
        output_tokens: count(output),
    })
}

/// The newest turn of the conversation. Turn ids are ordered by when they were made.
pub fn newest_turn(db: &Connection, session: SessionId) -> StoreResult<Option<TurnTaskId>> {
    db.query_row(
        "SELECT id FROM turn_tasks WHERE session_id=?1 ORDER BY id DESC LIMIT 1",
        [session.to_string()],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|id| id.parse().map_err(|_| Error::Conflict))
    .transpose()
}

/// The newest turn's to-do list.
pub fn todo(db: &Connection, session: SessionId) -> StoreResult<Vec<TodoItem>> {
    Ok(match newest_turn(db, session)? {
        Some(turn) => work::todo(db, turn)?.items,
        None => Vec::new(),
    })
}

/// The newest measured request's context: the figure the composer's meter shows.
pub fn context_use(
    db: &Connection,
    session: SessionId,
) -> StoreResult<Option<PluginSessionContextUse>> {
    let mut steps = db.prepare(
        "SELECT m.id FROM turn_tasks t \
         JOIN model_steps m ON m.agent_run_id=json_extract(t.payload, '$.agent_run_id') \
         WHERE t.session_id=?1 ORDER BY t.id DESC, m.ordinal DESC LIMIT ?2",
    )?;
    let ids = steps
        .query_map(params![session.to_string(), CONTEXT_STEPS], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids.iter().find_map(|step| desktop_messages::context_meter(db, step)).map(|usage| {
        PluginSessionContextUse {
            window_tokens: usage.runtime_effective_window_tokens,
            used_tokens: usage.used_input_tokens,
            estimated: usage.estimated,
        }
    }))
}

fn position(value: u64) -> StoreResult<i64> {
    i64::try_from(value).map_err(|_| Error::Conflict)
}

fn unsigned(sequence: i64) -> StoreResult<u64> {
    u64::try_from(sequence).map_err(|_| Error::Conflict)
}

/// A row that is gone is `None`; any other failure stays an error.
fn found<T>(read: StoreResult<T>) -> StoreResult<Option<T>> {
    match read {
        Ok(value) => Ok(Some(value)),
        Err(Error::Sqlite(rusqlite::Error::QueryReturnedNoRows)) => Ok(None),
        Err(error) => Err(error),
    }
}
