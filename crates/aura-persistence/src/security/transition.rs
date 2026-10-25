//! Transition acknowledgement is a versioned fact in the existing event store,
//! not another mode authority/table. The current mode/revision row is unique.
use super::*;
use aura_permission_domain::PermissionModeTransition;

pub(super) fn load(
    connection: &Connection,
    session: SessionId,
    revision: u64,
) -> Result<PermissionModeTransition, PersistenceError> {
    let value:Option<String>=connection.query_row(
        "SELECT payload_json FROM events WHERE session_id=?1 AND event_type='SessionPermissionTransition' AND json_extract(payload_json,'$.revision')=?2 ORDER BY global_sequence DESC LIMIT 1",
        params![session.to_string(),to_i64(revision,"permission revision")?],|row|row.get(0)).optional()?;
    value
        .map(|v| -> Result<_, PersistenceError> {
            let value: serde_json::Value = serde_json::from_str(&v)?;
            Ok(serde_json::from_value(value["phase"].clone())?)
        })
        .transpose()
        .map(|v| v.unwrap_or_default())
}
pub(super) fn record(
    tx: &Transaction<'_>,
    session: SessionId,
    workspace: WorkspaceId,
    revision: u64,
    phase: PermissionModeTransition,
    correlation: CorrelationId,
) -> Result<(), PersistenceError> {
    let mut event = PersistentEventDraft::system(
        "SessionPermissionTransition",
        correlation,
        json!({"schema":"permission-transition-v1","revision":revision,"phase":phase}),
    );
    event.workspace_id = Some(workspace);
    event.session_id = Some(session);
    insert_event(tx, &event)?;
    Ok(())
}
impl PersistenceCoordinator {
    /// Called only after Core has awaited affected process trees and HTTP
    /// futures. A newer setter cannot be settled by an older completion.
    pub fn finish_session_permission_transition(
        &self,
        session: SessionId,
        revision: u64,
        stopped: bool,
        correlation: CorrelationId,
    ) -> Result<SessionPermissionProjection, PersistenceError> {
        let _barrier = self.lock_permission_update(session)?;
        let mutation = self.dispatch(move |connection| {
            let tx = connection.transaction()?;
            let current = load_session_permission_projection(&tx, session)?;
            if current.revision != revision {
                return Err(PersistenceError::PermissionRevisionConflict);
            }
            let phase = if stopped {
                PermissionModeTransition::Settled
            } else {
                PermissionModeTransition::StopUnconfirmed
            };
            let workspace = session_workspace_id(&tx, session)?;
            record(&tx, session, workspace, revision, phase, correlation)?;
            let payload = load_session_permission_projection(&tx, session)?;
            let projection = commit_session_permission_projection(&tx, payload.clone())?;
            tx.commit()?;
            Ok(SecurityMutation { value: payload, projections: vec![projection] })
        })?;
        self.finish_security_mutation(mutation)
    }
    pub fn pending_session_permission_transitions(
        &self,
    ) -> Result<Vec<SessionPermissionProjection>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement =
                connection.prepare("SELECT session_id FROM session_permission_states")?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut pending = Vec::new();
            for id in ids {
                let state =
                    load_session_permission_projection(connection, SessionId::from_str(&id)?)?;
                if state.transition != PermissionModeTransition::Settled {
                    pending.push(state);
                }
            }
            Ok(pending)
        })
    }
}
