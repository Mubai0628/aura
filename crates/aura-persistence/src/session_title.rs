//! Session metadata mutations, separate from message and execution ownership.
use crate::{PersistenceCoordinator, PersistenceError, PersistentEventDraft};
use aura_domain_common::{CommandInvocationId, CorrelationId, UtcTimestamp};
use aura_workspace_domain::{SessionRenameRequest, SessionSummary};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Rename {
    command_invocation_id: CommandInvocationId,
    request: SessionRenameRequest,
    session: SessionSummary,
}

impl PersistenceCoordinator {
    pub fn rename_session(
        &self,
        command: CommandInvocationId,
        request: SessionRenameRequest,
        correlation: CorrelationId,
    ) -> Result<SessionSummary, PersistenceError> {
        if request.title.trim().is_empty()
            || request.title.chars().count() > 200
            || request.title.chars().any(char::is_control)
        {
            return Err(PersistenceError::InvalidRequest("session.title_invalid".into()));
        }
        let completed = self.dispatch_class_settled(
            crate::writer_queue::WorkClass::Sync,
            Some(std::time::Instant::now() + self.sync_deadline()),
            Self::session_write_identity(command),
            move |connection| transact(connection, command, request, correlation),
        )?;
        self.finish_session_write(completed)
    }

    pub fn begin_rename_session(
        &self,
        command: CommandInvocationId,
        request: SessionRenameRequest,
        correlation: CorrelationId,
    ) -> Result<crate::SessionWriteReceipt, PersistenceError> {
        if request.title.trim().is_empty()
            || request.title.chars().count() > 200
            || request.title.chars().any(char::is_control)
        {
            return Err(PersistenceError::InvalidRequest("session.title_invalid".into()));
        }
        Ok(crate::SessionWriteReceipt {
            receiver: self
                .dispatch_receipt(Self::session_write_identity(command), move |connection| {
                    transact(connection, command, request, correlation)
                })?,
        })
    }
}

fn transact(
    connection: &mut rusqlite::Connection,
    command: CommandInvocationId,
    request: SessionRenameRequest,
    correlation: CorrelationId,
) -> Result<crate::session_write::SessionWriteCompletion, PersistenceError> {
    let tx = connection.transaction()?;
    let previous: Option<String> = tx
        .query_row(
            "SELECT payload_json FROM events WHERE event_type='session.renamed'
                 AND json_extract(payload_json,'$.command_invocation_id')=?1",
            [command.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(previous) = previous {
        let previous: Rename = serde_json::from_str(&previous)?;
        if previous.request != request {
            return Err(PersistenceError::InvalidRequest("session.idempotency_conflict".into()));
        }
        return Ok(crate::session_write::SessionWriteCompletion {
            session: previous.session,
            permission: None,
            projection: None,
        });
    }
    let mut session = crate::coordinator::load_workspace_session_projection(&tx)?
        .sessions
        .into_iter()
        .find(|s| s.session_id == request.session_id)
        .ok_or_else(|| PersistenceError::InvalidRequest("session.not_found".into()))?;
    if session.title != request.expected_title && session.title != request.title {
        return Err(PersistenceError::InvalidRequest("session.title_conflict".into()));
    }
    session.title.clone_from(&request.title);
    tx.execute(
        "UPDATE sessions SET title=?1,updated_at=?2 WHERE session_id=?3",
        params![session.title, UtcTimestamp::now().to_string(), session.session_id.to_string()],
    )?;
    let mut event = PersistentEventDraft::system(
        "session.renamed",
        correlation,
        serde_json::to_value(Rename {
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
        permission: None,
        projection: Some(projection),
    })
}
