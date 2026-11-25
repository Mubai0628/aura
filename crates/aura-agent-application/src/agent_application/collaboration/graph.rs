use super::*;
use aura_agent_collaboration::{
    AgentGraphAction, AgentGraphMutationRequest, AgentGraphRequest, AgentGraphTeam, AgentGraphView,
    graph,
};
use aura_agent_team::{AgentTeamListRequest, store as teams};
use aura_persistence_core::{Connection, PersistenceCoreError as Error, StoreResult};
use std::collections::{BTreeMap, BTreeSet};

type Bindings = Vec<(SessionId, WorkspaceId)>;
fn access(
    db: &Connection,
    workspace: WorkspaceId,
    bindings: &Bindings,
) -> StoreResult<graph::GraphAccess> {
    let mut ordinary = BTreeSet::new();
    for (session, place) in bindings {
        if *place == workspace && teams::member_by_child(db, *session)?.is_none() {
            ordinary.insert(*session);
        }
    }
    let mut temporary = BTreeMap::new();
    for session in &ordinary {
        for member in teams::members_of(db, *session)? {
            temporary.insert(member.child_session_id, member.parent_session_id);
        }
    }
    Ok(graph::GraphAccess {
        workspace_id: workspace,
        ordinary_sessions: ordinary,
        temporary_owners: temporary,
    })
}

impl AgentApplication {
    pub(in crate::agent_application) async fn graph_route(
        self: &Arc<Self>,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        match method {
            "agent.team.graph.get" => {
                let input: AgentGraphRequest = serde_json::from_value(params)?;
                Ok(serde_json::to_value(self.agent_graph(input.session_id).await?)?)
            }
            "agent.team.graph.mutate" => {
                let input: AgentGraphMutationRequest = serde_json::from_value(params)?;
                self.ensure_session(input.session_id).await?;
                if let AgentGraphAction::Merge { other_session_id, .. } = &input.action {
                    self.ensure_session(*other_session_id).await?;
                }
                let projection = self.execution.persistence.workspace_session_projection()?;
                let bindings = projection
                    .sessions
                    .iter()
                    .map(|session| (session.session_id, session.workspace_id))
                    .collect::<Bindings>();
                let workspace = bindings
                    .iter()
                    .find(|(session, _)| *session == input.session_id)
                    .map(|(_, workspace)| *workspace)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "agents.graph_session_missing: 找不到这个会话，未修改编排图。"
                        )
                    })?;
                let session = input.session_id;
                self.store
                    .write(StoreStep::operation("agents.graph.mutate"), move |journal| {
                        journal.database.unit_of_work(|tx| {
                            let access = access(tx, workspace, &bindings)?;
                            graph::apply(tx, &input, &access, UtcTimestamp::now())
                        })
                    })
                    .await?;
                Ok(serde_json::to_value(self.agent_graph(session).await?)?)
            }
            _ => anyhow::bail!("agents.graph_route_unknown: 找不到这项画布操作，请重新打开画布。"),
        }
    }

    pub async fn agent_graph(
        self: &Arc<Self>,
        session: SessionId,
    ) -> anyhow::Result<AgentGraphView> {
        self.ensure_session(session).await?;
        let projection = self.execution.persistence.workspace_session_projection()?;
        let bindings = projection
            .sessions
            .iter()
            .map(|session| (session.session_id, session.workspace_id))
            .collect::<Bindings>();
        let workspace = bindings
            .iter()
            .find(|(id, _)| *id == session)
            .map(|(_, workspace)| *workspace)
            .ok_or_else(|| {
                anyhow::anyhow!("agents.graph_session_missing: 找不到这个会话，请重新选择。")
            })?;
        let (graph, candidates, peer_activity) = self.store.read(StoreStep::operation("agents.graph.get"), move |db| {
            let access = access(db, workspace, &bindings)?;
            if !access.ordinary_sessions.contains(&session) { return Err(Error::Invalid("agents.graph_session_ineligible: 临时子 Agent 随原主会话显示，不能单独加入编排图。")); }
            let graph = graph::load(db, session, workspace)?;
            let peer_activity = super::peers::graph_activity(db, &graph)?;
            Ok((graph, access.ordinary_sessions, peer_activity))
        }).await?;
        let mut profiles = Vec::new();
        let mut temporary_teams = Vec::new();
        for member in &graph.sessions {
            profiles.push(self.agent_profile(*member).await?);
            temporary_teams.push(AgentGraphTeam {
                parent_session_id: *member,
                team: self.team_list(AgentTeamListRequest { session_id: *member }).await?,
            });
        }
        let mut available = Vec::new();
        for candidate in candidates {
            if !graph.sessions.contains(&candidate) {
                available.push(self.agent_profile(candidate).await?);
            }
        }
        Ok(AgentGraphView {
            graph,
            profiles,
            teams: temporary_teams,
            candidates: available,
            peer_activity,
        })
    }
}
