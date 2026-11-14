//! The conversations with a turn under way, for the sidebar. Turns are few next to the
//! events, and a finished turn leaves this set; the query reads the turns table, never the
//! journal, and reaches each turn's own run by its key: the sidebar asks every 1.5 s, and this
//! took 1.9 ms with 5,000 turns and 13 ms with 30,000, a third of a join on the runs' unindexed
//! turn column. The turns a caller saw under way are looked up by key to say how they ended.
//! A sub-agent's turn counts for the conversation that started it, reached by the member's key:
//! that conversation runs or waits while it does, and the sub-agent's own turns are never listed.
use aura_agent_domain::{
    AgentSessionActivity, AgentSessionActivityState, AgentSessionWaitingFor, AgentSessionsActivity,
    AgentTurnEnded, AgentTurnOutcome,
};
use aura_domain_common::{SessionId, TurnTaskId};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, OptionalExtension};
use std::collections::BTreeMap;

/// Ended turns read in one request at most.
const ENDED_READ: usize = 64;

pub fn sessions(db: &Connection) -> StoreResult<AgentSessionsActivity> {
    activity(db, &[])
}

/// The conversations with a turn under way, and how the named turns that are no longer under
/// way ended.
pub fn activity(db: &Connection, ended_turns: &[TurnTaskId]) -> StoreResult<AgentSessionsActivity> {
    let mut query = db.prepare(
        "SELECT coalesce(m.parent_session_id, t.session_id), t.id, t.state, ar.state, \
         m.parent_session_id IS NOT NULL FROM turn_tasks t \
         LEFT JOIN agent_runs ar ON ar.id=json_extract(t.payload, '$.agent_run_id') \
         LEFT JOIN agent_team_members m ON m.child_session_id=t.session_id \
         WHERE t.state IN ('pending','running','awaiting_approval','awaiting_query')",
    )?;
    let rows = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    // A conversation waits for the user when any of its turns does, showing the wait that
    // comes first (`AgentSessionWaitingFor::first`).
    let mut states = BTreeMap::<String, (Option<AgentSessionWaitingFor>, Vec<TurnTaskId>)>::new();
    for (session, turn, turn_state, run, by_subagent) in rows {
        let waiting = match (turn_state.as_str(), run.as_deref()) {
            ("awaiting_approval", _) => Some(AgentSessionWaitingFor::Approval),
            ("awaiting_query", _) => Some(AgentSessionWaitingFor::Question),
            (_, Some("awaiting_context_remediation")) => Some(AgentSessionWaitingFor::Context),
            _ => None,
        };
        let entry = states.entry(session).or_default();
        entry.0 = match (entry.0, waiting) {
            (Some(known), Some(found)) => Some(known.first(found)),
            (known, found) => known.or(found),
        };
        if !by_subagent {
            entry.1.push(turn.parse::<TurnTaskId>().map_err(|_| Error::Conflict)?);
        }
    }
    let sessions = states
        .into_iter()
        .map(|(session, (waiting_for, turn_task_ids))| {
            Ok(AgentSessionActivity {
                session_id: session.parse::<SessionId>().map_err(|_| Error::Conflict)?,
                state: if waiting_for.is_some() {
                    AgentSessionActivityState::Waiting
                } else {
                    AgentSessionActivityState::Running
                },
                turn_task_ids,
                waiting_for,
            })
        })
        .collect::<StoreResult<Vec<_>>>()?;
    Ok(AgentSessionsActivity { sessions, ended: ended(db, ended_turns)? })
}

fn ended(db: &Connection, turns: &[TurnTaskId]) -> StoreResult<Vec<AgentTurnEnded>> {
    let mut query = db.prepare("SELECT session_id, state FROM turn_tasks WHERE id=?1")?;
    let mut ended = Vec::new();
    for turn in turns.iter().take(ENDED_READ) {
        let Some((session, state)) = query
            .query_row([turn.to_string()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .optional()?
        else {
            continue;
        };
        let outcome = match state.as_str() {
            "completed" => AgentTurnOutcome::Completed,
            "failed" => AgentTurnOutcome::Failed,
            "cancelled" => AgentTurnOutcome::Cancelled,
            "recovery_required" => AgentTurnOutcome::RecoveryRequired,
            _ => continue,
        };
        ended.push(AgentTurnEnded {
            session_id: session.parse::<SessionId>().map_err(|_| Error::Conflict)?,
            turn_task_id: *turn,
            outcome,
        });
    }
    Ok(ended)
}
