//! Session creation owns its SQL and publishes the existing projection. It does
//! not update workspace facts or create a second permission-mode authority.
use aura_domain_common::{CommandInvocationId, CorrelationId, SessionId, UtcTimestamp};
use aura_security_contract::PermissionMode;
use aura_workspace_domain::{SessionCreateRequest, SessionSummary};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{PersistenceCoordinator, PersistenceError, PersistentEventDraft};

#[derive(Deserialize, Serialize)]
struct Creation {
    command_invocation_id: CommandInvocationId,
    request: SessionCreateRequest,
    session: SessionSummary,
}

impl PersistenceCoordinator {
    pub fn create_session(
        &self,
        command: CommandInvocationId,
        request: SessionCreateRequest,
        default_mode: PermissionMode,
        correlation: CorrelationId,
    ) -> Result<SessionSummary, PersistenceError> {
        if request.title.chars().count() > 200 || request.title.chars().any(char::is_control) {
            return Err(PersistenceError::InvalidRequest("session.title_invalid".into()));
        }
        let completed = self.dispatch_class_settled(
            crate::writer_queue::WorkClass::Sync,
            Some(std::time::Instant::now() + self.sync_deadline()),
            Self::session_write_identity(command),
            move |connection| transact(connection, command, request, default_mode, correlation),
        )?;
        self.finish_session_write(completed)
    }

    pub fn begin_create_session(
        &self,
        command: CommandInvocationId,
        request: SessionCreateRequest,
        default_mode: PermissionMode,
        correlation: CorrelationId,
    ) -> Result<crate::SessionWriteReceipt, PersistenceError> {
        if request.title.chars().count() > 200 || request.title.chars().any(char::is_control) {
            return Err(PersistenceError::InvalidRequest("session.title_invalid".into()));
        }
        Ok(crate::SessionWriteReceipt {
            receiver: self
                .dispatch_receipt(Self::session_write_identity(command), move |connection| {
                    transact(connection, command, request, default_mode, correlation)
                })?,
        })
    }
}

fn transact(
    connection: &mut rusqlite::Connection,
    command: CommandInvocationId,
    request: SessionCreateRequest,
    default_mode: PermissionMode,
    correlation: CorrelationId,
) -> Result<crate::session_write::SessionWriteCompletion, PersistenceError> {
    let tx = connection.transaction()?;
    let previous: Option<String> = tx
        .query_row(
            "SELECT payload_json FROM events WHERE event_type='session.created'
                 AND json_extract(payload_json,'$.command_invocation_id')=?1",
            [command.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(previous) = previous {
        let previous: Creation = serde_json::from_str(&previous)?;
        if previous.request != request {
            return Err(PersistenceError::InvalidRequest("session.idempotency_conflict".into()));
        }
        return Ok(crate::session_write::SessionWriteCompletion {
            session: previous.session,
            permission: None,
            projection: None,
        });
    }
    let workspace_state: Option<String> = tx
        .query_row(
            "SELECT state FROM workspaces WHERE workspace_id=?1",
            [request.workspace_id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if workspace_state.as_deref() != Some("online") {
        return Err(PersistenceError::InvalidRequest("session.workspace_unavailable".into()));
    }
    let session = SessionSummary {
        session_id: SessionId::new(),
        workspace_id: request.workspace_id,
        title: request.title.clone(),
        state: "idle".into(),
    };
    tx.execute(
        "INSERT INTO sessions(session_id,workspace_id,title,state,created_at,updated_at)
                 VALUES(?1,?2,?3,'idle',?4,?4)",
        params![
            session.session_id.to_string(),
            session.workspace_id.to_string(),
            session.title,
            UtcTimestamp::now().to_string()
        ],
    )?;
    let permission = crate::security::session_authority::ensure(
        &tx,
        session.session_id,
        session.workspace_id,
        default_mode,
        correlation,
    )?;
    let mut event = PersistentEventDraft::system(
        "session.created",
        correlation,
        serde_json::to_value(Creation {
            command_invocation_id: command,
            request,
            session: session.clone(),
        })?,
    );
    event.workspace_id = Some(session.workspace_id);
    event.session_id = Some(session.session_id);
    crate::coordinator::insert_event(&tx, &event)?;
    let projection = crate::workspace::commit_projection(&tx)?;
    tx.commit()?;
    Ok(crate::session_write::SessionWriteCompletion {
        session,
        permission: Some(permission),
        projection: Some(projection),
    })
}
