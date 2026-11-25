//! Durable routing and accounting. All decisions write in the caller's transaction;
//! only the host can authenticate a sender and claim a delivery into a real turn.
use crate::*;
use aura_domain_common::{
    AgentRunId, EventId, SessionId, ToolCallId, TurnTaskId, UtcTimestamp, WorkspaceId,
};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
pub mod controls;
pub mod review;

#[derive(Clone, Debug)]
pub struct Delivery {
    pub id: EventId,
    pub sender: PeerSender,
    pub target: SessionId,
    pub origin: PeerOrigin,
    pub input: PeerDispatchInput,
    pub target_run: Option<AgentRunId>,
    pub kind: PeerDeliveryKind,
    pub cancelled: bool,
}

pub fn receipt_ids(
    db: &Connection,
    sender: PeerSender,
    input: &PeerDispatchInput,
    kind: PeerDeliveryKind,
) -> StoreResult<Option<Vec<EventId>>> {
    let stored = db.query_row(
        "SELECT c.source_session_id,c.source_turn_id,c.request,q.call_id IS NOT NULL FROM agent_collaboration_dispatches c LEFT JOIN agent_collaboration_questions q ON q.call_id=c.call_id WHERE c.call_id=?1",
        [sender.tool_call_id.to_string()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, bool>(3)?)),
    ).optional()?;
    let Some((session, turn, raw, question)) = stored else { return Ok(None) };
    if session != sender.session_id.to_string()
        || turn != sender.turn_task_id.to_string()
        || serde_json::from_str::<PeerDispatchInput>(&raw)? != *input
        || question != (kind == PeerDeliveryKind::Question)
    {
        return Err(Error::Invalid(
            "peers.call_conflict: 原调用身份已绑定不同的协作请求；请按原调用对账。",
        ));
    }
    Ok(Some(by_call(db, sender.session_id, sender.tool_call_id)?))
}

pub fn accept(
    tx: &Transaction<'_>,
    sender: PeerSender,
    input: &PeerDispatchInput,
    kind: PeerDeliveryKind,
    origin: &PeerOrigin,
    at: UtcTimestamp,
) -> StoreResult<Vec<EventId>> {
    if let Some(ids) = receipt_ids(tx, sender, input, kind)? {
        return Ok(ids);
    }
    let targets: std::collections::BTreeSet<_> = input.targets.iter().copied().collect();
    if targets.is_empty()
        || targets.len() != input.targets.len()
        || targets.len() > DEFAULT_PEER_BUDGET as usize
        || (kind == PeerDeliveryKind::Question && targets.len() != 1)
        || input.task.trim().is_empty()
        || input.task.len() > MAX_PEER_TASK
        || origin.sources.is_empty()
    {
        return Err(Error::Invalid("peers.invalid_dispatch: 请填写有界的内容和不重复的目标。"));
    }
    let graph = graph::load(tx, sender.session_id, origin.workspace_id)?;
    if graph.graph_id != Some(input.graph_id) || graph.revision != input.graph_revision {
        return Err(Error::Invalid(
            "peers.graph_changed: 编排图已变化，没有投递。请先读取最新目录。",
        ));
    }
    if targets.iter().any(|target| {
        !graph.edges.iter().any(|edge| {
            (edge.source_session_id == sender.session_id && edge.target_session_id == *target)
                || (kind == PeerDeliveryKind::Question
                    && edge.target_session_id == sender.session_id
                    && edge.source_session_id == *target)
        })
    }) {
        return Err(Error::Invalid("peers.target_ineligible: 当前连线不允许这项操作；没有投递。"));
    }
    let root = origin.root_event_id.to_string();
    let workspace = origin.workspace_id.to_string();
    let limit = controls::default_budget(tx)?.limit;
    tx.execute("INSERT INTO agent_collaboration_runs VALUES(?1,?2,0,?3) ON CONFLICT(root_event_id) DO NOTHING",
        params![root,workspace,limit])?;
    if controls::budget(tx, origin.workspace_id, origin.root_event_id)?.stopped {
        return Err(Error::Invalid("peers.run_stopped: 用户已停止此次协作，没有投递。"));
    }
    let charged = tx.execute("UPDATE agent_collaboration_runs SET used=used+?3 WHERE root_event_id=?1 AND workspace_id=?2 AND used+?3<=dispatch_limit",
        params![root,workspace,targets.len() as u32])?;
    if charged != 1 {
        return Err(Error::Invalid(
            "peers.budget_exhausted: 本次协作的剩余额度不足，未向任何目标投递。已收到的任务不受影响。",
        ));
    }
    let call = sender.tool_call_id.to_string();
    tx.execute(
        "INSERT INTO agent_collaboration_dispatches VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            call,
            sender.session_id.to_string(),
            sender.turn_task_id.to_string(),
            root,
            serde_json::to_string(origin)?,
            serde_json::to_string(input)?,
            at.to_string()
        ],
    )?;
    if kind == PeerDeliveryKind::Question {
        tx.execute("INSERT INTO agent_collaboration_questions VALUES(?1)", [&call])?;
    }
    let mut ids = Vec::with_capacity(targets.len());
    for target in targets {
        let id = EventId::new();
        tx.execute("INSERT INTO agent_collaboration_deliveries(id,call_id,target_session_id) VALUES(?1,?2,?3)",
            params![id.to_string(),call,target.to_string()])?;
        tx.execute(
            "INSERT INTO agent_collaboration_outbox VALUES(?1,?2)",
            params![id.to_string(), target.to_string()],
        )?;
        review::enqueue(tx, id)?;
        ids.push(id);
    }
    Ok(ids)
}

pub fn budget(db: &Connection, origin: &PeerOrigin) -> StoreResult<(u32, u32)> {
    let value = controls::budget(db, origin.workspace_id, origin.root_event_id)?;
    Ok((value.used, value.limit))
}

pub fn by_call(db: &Connection, session: SessionId, call: ToolCallId) -> StoreResult<Vec<EventId>> {
    let mut stmt = db.prepare("SELECT d.id FROM agent_collaboration_deliveries d JOIN agent_collaboration_dispatches c ON c.call_id=d.call_id WHERE c.call_id=?1 AND c.source_session_id=?2 ORDER BY d.rowid")?;
    parse_ids(stmt.query_map(params![call.to_string(), session.to_string()], |row| row.get(0))?)
}

pub fn load(db: &Connection, id: EventId) -> StoreResult<Option<Delivery>> {
    let payload = db.query_row("SELECT d.target_session_id,d.target_run_id,c.call_id,c.source_session_id,c.source_turn_id,c.origin,c.request,q.call_id IS NOT NULL,x.delivery_id IS NOT NULL FROM agent_collaboration_deliveries d JOIN agent_collaboration_dispatches c ON c.call_id=d.call_id LEFT JOIN agent_collaboration_questions q ON q.call_id=c.call_id LEFT JOIN agent_collaboration_cancelled x ON x.delivery_id=d.id WHERE d.id=?1",
        [id.to_string()], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,bool>(7)?,r.get::<_,bool>(8)?))).optional()?;
    payload
        .map(|(target, run, call, session, turn, origin, input, question, cancelled)| {
            Ok(Delivery {
                id,
                target: parse(&target)?,
                target_run: run.as_deref().map(parse).transpose()?,
                sender: PeerSender {
                    session_id: parse(&session)?,
                    turn_task_id: parse(&turn)?,
                    tool_call_id: parse(&call)?,
                },
                origin: serde_json::from_str(&origin)?,
                input: serde_json::from_str(&input)?,
                kind: if question {
                    PeerDeliveryKind::Question
                } else {
                    PeerDeliveryKind::Dispatch
                },
                cancelled,
            })
        })
        .transpose()
}

pub fn pending(db: &Connection, target: SessionId) -> StoreResult<Option<EventId>> {
    let id: Option<String> = db.query_row("SELECT o.delivery_id FROM agent_collaboration_outbox o LEFT JOIN agent_collaboration_reviews r ON r.delivery_id=o.delivery_id WHERE o.target_session_id=?1 AND (r.delivery_id IS NULL OR r.reviewed_by IS NOT NULL) ORDER BY o.rowid LIMIT 1",
        [target.to_string()], |r| r.get(0)).optional()?;
    id.as_deref().map(parse).transpose()
}

pub fn receivers(db: &Connection) -> StoreResult<Vec<SessionId>> {
    let mut stmt = db.prepare("SELECT DISTINCT target_session_id FROM agent_collaboration_outbox ORDER BY target_session_id")?;
    stmt.query_map([], |r| r.get::<_, String>(0))?.map(|id| parse(&id?)).collect::<StoreResult<_>>()
}

/// Terminal deliveries whose requestor has not opened the result yet.
pub fn unseen_results(db: &Connection) -> StoreResult<Vec<(SessionId, EventId)>> {
    let mut stmt = db.prepare("SELECT c.source_session_id,d.id FROM agent_collaboration_deliveries d JOIN agent_collaboration_dispatches c USING(call_id) LEFT JOIN agent_collaboration_result_seen s ON s.delivery_id=d.id WHERE s.delivery_id IS NULL AND d.target_run_id IS NOT NULL ORDER BY d.rowid DESC LIMIT 64")?;
    stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .map(|row| {
            let (session, id) = row?;
            Ok((parse(&session)?, parse(&id)?))
        })
        .collect()
}

/// Only the requestor may dismiss its own result prompt. The receipt and result
/// remain readable; this writes no new message and consumes no collaboration unit.
pub fn mark_result_seen(
    tx: &Transaction<'_>,
    delivery: &Delivery,
    session: SessionId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if delivery.sender.session_id != session {
        return Err(Error::Conflict);
    }
    tx.execute(
        "INSERT OR IGNORE INTO agent_collaboration_result_seen VALUES(?1,?2,?3)",
        params![delivery.id.to_string(), session.to_string(), at.to_string()],
    )?;
    Ok(())
}

/// A missing model is retried on a bounded, persisted schedule. Busy, paused
/// and disabled receivers stay event-driven and do not consume attempts.
pub fn retry_due(db: &Connection, id: EventId) -> StoreResult<bool> {
    Ok(retry_deadline(db, id)?.is_none_or(|deadline| deadline <= UtcTimestamp::now()))
}

pub fn retry_deadline(db: &Connection, id: EventId) -> StoreResult<Option<UtcTimestamp>> {
    let deadline: Option<String> = db
        .query_row(
            "SELECT next_at FROM agent_collaboration_retry WHERE delivery_id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    deadline.map(|at| at.parse().map_err(|_| Error::Conflict)).transpose()
}

pub fn retry_attempts(db: &Connection, id: EventId) -> StoreResult<u32> {
    Ok(db
        .query_row(
            "SELECT attempts FROM agent_collaboration_retry WHERE delivery_id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

pub fn record_retry(tx: &Transaction<'_>, id: EventId, reason: &str) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO agent_collaboration_retry VALUES(?1,1,?2,strftime('%Y-%m-%dT%H:%M:%fZ','now','+2 seconds')) ON CONFLICT(delivery_id) DO UPDATE SET attempts=attempts+1,last_reason=excluded.last_reason,next_at=strftime('%Y-%m-%dT%H:%M:%fZ','now','+' || min(60,2*(attempts+1)) || ' seconds')",
        params![id.to_string(), reason],
    )?;
    Ok(())
}

pub fn clear_retry(tx: &Transaction<'_>, id: EventId) -> StoreResult<()> {
    tx.execute("DELETE FROM agent_collaboration_retry WHERE delivery_id=?1", [id.to_string()])?;
    Ok(())
}

/// Removing outbox and recording the receiver run must commit with creating that exact run.
/// An uncertain claimant must query that binding, not create another turn.
pub fn claim(
    tx: &Transaction<'_>,
    delivery: &Delivery,
    turn: TurnTaskId,
    run: AgentRunId,
) -> StoreResult<()> {
    if delivery.cancelled
        || review::waiting(tx, delivery.id)?
        || controls::budget(tx, delivery.origin.workspace_id, delivery.origin.root_event_id)?
            .stopped
    {
        return Err(Error::Conflict);
    }
    let removed = tx.execute(
        "DELETE FROM agent_collaboration_outbox WHERE delivery_id=?1 AND target_session_id=?2",
        params![delivery.id.to_string(), delivery.target.to_string()],
    )?;
    let bound = tx.execute("UPDATE agent_collaboration_deliveries SET target_turn_id=?3,target_run_id=?4 WHERE id=?1 AND target_session_id=?2 AND target_run_id IS NULL",
        params![delivery.id.to_string(),delivery.target.to_string(),turn.to_string(),run.to_string()])?;
    if removed == 1 && bound == 1 { clear_retry(tx, delivery.id) } else { Err(Error::Conflict) }
}

pub fn for_run(db: &Connection, run: AgentRunId) -> StoreResult<Option<Delivery>> {
    let id: Option<String> = db
        .query_row(
            "SELECT id FROM agent_collaboration_deliveries WHERE target_run_id=?1",
            [run.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => Ok(Some(load(db, parse(&id)?)?.ok_or(Error::Conflict)?)),
        None => Ok(None),
    }
}

pub fn latest_between(
    db: &Connection,
    workspace: WorkspaceId,
    edge: AgentGraphEdge,
) -> StoreResult<Option<Delivery>> {
    let id: Option<String> = db.query_row(
        "SELECT d.id FROM agent_collaboration_deliveries d JOIN agent_collaboration_dispatches c ON c.call_id=d.call_id JOIN agent_collaboration_runs r ON r.root_event_id=c.root_event_id WHERE d.target_session_id=?1 AND c.source_session_id=?2 AND r.workspace_id=?3 ORDER BY d.rowid DESC LIMIT 1",
        params![edge.target_session_id.to_string(), edge.source_session_id.to_string(), workspace.to_string()], |r|r.get(0),
    ).optional()?;
    match id {
        Some(id) => load(db, parse(&id)?),
        None => Ok(None),
    }
}

fn parse_ids(rows: impl Iterator<Item = rusqlite::Result<String>>) -> StoreResult<Vec<EventId>> {
    rows.map(|id| parse(&id?)).collect()
}
fn parse<T: std::str::FromStr>(id: &str) -> StoreResult<T> {
    id.parse().map_err(|_| Error::Conflict)
}
