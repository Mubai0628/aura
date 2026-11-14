//! 本工作区允许 on a sub-agent's deletion: the question stores one answer, and the card offers
//! the allowance beside it when the command asks only because it deletes in the sub-agent's own
//! folders. Answering with it runs this command once and records the allowance on the sub-agent.
use super::*;
use aura_execution_contract::ExecutionApprovalView;
use aura_security_contract::{ApprovalDecisionKind, ApprovalState};

impl AgentApplication {
    /// Adds 本工作区允许 after the once-answer of a pending question that may be answered with it.
    pub(in crate::agent_application) async fn offer_deletion_allowance(
        &self,
        view: &mut ExecutionApprovalView,
    ) -> anyhow::Result<()> {
        if self.offers_deletion_allowance(view).await? {
            let decisions = &mut view.approval.allowed_decisions;
            let after_once = decisions.iter().position(|d| *d == ApprovalDecisionKind::AllowOnce);
            decisions
                .insert(after_once.map_or(0, |at| at + 1), ApprovalDecisionKind::AllowWorkspace);
        }
        Ok(())
    }

    /// Whether the pending question in `view` offers 本工作区允许 beside what it stores.
    pub(in crate::agent_application) async fn offers_deletion_allowance(
        &self,
        view: &ExecutionApprovalView,
    ) -> anyhow::Result<bool> {
        let decisions = &view.approval.allowed_decisions;
        if view.approval.state != ApprovalState::Pending
            || decisions.contains(&ApprovalDecisionKind::AllowWorkspace)
            || !decisions.contains(&ApprovalDecisionKind::AllowOnce)
        {
            return Ok(false);
        }
        let execution = Arc::clone(&self.execution);
        let operation = view.operation.clone();
        tokio::task::spawn_blocking(move || execution.offers_deletion_allowance(&operation)).await?
    }
}
