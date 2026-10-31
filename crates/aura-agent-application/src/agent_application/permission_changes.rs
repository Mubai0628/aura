use super::*;
use aura_security_contract::PermissionMode;

impl AgentApplication {
    /// Changes the defaults, then stops only managed calls whose lost authority has no approval.
    /// Model generation and Query waiting have no file-mode cancellation dependency.
    pub async fn mode_changed(
        self: &Arc<Self>,
        session: SessionId,
        before: PermissionMode,
    ) -> anyhow::Result<()> {
        self.execution
            .stop_authority_dependencies(
                session,
                Some(before),
                Some(self.commands.clone()),
                Self::stop_deadline(),
            )
            .await
    }
    pub async fn file_authority_withdrawn(
        self: &Arc<Self>,
        session: SessionId,
    ) -> anyhow::Result<()> {
        self.execution
            .stop_authority_dependencies(
                session,
                None,
                Some(self.commands.clone()),
                Self::stop_deadline(),
            )
            .await
    }
}
