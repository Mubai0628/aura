use super::*;
use aura_agent_collaboration::{
    AgentProfile, AgentProfileRequest, AgentProfileSetRequest, profile,
};
mod generation;
mod graph;
mod peers;
pub(super) use peers::directions;
pub(in crate::agent_application) use peers::unseen_results;
pub(super) use peers::working_set as peer_working_set;
mod policy;
mod preview;

struct ProfileWork {
    run: AgentRunId,
    previous: AgentProfile,
    record: aura_agent_collaboration::AgentProfileGeneration,
    key: String,
    preview: bool,
}

impl AgentApplication {
    pub fn browser(&self) -> Arc<agent_execution::browser::BrowserBridge> {
        Arc::clone(&self.execution.browser)
    }
    pub fn media(&self) -> Arc<agent_execution::media::MediaBridge> {
        Arc::clone(&self.execution.media)
    }
    pub(super) async fn profile_route(
        self: &Arc<Self>,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        match method {
            "agent.team.profile.preview" => {
                let request = serde_json::from_value(params)?;
                Ok(serde_json::to_value(self.preview_profile(request).await?)?)
            }
            "agent.team.profile.get" => {
                let request: AgentProfileRequest = serde_json::from_value(params)?;
                Ok(serde_json::to_value(self.agent_profile(request.session_id).await?)?)
            }
            "agent.team.profile.set" => {
                let request: AgentProfileSetRequest = serde_json::from_value(params)?;
                anyhow::ensure!(
                    aura_secret_scanner::detect_text(&request.description).is_none(),
                    "agents.profile_secret: 简介中可能含有凭据，未保存；请移除保密内容后再试。"
                );
                self.ensure_session(request.session_id).await?;
                let value = self
                    .store
                    .write(StoreStep::operation("agents.profile.set"), move |journal| {
                        journal.database.unit_of_work(|tx| {
                            let metadata =
                                aura_persistence_v2::workspace::session(tx, request.session_id)?
                                    .ok_or(aura_persistence_core::PersistenceCoreError::Invalid(
                                        "session",
                                    ))?;
                            profile::set_user(
                                tx,
                                metadata.workspace_id,
                                &metadata.title,
                                &request,
                                UtcTimestamp::now(),
                            )
                        })
                    })
                    .await?;
                Ok(serde_json::to_value(value)?)
            }
            _ => anyhow::bail!("ipc.route_unknown"),
        }
    }

    pub async fn agent_profile(
        self: &Arc<Self>,
        session: SessionId,
    ) -> anyhow::Result<AgentProfile> {
        self.ensure_session(session).await?;
        Ok(self
            .store
            .read(StoreStep::operation("agents.profile.get"), move |db| {
                let metadata = aura_persistence_v2::workspace::session(db, session)?
                    .ok_or(aura_persistence_core::PersistenceCoreError::Invalid("session"))?;
                profile::load(db, session, metadata.workspace_id, &metadata.title)
            })
            .await?)
    }
}
