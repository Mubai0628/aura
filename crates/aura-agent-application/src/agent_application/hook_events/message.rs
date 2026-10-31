//! `message.before_accept`.
use super::*;

impl AgentApplication {
    /// `message.before_accept`, before a message is stored: a block refuses it with the Hook's
    /// name and reason; a rewrite is returned to be kept with the message, in its transaction.
    pub(in crate::agent_application) async fn accept_message(
        &self,
        session: SessionId,
        workspace: Option<WorkspaceId>,
        text: &str,
        lane: &str,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        if !self.hooks_registered() || !self.hook_runner()?.hears(HookEvent::MessageBeforeAccept) {
            return Ok(None);
        }
        let call = HookCall {
            event: HookEvent::MessageBeforeAccept,
            session_id: Some(session),
            turn_task_id: None,
            tool_call_id: None,
            tool_name: None,
            workspace_root: workspace.and_then(|w| self.hook_workspace_root(w)),
            subject: None,
            payload: serde_json::json!({"text": text, "lane": lane}),
        };
        let outcome = self.run_hooks(call).await?;
        if let Some(block) = outcome.block {
            anyhow::bail!(
                "hooks.message_blocked: Hook {} 没有接收这条消息：{}",
                block.hook_name,
                block.reason
            );
        }
        Ok(outcome.transformed.then(|| {
            let hooks = outcome
                .invocations
                .iter()
                .filter(|view| view.outcome == aura_hooks::HookOutcome::Transformed)
                .map(|view| view.hook_name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            serde_json::json!({"text": outcome.payload["text"], "hooks": hooks})
        }))
    }

    /// The workspace a session belongs to, for the Hooks' working folder.
    pub fn session_workspace(&self, session: SessionId) -> Option<WorkspaceId> {
        let projection = self.execution.persistence.workspace_session_projection().ok()?;
        projection.sessions.iter().find(|s| s.session_id == session).map(|s| s.workspace_id)
    }
}
