use aura_domain_common::{AgentRunId, SessionId, TurnTaskId};
use aura_persistence_core::{StoreResult, record};
use rusqlite::{Connection, Transaction, params};
use serde_json::Value;

pub mod application;
pub mod approval;
pub mod attachments;
pub mod branch;
pub mod checkpoint;
pub mod completion;
pub mod continuation;
pub mod control;
pub mod counted_request;
pub mod desktop;
pub mod desktop_messages;
mod evidence;
mod facts;
pub mod guard;
pub mod history;
pub mod inbox;
pub mod input_consumption;
pub mod inspector;
pub mod lifecycle;
pub mod lineage;
pub mod mode;
pub mod model_context;
mod network_proxy_notice;
pub mod query;
pub mod reconcile;
pub mod recovery;
pub mod related;
pub mod session;
pub mod skills;
mod source_coverage;
pub mod under_way;
pub use source_coverage::coverage_hops_for_events;
pub mod stream;
pub mod tool_state;
pub mod turns;
pub mod work;

/// Stores exact versioned facts with CAS; R09's Agent Application owns the
/// permitted state machine and the meaning of its payload, not this SQL module.
pub fn insert_turn(
    tx: &Transaction<'_>,
    id: TurnTaskId,
    session: SessionId,
    payload: &Value,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO turn_tasks VALUES(?1,?2,'pending',1,?3)",
        params![id.to_string(), session.to_string(), record::encode(payload)?],
    )?;
    let mut poll = payload.clone();
    if let Some(report) = poll.pointer_mut("/state/final_report") {
        *report = Value::String(String::new());
    }
    tx.execute(
        "INSERT INTO turn_poll(id,revision,state,payload) VALUES(?1,1,'pending',?2)",
        params![id.to_string(), record::encode(&poll)?],
    )?;
    Ok(())
}
pub fn insert_run(
    tx: &Transaction<'_>,
    id: AgentRunId,
    turn: TurnTaskId,
    parent: Option<AgentRunId>,
    payload: &Value,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO agent_runs VALUES(?1,?2,?3,'pending',1,?4)",
        params![
            id.to_string(),
            turn.to_string(),
            parent.map(|p| p.to_string()),
            record::encode(payload)?
        ],
    )?;
    Ok(())
}
pub fn update_turn(
    tx: &Transaction<'_>,
    id: TurnTaskId,
    revision: i64,
    state: &str,
    payload: &Value,
) -> StoreResult<()> {
    record::changed(tx.execute(
        "UPDATE turn_tasks SET state=?3,revision=revision+1,payload=?4 WHERE id=?1 AND revision=?2",
        params![id.to_string(), revision, state, record::encode(payload)?],
    )?)?;
    tx.execute("INSERT INTO turn_state_snapshots VALUES(?1,?2,?3) ON CONFLICT(turn_task_id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload",params![id.to_string(),revision+1,record::encode(payload)?])?;
    let mut poll = payload.clone();
    if let Some(report) = poll.pointer_mut("/state/final_report") {
        *report = Value::String(String::new());
    }
    tx.execute(
        "INSERT INTO turn_poll(id,revision,state,payload) VALUES(?1,?2,?3,?4) \
         ON CONFLICT(id) DO UPDATE SET revision=excluded.revision, state=excluded.state, payload=excluded.payload",
        params![id.to_string(), revision + 1, state, record::encode(&poll)?],
    )?;
    Ok(())
}
pub fn turn(connection: &Connection, id: TurnTaskId) -> StoreResult<(i64, String, Value)> {
    let (revision, state, payload): (i64, String, String) = connection.query_row(
        "SELECT revision,state,payload FROM turn_tasks WHERE id=?1",
        [id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok((revision, state, record::decode(&payload)?))
}
