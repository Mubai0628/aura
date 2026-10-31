use super::*;
impl P04Services {
    pub fn ensure_permission_state(
        &self,
        session_id: SessionId,
        correlation_id: CorrelationId,
    ) -> Result<aura_permission_domain::SessionPermissionProjection, PersistenceError> {
        match self.persistence.session_permission_state(session_id) {
            Ok(state) => Ok(state),
            Err(PersistenceError::PermissionStateNotFound) => {
                let workspace_id = self
                    .persistence
                    .workspace_session_projection()?
                    .sessions
                    .into_iter()
                    .find(|session| session.session_id == session_id)
                    .map(|session| session.workspace_id)
                    .ok_or(PersistenceError::PermissionStateNotFound)?;
                self.persistence.ensure_session_permission_state(
                    session_id,
                    workspace_id,
                    self.default_permission_mode,
                    correlation_id,
                )
            }
            Err(error) => Err(error),
        }
    }
}
