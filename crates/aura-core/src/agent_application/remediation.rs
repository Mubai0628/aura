//! Context remediation entries on the bounded Store owner.
use super::*;
use crate::agent_store::StoreStep;
use aura_context_domain::{AgentContextRecheckRequest, AgentContextRemediationView};
use aura_persistence_v2::context::remediation;

impl AgentApplication {
    pub async fn context_remediation(
        &self,
        input: AgentRunRequest,
    ) -> anyhow::Result<AgentContextRemediationView> {
        Ok(self
            .store
            .read(StoreStep::run(input.agent_run_id, "agent.context.remediation"), move |db| {
                remediation::view(db, &input)
            })
            .await?)
    }

    pub async fn recheck_context(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: AgentContextRecheckRequest,
    ) -> anyhow::Result<AgentContextRemediationView> {
        let deadline = Self::stop_deadline();
        let scope =
            AgentRunRequest { session_id: input.session_id, agent_run_id: input.agent_run_id };
        self.context_remediation(scope.clone()).await?;
        let stored = input.clone();
        let rechecked = self
            .store
            .read(StoreStep::run(input.agent_run_id, "agent.context.rechecked"), move |db| {
                remediation::rechecked(db, command, &stored)
            })
            .await?;
        if rechecked {
            return self.context_remediation(scope).await;
        }
        self.idle_within(input.agent_run_id, deadline).await?;
        let stored = input.clone();
        self.store
            .write(StoreStep::run(input.agent_run_id, "agent.context.recheck"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    remediation::recheck(tx, command, &stored, UtcTimestamp::now())
                })
            })
            .await?;
        self.launch(input.agent_run_id)?;
        self.context_remediation(scope).await
    }
}
