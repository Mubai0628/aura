use super::*;

pub fn apply(
    tx: &Transaction<'_>,
    input: &AgentGraphMutationRequest,
    access: &GraphAccess,
    at: UtcTimestamp,
) -> StoreResult<AgentGraph> {
    if !access.ordinary_sessions.contains(&input.session_id) {
        return Err(invalid(
            "agents.graph_session_ineligible: 只能编排同一工作区的普通会话，未修改图；请重新选择。",
        ));
    }
    let mut current = load(tx, input.session_id, access.workspace_id)?;
    if !access.permits(&current) {
        return Err(invalid(
            "agents.graph_scope_changed: 图中成员的工作区或会话状态已经改变，未修改；请重新读取。",
        ));
    }
    if let AgentGraphAction::Merge { other_session_id, expected_other, confirmed, positions } =
        &input.action
    {
        if !access.ordinary_sessions.contains(other_session_id) {
            return Err(invalid(
                "agents.graph_cross_workspace: 只能合并同一工作区的普通会话，未修改图。",
            ));
        }
        let other = load(tx, *other_session_id, access.workspace_id)?;
        if input.session_id == *other_session_id
            || current.graph_id.is_some() && current.graph_id == other.graph_id
        {
            return Ok(current);
        }
        check_expected(&current, input.expected_graph)?;
        check_expected(&other, *expected_other)?;
        if !access.permits(&other) {
            return Err(Error::Conflict);
        }
        let mut combined = current.clone();
        combined.sessions.extend_from_slice(&other.sessions);
        access.validate_positions(&combined, positions)?;
        return membership::merge(tx, &current, &other, *confirmed, positions, at);
    }
    check_expected(&current, input.expected_graph)?;
    let roots = current.sessions.iter().copied().collect::<BTreeSet<_>>();
    current.positions.retain(|position| {
        access.root_of(position.session_id).is_some_and(|root| roots.contains(&root))
    });
    let mut next = current.clone();
    let reason = match &input.action {
        AgentGraphAction::Remove { session_id } => {
            membership::remove(tx, &current, *session_id, access, at)?;
            return load(tx, input.session_id, access.workspace_id);
        }
        AgentGraphAction::Connect { source_session_id, target_session_id }
        | AgentGraphAction::Disconnect { source_session_id, target_session_id } => {
            if source_session_id == target_session_id {
                return Err(invalid("agents.graph_self_edge: 不能把会话连到自己，未建立连线。"));
            }
            if !current.sessions.contains(source_session_id)
                || !current.sessions.contains(target_session_id)
            {
                return Err(invalid(
                    "agents.graph_edge_outside: 连线两端必须都在当前图中，未修改连线。",
                ));
            }
            let edge = AgentGraphEdge {
                source_session_id: *source_session_id,
                target_session_id: *target_session_id,
            };
            if matches!(input.action, AgentGraphAction::Connect { .. }) {
                if next.edges.contains(&edge) {
                    return Err(invalid(
                        "agents.graph_edge_exists: 这条有向连线已经存在，未重复建立。",
                    ));
                }
                next.edges.push(edge);
                "connect"
            } else {
                next.edges.retain(|existing| existing != &edge);
                "disconnect"
            }
        }
        AgentGraphAction::Place { positions } => {
            access.validate_positions(&current, positions)?;
            let mut placed = next
                .positions
                .iter()
                .map(|position| (position.session_id, *position))
                .collect::<BTreeMap<_, _>>();
            for position in positions {
                placed.insert(position.session_id, *position);
            }
            next.positions = placed.into_values().collect();
            "place"
        }
        AgentGraphAction::ResetLayout => {
            next.positions.clear();
            "reset layout"
        }
        AgentGraphAction::Merge { .. } => return Err(Error::Conflict),
    };
    if next.edges == current.edges && next.positions == current.positions {
        return Ok(current);
    }
    store::save_one(tx, &current, &mut next, reason, at)?;
    Ok(next)
}
