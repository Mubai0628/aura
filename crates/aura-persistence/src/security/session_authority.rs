//! Single initializer used by legacy consumers and production Session creation.
use super::*;
pub(crate) fn ensure(
    transaction: &Transaction<'_>,
    session_id: SessionId,
    workspace_id: WorkspaceId,
    default_mode: PermissionMode,
    correlation_id: CorrelationId,
) -> Result<SecurityMutation<SessionPermissionProjection>, PersistenceError> {
    let now = UtcTimestamp::now();
    transaction.execute(
        "INSERT INTO session_permission_states(
            session_id, workspace_id, mode, revision, updated_at
         ) VALUES (?1, ?2, ?3, 1, ?4)
         ON CONFLICT(session_id) DO NOTHING",
        params![
            session_id.to_string(),
            workspace_id.to_string(),
            permission_mode_name(default_mode),
            now.to_string()
        ],
    )?;
    if session_workspace_id(transaction, session_id)? != workspace_id {
        return Err(PersistenceError::InvalidRequest(
            "session permission workspace does not match session".to_owned(),
        ));
    }
    let payload = load_session_permission_projection(transaction, session_id)?;
    let mut event = PersistentEventDraft::system(
        "SessionPermissionStateEnsured",
        correlation_id,
        json!({
            "session_id": session_id,
            "workspace_id": workspace_id,
            "mode": default_mode,
        }),
    );
    event.workspace_id = Some(workspace_id);
    event.session_id = Some(session_id);
    insert_event(transaction, &event)?;
    let projection = commit_session_permission_projection(transaction, payload.clone())?;
    Ok(SecurityMutation { value: payload, projections: vec![projection] })
}
