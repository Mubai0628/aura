use super::*;

pub const PAGE_SIZE: usize = 30;

pub fn recent_in_run(
    db: &Connection,
    workspace: WorkspaceId,
    session: SessionId,
    root: EventId,
) -> StoreResult<Vec<Delivery>> {
    let mut stmt = db.prepare("SELECT d.id FROM agent_collaboration_deliveries d JOIN agent_collaboration_dispatches c USING(call_id) JOIN agent_collaboration_runs r USING(root_event_id) WHERE c.root_event_id=?1 AND r.workspace_id=?2 AND (d.target_session_id=?3 OR c.source_session_id=?3) ORDER BY d.rowid DESC LIMIT 8")?;
    let ids = parse_ids(stmt.query_map(
        params![root.to_string(), workspace.to_string(), session.to_string()],
        |row| row.get(0),
    )?)?;
    ids.into_iter().map(|id| load(db, id)?.ok_or(Error::Conflict)).collect()
}

pub fn default_budget(db: &Connection) -> StoreResult<PeerBudgetDefault> {
    Ok(db
        .query_row(
            "SELECT dispatch_limit,revision FROM agent_collaboration_budget_default WHERE id=1",
            [],
            |row| {
                Ok(PeerBudgetDefault {
                    limit: row.get(0)?,
                    revision: u64::from(row.get::<_, u32>(1)?),
                })
            },
        )
        .optional()?
        .unwrap_or(PeerBudgetDefault { revision: 0, limit: DEFAULT_PEER_BUDGET }))
}

pub fn latest_outgoing_in_run(
    db: &Connection,
    workspace: WorkspaceId,
    session: SessionId,
    root: EventId,
) -> StoreResult<Option<EventId>> {
    let mut stmt = db.prepare(
        "SELECT d.id FROM agent_collaboration_deliveries d
         JOIN agent_collaboration_dispatches c USING(call_id)
         JOIN agent_collaboration_runs r USING(root_event_id)
         WHERE c.root_event_id=?1 AND r.workspace_id=?2 AND c.source_session_id=?3
         ORDER BY d.rowid DESC LIMIT 1",
    )?;
    let ids = parse_ids(stmt.query_map(
        params![root.to_string(), workspace.to_string(), session.to_string()],
        |row| row.get(0),
    )?)?;
    Ok(ids.into_iter().next())
}

pub fn budget(db: &Connection, workspace: WorkspaceId, root: EventId) -> StoreResult<PeerBudget> {
    let found = db.query_row("SELECT r.used,r.dispatch_limit,coalesce(x.revision,0),coalesce(x.stopped,0) FROM agent_collaboration_runs r LEFT JOIN agent_collaboration_run_controls x USING(root_event_id) WHERE r.root_event_id=?1 AND r.workspace_id=?2",
        params![root.to_string(),workspace.to_string()], |r| Ok(PeerBudget {
            root_event_id: root, used: r.get(0)?, limit: r.get(1)?, revision: u64::from(r.get::<_, u32>(2)?), stopped: r.get(3)?,
        })).optional()?;
    Ok(match found {
        Some(found) => found,
        None => PeerBudget {
            root_event_id: root,
            used: 0,
            limit: default_budget(db)?.limit,
            revision: 0,
            stopped: false,
        },
    })
}

pub fn set_default(tx: &Transaction<'_>, expected: u64, limit: u32) -> StoreResult<()> {
    let current = default_budget(tx)?;
    if current.revision != expected {
        return Err(Error::Conflict);
    }
    if !(1..=MAX_PEER_BUDGET).contains(&limit) {
        return Err(Error::Invalid("peers.budget_invalid: 默认额度超出允许范围。"));
    }
    if limit != current.limit {
        tx.execute("INSERT INTO agent_collaboration_budget_default VALUES(1,1,?1) ON CONFLICT(id) DO UPDATE SET dispatch_limit=excluded.dispatch_limit,revision=revision+1", [limit])?;
    }
    Ok(())
}

pub fn increase(
    tx: &Transaction<'_>,
    origin: &PeerOrigin,
    expected: u64,
    limit: u32,
) -> StoreResult<()> {
    let current = budget(tx, origin.workspace_id, origin.root_event_id)?;
    if current.revision != expected || current.stopped {
        return Err(Error::Conflict);
    }
    if limit <= current.limit || limit > MAX_PEER_BUDGET {
        return Err(Error::Invalid(
            "peers.budget_invalid: 新额度必须高于当前上限，且不得超出允许范围。",
        ));
    }
    tx.execute(
        "UPDATE agent_collaboration_runs SET dispatch_limit=?2 WHERE root_event_id=?1",
        params![origin.root_event_id.to_string(), limit],
    )?;
    advance(tx, origin.root_event_id, false)
}

pub fn stop(
    tx: &Transaction<'_>,
    origin: &PeerOrigin,
    expected: u64,
) -> StoreResult<Vec<Delivery>> {
    let current = budget(tx, origin.workspace_id, origin.root_event_id)?;
    if !current.stopped {
        if current.revision != expected {
            return Err(Error::Conflict);
        }
        advance(tx, origin.root_event_id, true)?;
    }
    let mut reads = tx.prepare("SELECT d.id FROM agent_collaboration_deliveries d JOIN agent_collaboration_dispatches c USING(call_id) WHERE c.root_event_id=?1 ORDER BY d.rowid")?;
    let ids = parse_ids(reads.query_map([origin.root_event_id.to_string()], |r| r.get(0))?)?;
    ids.into_iter().map(|id| load(tx, id)?.ok_or(Error::Conflict)).collect()
}

fn advance(tx: &Transaction<'_>, root: EventId, stopped: bool) -> StoreResult<()> {
    tx.execute("INSERT INTO agent_collaboration_run_controls VALUES(?1,1,?2) ON CONFLICT(root_event_id) DO UPDATE SET revision=revision+1,stopped=excluded.stopped", params![root.to_string(),stopped])?;
    Ok(())
}

pub fn cancel_queued(tx: &Transaction<'_>, delivery: &Delivery) -> StoreResult<bool> {
    let id = delivery.id.to_string();
    if tx.execute("DELETE FROM agent_collaboration_outbox WHERE delivery_id=?1", [&id])? == 0 {
        return Ok(false);
    }
    tx.execute("INSERT INTO agent_collaboration_cancelled VALUES(?1)", [&id])?;
    Ok(true)
}

/// Cursor ownership is checked before using the opaque delivery position.
pub fn mailbox(
    db: &Connection,
    session: SessionId,
    workspace: WorkspaceId,
    before: Option<EventId>,
    active_only: bool,
) -> StoreResult<(Vec<Delivery>, Option<EventId>)> {
    let before = before
        .map(|id| {
            let d = load(db, id)?.ok_or(Error::Conflict)?;
            if d.origin.workspace_id != workspace
                || (d.target != session && d.sender.session_id != session)
            {
                return Err(Error::Conflict);
            }
            Ok(id.to_string())
        })
        .transpose()?;
    let mut stmt = db.prepare(
        "SELECT d.id FROM agent_collaboration_deliveries d
         JOIN agent_collaboration_dispatches c USING(call_id)
         JOIN agent_collaboration_runs r USING(root_event_id)
         WHERE (c.source_session_id=?1 OR d.target_session_id=?1) AND r.workspace_id=?2
           AND (?3 IS NULL OR d.rowid<(SELECT rowid FROM agent_collaboration_deliveries WHERE id=?3))
           AND (NOT ?5 OR (
               NOT EXISTS(SELECT 1 FROM agent_collaboration_cancelled cancelled WHERE cancelled.delivery_id=d.id)
               AND (d.target_turn_id IS NULL OR EXISTS(
                   SELECT 1 FROM turn_tasks t WHERE t.id=d.target_turn_id
                   AND t.state IN ('pending','running','awaiting_approval','awaiting_query')
               ))
           ))
         ORDER BY d.rowid DESC LIMIT ?4",
    )?;
    let mut ids = parse_ids(stmt.query_map(
        params![
            session.to_string(),
            workspace.to_string(),
            before,
            PAGE_SIZE as i64 + 1,
            active_only
        ],
        |row| row.get(0),
    )?)?;
    let next = if ids.len() > PAGE_SIZE {
        ids.truncate(PAGE_SIZE);
        ids.last().copied()
    } else {
        None
    };
    let deliveries = ids
        .into_iter()
        .map(|id| load(db, id)?.ok_or(Error::Conflict))
        .collect::<StoreResult<_>>()?;
    Ok((deliveries, next))
}
