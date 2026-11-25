use super::*;

pub(super) fn merge(
    tx: &Transaction<'_>,
    current: &AgentGraph,
    other: &AgentGraph,
    confirmed: bool,
    positions: &[AgentGraphPosition],
    at: UtcTimestamp,
) -> StoreResult<AgentGraph> {
    if (current.sessions.len() > 1 || other.sessions.len() > 1) && !confirmed {
        return Err(invalid(
            "agents.graph_merge_confirmation: 这会合并已有整张编排图，尚未修改；请先核对双方所有成员并确认。",
        ));
    }
    let mut next = current.clone();
    next.sessions.extend_from_slice(&other.sessions);
    next.edges.extend_from_slice(&other.edges);
    let placed = next
        .positions
        .iter()
        .chain(&other.positions)
        .chain(positions)
        .map(|position| (position.session_id, *position))
        .collect::<BTreeMap<_, _>>();
    next.positions = placed.into_values().collect();
    let id = store::persist(tx, current, &mut next, "merge", at)?;
    if current.graph_id.is_none() {
        for session in &current.sessions {
            store::assign(tx, id, *session, None)?;
        }
    }
    for session in &other.sessions {
        store::assign(tx, id, *session, other.graph_id)?;
    }
    if other.graph_id.is_some() {
        let mut retired = other.clone();
        retired.sessions.clear();
        retired.edges.clear();
        retired.positions.clear();
        store::persist(tx, other, &mut retired, "merged into another graph", at)?;
    }
    Ok(next)
}

pub(super) fn remove(
    tx: &Transaction<'_>,
    current: &AgentGraph,
    leaving: SessionId,
    access: &GraphAccess,
    at: UtcTimestamp,
) -> StoreResult<AgentGraph> {
    if !current.sessions.contains(&leaving) {
        return Err(invalid(
            "agents.graph_member_missing: 这个会话不在当前图中，未移出任何成员；请重新读取。",
        ));
    }
    if current.sessions.len() == 1 {
        return Ok(current.clone());
    }
    let before_split = store::singleton(leaving, current.workspace_id);
    let mut split = before_split.clone();
    split.positions = current
        .positions
        .iter()
        .filter(|position| access.root_of(position.session_id) == Some(leaving))
        .copied()
        .collect();
    let split_id =
        store::persist(tx, &before_split, &mut split, "removed into independent canvas", at)?;
    store::assign(tx, split_id, leaving, current.graph_id)?;
    let mut next = current.clone();
    next.sessions.retain(|session| *session != leaving);
    next.edges
        .retain(|edge| edge.source_session_id != leaving && edge.target_session_id != leaving);
    next.positions.retain(|position| access.root_of(position.session_id) != Some(leaving));
    store::persist(tx, current, &mut next, "remove member", at)?;
    Ok(next)
}
