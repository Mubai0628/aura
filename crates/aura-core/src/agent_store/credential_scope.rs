//! The V2 store facts the V1 credential journal reads before its own transaction.
use super::AgentStore;

impl aura_persistence::ToolCredentialScopeSource for AgentStore {
    fn session_belongs_to(
        &self,
        workspace: aura_domain_common::WorkspaceId,
        session: aura_domain_common::SessionId,
    ) -> Result<bool, aura_persistence::PersistenceError> {
        use aura_persistence_core::OptionalExtension;
        let journal = self
            .journal
            .lock()
            .map_err(|_| aura_persistence::PersistenceError::ToolEntityIntegrity)?;
        journal
            .database
            .connection()
            .query_row(
                "SELECT workspace_id FROM sessions WHERE id=?1",
                [session.to_string()],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map(|value| value.as_deref() == Some(workspace.to_string().as_str()))
            .map_err(|_| aura_persistence::PersistenceError::ToolEntityIntegrity)
    }
    fn package_tool_target(
        &self,
        instance: aura_domain_common::ToolInstanceId,
    ) -> Result<
        Option<aura_persistence::PackageToolCredentialFacts>,
        aura_persistence::PersistenceError,
    > {
        let journal = self
            .journal
            .lock()
            .map_err(|_| aura_persistence::PersistenceError::ToolEntityIntegrity)?;
        crate::package_services::credential_facts(journal.database.connection(), instance)
            .map_err(|_| aura_persistence::PersistenceError::ToolEntityIntegrity)
    }
}
