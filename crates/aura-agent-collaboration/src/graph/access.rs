use super::*;

/// The host resolves these facts from the actual session and temporary-team owners.
pub struct GraphAccess {
    pub workspace_id: WorkspaceId,
    pub ordinary_sessions: BTreeSet<SessionId>,
    pub temporary_owners: BTreeMap<SessionId, SessionId>,
}
impl GraphAccess {
    pub(super) fn root_of(&self, subject: SessionId) -> Option<SessionId> {
        if self.ordinary_sessions.contains(&subject) {
            Some(subject)
        } else {
            self.temporary_owners.get(&subject).copied()
        }
    }
    pub(super) fn permits(&self, graph: &AgentGraph) -> bool {
        graph.workspace_id == self.workspace_id
            && graph.sessions.iter().all(|id| self.ordinary_sessions.contains(id))
    }
    pub(super) fn contains_subject(&self, graph: &AgentGraph, subject: SessionId) -> bool {
        self.root_of(subject).is_some_and(|root| graph.sessions.contains(&root))
    }
    pub(super) fn validate_positions(
        &self,
        graph: &AgentGraph,
        positions: &[AgentGraphPosition],
    ) -> StoreResult<()> {
        let mut seen = BTreeSet::new();
        for position in positions {
            if !seen.insert(position.session_id)
                || !self.contains_subject(graph, position.session_id)
            {
                return Err(invalid(
                    "agents.graph_position_owner: 只能移动本图真实节点，每个节点只写一次；本次没有保存。",
                ));
            }
            if position.x.unsigned_abs() > 1_000_000 || position.y.unsigned_abs() > 1_000_000 {
                return Err(invalid(
                    "agents.graph_position_invalid: 坐标超出画布范围，未移动节点；请重新放置。",
                ));
            }
        }
        Ok(())
    }
}
pub(super) fn invalid(message: &'static str) -> Error {
    Error::Invalid(message)
}
pub(super) fn check_expected(graph: &AgentGraph, expected: AgentGraphRevision) -> StoreResult<()> {
    if graph.expected() != expected {
        return Err(Error::Conflict);
    }
    Ok(())
}
