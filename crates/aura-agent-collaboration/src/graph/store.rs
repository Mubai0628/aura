use super::*;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Data {
    edges: Vec<AgentGraphEdge>,
    positions: Vec<AgentGraphPosition>,
}

pub fn load(
    db: &Connection,
    session: SessionId,
    workspace: WorkspaceId,
) -> StoreResult<AgentGraph> {
    let id: Option<String> = db
        .query_row(
            "SELECT graph_id FROM agent_collaboration_graph_members WHERE session_id=?1",
            [session.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(id) = id else {
        return Ok(singleton(session, workspace));
    };
    let id = id.parse().map_err(|_| Error::Invalid("graph id"))?;
    let graph = by_id(db, id)?;
    if graph.workspace_id != workspace {
        return Err(invalid(
            "agents.graph_workspace_changed: 会话工作区与编排记录不一致，未修改画布；请先核对会话所在工作区。",
        ));
    }
    Ok(graph)
}
pub(super) fn singleton(session: SessionId, workspace: WorkspaceId) -> AgentGraph {
    AgentGraph {
        graph_id: None,
        workspace_id: workspace,
        revision: 0,
        sessions: vec![session],
        edges: vec![],
        positions: vec![],
    }
}
fn by_id(db: &Connection, id: AgentCollaborationGraphId) -> StoreResult<AgentGraph> {
    let (workspace, revision, raw): (String, i64, String) = db.query_row(
        "SELECT workspace_id,revision,payload FROM agent_collaboration_graphs WHERE id=?1",
        [id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let data: Data = serde_json::from_str(&raw)?;
    let mut statement = db.prepare("SELECT session_id FROM agent_collaboration_graph_members WHERE graph_id=?1 ORDER BY session_id")?;
    let sessions = statement
        .query_map([id.to_string()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|id| id.parse().map_err(|_| Error::Invalid("graph session")))
        .collect::<StoreResult<Vec<SessionId>>>()?;
    let edges = data
        .edges
        .into_iter()
        .filter(|edge| {
            sessions.contains(&edge.source_session_id) && sessions.contains(&edge.target_session_id)
        })
        .collect();
    Ok(AgentGraph {
        graph_id: Some(id),
        workspace_id: workspace.parse().map_err(|_| Error::Invalid("graph workspace"))?,
        revision: u64::try_from(revision).map_err(|_| Error::Conflict)?,
        sessions,
        edges,
        positions: data.positions,
    })
}

pub(super) fn persist(
    tx: &Transaction<'_>,
    before: &AgentGraph,
    next: &mut AgentGraph,
    reason: &str,
    at: UtcTimestamp,
) -> StoreResult<AgentCollaborationGraphId> {
    let id = before.graph_id.unwrap_or_default();
    next.graph_id = Some(id);
    next.revision = before.revision.checked_add(1).ok_or(Error::Conflict)?;
    next.sessions.sort();
    next.sessions.dedup();
    next.edges.sort();
    next.positions.sort_by_key(|position| position.session_id);
    let data = Data { edges: next.edges.clone(), positions: next.positions.clone() };
    let revision = i64::try_from(next.revision).map_err(|_| Error::Conflict)?;
    if before.graph_id.is_some() {
        let changed = tx.execute("UPDATE agent_collaboration_graphs SET revision=?2,payload=?3,updated_at=?4 WHERE id=?1 AND revision=?5", params![id.to_string(),revision,serde_json::to_string(&data)?,at.to_string(),i64::try_from(before.revision).map_err(|_| Error::Conflict)?])?;
        if changed != 1 {
            return Err(Error::Conflict);
        }
    } else {
        tx.execute("INSERT INTO agent_collaboration_graphs(id,workspace_id,revision,payload,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5)", params![id.to_string(),next.workspace_id.to_string(),revision,serde_json::to_string(&data)?,at.to_string()])?;
    }
    tx.execute("INSERT INTO agent_collaboration_graph_history(graph_id,revision,snapshot,reason,created_at) VALUES(?1,?2,?3,?4,?5)", params![id.to_string(),revision,serde_json::to_string(next)?,reason,at.to_string()])?;
    Ok(id)
}
pub(super) fn assign(
    tx: &Transaction<'_>,
    graph: AgentCollaborationGraphId,
    session: SessionId,
    previous: Option<AgentCollaborationGraphId>,
) -> StoreResult<()> {
    match previous {
        Some(previous) => {
            if tx.execute("UPDATE agent_collaboration_graph_members SET graph_id=?2 WHERE session_id=?1 AND graph_id=?3", params![session.to_string(),graph.to_string(),previous.to_string()])? != 1 { return Err(Error::Conflict); }
        }
        None => { tx.execute("INSERT INTO agent_collaboration_graph_members(session_id,graph_id) VALUES(?1,?2)", params![session.to_string(),graph.to_string()])?; }
    }
    Ok(())
}
pub(super) fn save_one(
    tx: &Transaction<'_>,
    before: &AgentGraph,
    next: &mut AgentGraph,
    reason: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let id = persist(tx, before, next, reason, at)?;
    if before.graph_id.is_none() {
        for session in &next.sessions {
            assign(tx, id, *session, None)?;
        }
    }
    Ok(())
}
