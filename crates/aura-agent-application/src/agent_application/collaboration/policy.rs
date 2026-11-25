use super::*;
use aura_agent_collaboration::{AgentSessionPolicyRequest, AgentSessionPolicySetRequest, policy};

impl AgentApplication {
    pub(in crate::agent_application) async fn session_policy_route(
        self: &Arc<Self>,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        match method {
            "agent.team.policy.get" => {
                let request: AgentSessionPolicyRequest = serde_json::from_value(params)?;
                self.ensure_session(request.session_id).await?;
                let value = self
                    .store
                    .read(StoreStep::operation("agents.policy.get"), move |db| {
                        policy::load(db, request.session_id)
                    })
                    .await?;
                Ok(serde_json::to_value(value)?)
            }
            "agent.team.policy.set" => {
                let request: AgentSessionPolicySetRequest = serde_json::from_value(params)?;
                self.ensure_session(request.session_id).await?;
                let parent = request.session_id;
                let value = self
                    .store
                    .write(StoreStep::operation("agents.policy.set"), move |journal| {
                        journal
                            .database
                            .unit_of_work(|tx| policy::set(tx, &request, UtcTimestamp::now()))
                    })
                    .await?;
                if value.allow_proactive_temporary_agents {
                    self.wake_temporary_queues(Some(parent)).await?;
                }
                Ok(serde_json::to_value(value)?)
            }
            _ => anyhow::bail!(
                "agents.policy_route_unknown: 找不到这项会话策略操作，请重新打开会话。"
            ),
        }
    }
}
