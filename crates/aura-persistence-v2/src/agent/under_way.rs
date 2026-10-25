//! The conversations with a turn under way, for the sidebar. Turns are few next to the
//! events, and a finished turn leaves this set; the query reads the turns table, never the
//! journal, and reaches each turn's own run by its key: the sidebar asks every 1.5 s, and this
//! took 1.9 ms with 5,000 turns and 13 ms with 30,000, a third of a join on the runs' unindexed
//! turn column.
use aura_agent_domain::{AgentSessionActivity, AgentSessionActivityState, AgentSessionsActivity};
use aura_domain_common::SessionId;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Connection;
use std::collections::BTreeMap;

pub fn sessions(db: &Connection) -> StoreResult<AgentSessionsActivity> {
    let mut query = db.prepare(
        "SELECT t.session_id, t.state, ar.state FROM turn_tasks t \
         LEFT JOIN agent_runs ar ON ar.id=json_extract(t.payload, '$.agent_run_id') \
         WHERE t.state IN ('pending','running','awaiting_approval','awaiting_query')",
    )?;
    let rows = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    // A conversation waits for the user when any of its turns does.
    let mut states = BTreeMap::<String, AgentSessionActivityState>::new();
    for (session, turn, run) in rows {
        let waiting = matches!(turn.as_str(), "awaiting_approval" | "awaiting_query")
            || run.as_deref() == Some("awaiting_context_remediation");
        let state = if waiting {
            AgentSessionActivityState::Waiting
        } else {
            AgentSessionActivityState::Running
        };
        let entry = states.entry(session).or_insert(state);
        if waiting {
            *entry = AgentSessionActivityState::Waiting;
        }
    }
    let sessions = states
        .into_iter()
        .map(|(session, state)| {
            Ok(AgentSessionActivity {
                session_id: session.parse::<SessionId>().map_err(|_| Error::Conflict)?,
                state,
            })
        })
        .collect::<StoreResult<Vec<_>>>()?;
    Ok(AgentSessionsActivity { sessions })
}
