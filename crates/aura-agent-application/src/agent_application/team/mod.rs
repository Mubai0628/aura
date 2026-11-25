//! Sub-agents in the application: the team's calls, the wait before an `agents.wait` call is
//! admitted, what a sub-agent's ended turn and a stopped turn mean for the team, the limits the
//! user sets, the watchdog's clock and what a restart leaves of the sub-agents.
mod admission;
mod allowance;
mod answers;
mod host;
mod limits;
mod recovery;
mod registry;
#[cfg(any(test, feature = "test-hooks"))]
mod start_test_gate;
mod worktree_host;
mod worktrees;

use super::*;
pub(super) use admission::{can_start, is_control_pause, require_start};
use aura_agent_team::{
    AgentTeamAllowanceRevokeRequest, AgentTeamListRequest, AgentTeamStopRequest, AgentTeamView,
    AgentTeamWorktreeFolder, AgentTeamWorktreeRequest, AgentsWaitInput, Team, TeamCall, TeamFuture,
    TeamTools, store as team_store,
};
pub(super) use recovery::recover_after_restart;
pub(super) use registry::{restrict_candidates, team_member};
pub(super) use worktrees::restore_worktree_links;

/// The team's steps inside every turn are boxed: the turn's own futures and debug-build frames
/// then grow by a pointer, not by the team's state. A compaction's path is already deep.
type Boxed<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// The application as the team's host.
pub(super) struct TeamApp(pub Arc<AgentApplication>);

impl TeamTools for AgentApplication {
    fn call(self: Arc<Self>, call: TeamCall) -> TeamFuture {
        Box::pin(async move {
            if call.tool.starts_with("peers.") {
                return self.peer_tool(call).await;
            }
            let sets_to_work =
                matches!(call.tool.as_str(), aura_agent_team::SPAWN | aura_agent_team::SEND);
            let host = TeamApp(Arc::clone(&self));
            let outcome = Team::new(&host, self.execution.team.news()).call(call).await;
            if sets_to_work && outcome.is_ok() {
                self.watch_team();
            }
            outcome
        })
    }

    fn watch(self: Arc<Self>, elapsed: std::time::Duration) -> aura_agent_team::WatchFuture {
        self.watch_once(elapsed)
    }
}

impl AgentApplication {
    /// Lets the execution layer reach this application's team.
    pub(super) fn bind_team(self: &Arc<Self>) {
        let tools: std::sync::Weak<dyn TeamTools> = Arc::downgrade(self) as _;
        self.execution.team.bind(tools);
    }

    /// The desktop's `agent.team.*` requests.
    pub async fn team_route(
        self: &Arc<Self>,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(match method {
            "agent.team.profile.get" | "agent.team.profile.set" | "agent.team.profile.preview" => {
                self.profile_route(method, params).await?
            }
            "agent.team.list" => {
                serde_json::to_value(self.team_list(serde_json::from_value(params)?).await?)?
            }
            "agent.team.stop" => {
                serde_json::to_value(self.team_stop(serde_json::from_value(params)?).await?)?
            }
            "agent.team.changes" => {
                serde_json::to_value(self.team_changes(serde_json::from_value(params)?).await?)?
            }
            "agent.team.worktree.folder" => {
                let request: AgentTeamWorktreeRequest = serde_json::from_value(params)?;
                let host = TeamApp(Arc::clone(self));
                let team = Team::new(&host, self.execution.team.news());
                let path = team.worktree_folder(request.session_id, &request.agent).await?;
                serde_json::to_value(AgentTeamWorktreeFolder { path })?
            }
            "agent.team.allowance.revoke" => {
                let request: AgentTeamAllowanceRevokeRequest = serde_json::from_value(params)?;
                let host = TeamApp(Arc::clone(self));
                let team = Team::new(&host, self.execution.team.news());
                team.revoke_allowance(request.session_id, &request.agent, request.allowance)
                    .await?;
                serde_json::to_value(team.list(request.session_id).await?)?
            }
            "agent.team.worktrees.location" => {
                serde_json::to_value(self.worktree_location().await?)?
            }
            "agent.team.worktrees.location.set" => serde_json::to_value(
                self.set_worktree_location(serde_json::from_value(params)?).await?,
            )?,
            "agent.team.graph.get" | "agent.team.graph.mutate" => {
                self.graph_route(method, params).await?
            }
            "agent.team.peers.get" | "agent.team.peers.manage" => {
                self.peer_mailbox_route(method, params).await?
            }
            "agent.team.policy.get" | "agent.team.policy.set" => {
                self.session_policy_route(method, params).await?
            }
            "agent.team.settings" => serde_json::to_value(self.team_settings().await?)?,
            "agent.team.settings.set" => serde_json::to_value(
                self.set_team_settings(serde_json::from_value(params)?).await?,
            )?,
            _ => anyhow::bail!("ipc.route_unknown"),
        })
    }

    /// The sub-agents a conversation started, for the desktop.
    pub async fn team_list(
        self: &Arc<Self>,
        request: AgentTeamListRequest,
    ) -> anyhow::Result<AgentTeamView> {
        let host = TeamApp(Arc::clone(self));
        Team::new(&host, self.execution.team.news()).list(request.session_id).await
    }

    /// The user stops one sub-agent of a conversation; the list as it is then.
    pub async fn team_stop(
        self: &Arc<Self>,
        request: AgentTeamStopRequest,
    ) -> anyhow::Result<AgentTeamView> {
        let host = TeamApp(Arc::clone(self));
        let team = Team::new(&host, self.execution.team.news());
        team.stop_for_user(request.session_id, &request.agent).await?;
        team.list(request.session_id).await
    }

    /// Before the step's calls are admitted, its `agents.wait` calls wait for their reports, so
    /// the wait itself holds no tool slot.
    pub(super) fn prewait_team_calls(
        self: &Arc<Self>,
        run_id: AgentRunId,
        waits: Vec<AgentsWaitInput>,
    ) -> Boxed<'_, anyhow::Result<()>> {
        Box::pin(async move {
            let session = self.turn(run_id).await?.session_id;
            // No call runs while it waits: the list shows the wait from this mark.
            let _waiting = self.execution.team.waiting(session);
            let host = TeamApp(Arc::clone(self));
            let team = Team::new(&host, self.execution.team.news());
            for wait in &waits {
                team.prewait(session, wait).await?;
            }
            Ok(())
        })
    }

    /// A turn ended: a sub-agent's end goes to its team, and a stopped turn stops the sub-agents
    /// its conversation has running.
    pub(super) fn team_turn_ended<'a>(self: &'a Arc<Self>, turn: &'a TurnTask) -> Boxed<'a, ()> {
        Box::pin(async move {
            let Some(end) = recovery::turn_end(turn) else { return };
            let host = TeamApp(Arc::clone(self));
            let team = Team::new(&host, self.execution.team.news());
            let session = turn.session_id;
            let result = match team_member(self, session).await {
                Ok(Some(_)) => {
                    let ended = turn.clone();
                    // Without the words the report names the stop reason as it is.
                    let cause = self
                        .store
                        .read(StoreStep::operation("agents.turn_end.cause"), move |db| {
                            recovery::stop_cause(db, &ended)
                        })
                        .await
                        .ok()
                        .flatten();
                    team.turn_ended(session, end, cause).await
                }
                Ok(None) if end.outcome == "cancelled" => team.stop_running(session).await,
                Ok(None) => Ok(()),
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                append_core_log(
                    "agents.turn_end_failed",
                    serde_json::json!({"session_id": session, "reason": error.to_string()}),
                );
            }
        })
    }
}

/// The step's `agents.wait` calls before its batch is admitted, read on the caller's store section.
pub(super) fn waits_of(
    db: &aura_persistence_core::Connection,
    step_id: ModelStepId,
) -> aura_persistence_core::StoreResult<Vec<AgentsWaitInput>> {
    Ok(aura_persistence_v2::hooks::calls_before_admission(db, step_id)?
        .into_iter()
        .filter(|call| call.tool_name == aura_agent_team::WAIT)
        .filter_map(|call| serde_json::from_value(call.arguments).ok())
        .collect())
}

/// The member a conversation's sub-agent is, read on the caller's store section.
pub(super) fn member_of(
    db: &aura_persistence_core::Connection,
    session: SessionId,
) -> aura_persistence_core::StoreResult<Option<aura_agent_team::Member>> {
    team_store::member_by_child(db, session)
}
