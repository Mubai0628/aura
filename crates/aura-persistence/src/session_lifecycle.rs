//! Archiving, restoring and deleting a Session in the primary registry.
//!
//! Deleting is two steps, so a crash never leaves half a Session behind. `mark_session_deleting`
//! hides the Session at once (the workspace/session projection leaves out a Session whose state
//! is `deleting`) and is the durable record that its data is to go. `purge_session` removes the
//! rows once the execution store's rows are gone; a restart finds the unfinished deletions in
//! `deleting_sessions` and finishes them.
//!
//! Global records may still name a deleted Session's ID: the model configuration journal keeps the
//! projections it fanned out, and the latest row of a global projection is kept for its readers.
//! They hold no conversation and are left as they are.
use std::str::FromStr;

use aura_domain_common::{SessionId, UtcTimestamp, WorkspaceId};
use aura_workspace_domain::SessionSummary;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{PersistenceCoordinator, PersistenceError};

pub const SESSION_IDLE: &str = "idle";
pub const SESSION_ARCHIVED: &str = "archived";
/// While its data is being deleted. The workspace/session projection leaves such a Session out.
pub const SESSION_DELETING: &str = "deleting";

/// A removed Workspace: the primary registry already deleted its rows and its Sessions' rows; the
/// execution store's rows and Aura's files for it are still to go. `notification_id` is marked
/// delivered once they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedWorkspace {
    pub notification_id: String,
    pub workspace_id: WorkspaceId,
    /// Where the Workspace was, to find what Aura kept for its project memory.
    pub roots: Vec<std::path::PathBuf>,
}

/// Inside a Workspace's permanent deletion: one notification for each of its Sessions (their
/// owners end them) and one for the Workspace, with the roots it had.
pub(crate) fn record_removal(
    tx: &Transaction<'_>,
    workspace_id: WorkspaceId,
    sessions: &[String],
    now: &str,
) -> Result<(), PersistenceError> {
    let insert = |kind: &str, subject: &str, payload: serde_json::Value| {
        tx.execute(
            "INSERT INTO cleanup_notifications(
                notification_id, notification_kind, subject_id, payload_json, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![format!("cln_{}", ulid::Ulid::new()), kind, subject, payload.to_string(), now],
        )
    };
    for session_id in sessions {
        insert(
            "session.ended",
            session_id,
            serde_json::json!({"session_id": session_id, "workspace_id": workspace_id, "blocking": false}),
        )?;
    }
    let (selected, canonical): (String, String) = tx.query_row(
        "SELECT selected_root, canonical_root FROM workspaces WHERE workspace_id=?1",
        [workspace_id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    insert(
        WORKSPACE_REMOVED,
        &workspace_id.to_string(),
        serde_json::json!({"workspace_id": workspace_id, "roots": [selected, canonical]}),
    )?;
    Ok(())
}

const WORKSPACE_REMOVED: &str = "workspace.removed";

impl PersistenceCoordinator {
    /// Archives or restores a Session. Asking for the state it is already in changes nothing.
    pub fn set_session_archived(
        &self,
        session_id: SessionId,
        archived: bool,
    ) -> Result<SessionSummary, PersistenceError> {
        let target = if archived { SESSION_ARCHIVED } else { SESSION_IDLE };
        self.change_session_state(session_id, target)
    }

    /// Hides the Session and records that its data is to be deleted. Repeating it is harmless.
    pub fn mark_session_deleting(
        &self,
        session_id: SessionId,
    ) -> Result<SessionSummary, PersistenceError> {
        self.change_session_state(session_id, SESSION_DELETING)
    }

    /// Sessions whose deletion was started and not finished.
    pub fn deleting_sessions(&self) -> Result<Vec<SessionId>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection
                .prepare("SELECT session_id FROM sessions WHERE state=?1 ORDER BY created_at")?;
            let ids = statement
                .query_map([SESSION_DELETING], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.iter().map(|id| Ok(SessionId::from_str(id)?)).collect()
        })
    }

    /// Deletes every primary-registry row of a Session in state `deleting`, with its events.
    /// Returns false when the Session is already gone. The rows found only by reading whole
    /// tables are read first on a reader, and its full-text rows go in short background jobs,
    /// so the one transaction that deletes the rest holds the writer briefly.
    pub fn purge_session(&self, session_id: SessionId) -> Result<bool, PersistenceError> {
        let session = session_id.to_string();
        let planned = self.with_reader(|connection| match state_of(connection, session_id)? {
            None => Ok(None),
            Some(state) if state != SESSION_DELETING => {
                Err(PersistenceError::InvalidRequest("session.not_deleting".into()))
            }
            Some(_) => Ok(Some(crate::registry_purge::of_session(connection, &session)?)),
        })?;
        let Some(mut scanned) = planned else {
            return Ok(false);
        };
        scanned.delete_text(self)?;
        let committed = self.dispatch(move |connection| {
            let tx = connection.transaction()?;
            let Some(state) = state_of(&tx, session_id)? else {
                return Ok(None);
            };
            if state != SESSION_DELETING {
                return Err(PersistenceError::InvalidRequest("session.not_deleting".into()));
            }
            purge_rows(&tx, &session, &scanned)?;
            let projection = crate::workspace::commit_projection(&tx)?;
            tx.commit()?;
            Ok(Some(projection))
        })?;
        let Some(projection) = committed else {
            return Ok(false);
        };
        self.publish_and_mark(projection)?;
        Ok(true)
    }

    /// Removed Workspaces whose execution-store rows and Aura files are still to be deleted.
    pub fn removed_workspaces(&self) -> Result<Vec<RemovedWorkspace>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT notification_id, subject_id, payload_json FROM cleanup_notifications
                 WHERE notification_kind=?1 AND delivered_at IS NULL
                 ORDER BY created_at, notification_id",
            )?;
            let rows = statement
                .query_map([WORKSPACE_REMOVED], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|(notification_id, workspace, payload)| {
                    let payload: serde_json::Value = serde_json::from_str(&payload)?;
                    let roots = payload["roots"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|root| root.as_str())
                        .filter(|root| !root.is_empty())
                        .map(std::path::PathBuf::from)
                        .collect();
                    Ok(RemovedWorkspace {
                        notification_id,
                        workspace_id: WorkspaceId::from_str(&workspace)?,
                        roots,
                    })
                })
                .collect()
        })
    }

    /// Records that a removed Workspace's execution-store rows and Aura files are gone, with its
    /// Sessions' notifications.
    pub fn finish_removed_workspace(
        &self,
        removed: &RemovedWorkspace,
    ) -> Result<(), PersistenceError> {
        let notification = removed.notification_id.clone();
        let workspace = removed.workspace_id.to_string();
        self.dispatch(move |connection| {
            let now = UtcTimestamp::now().to_string();
            connection.execute(
                "UPDATE cleanup_notifications SET delivered_at=?1
                 WHERE delivered_at IS NULL AND (notification_id=?2 OR (
                    notification_kind='session.ended'
                    AND json_extract(payload_json,'$.workspace_id')=?3))",
                params![now, notification, workspace],
            )?;
            Ok(())
        })
    }

    fn change_session_state(
        &self,
        session_id: SessionId,
        target: &'static str,
    ) -> Result<SessionSummary, PersistenceError> {
        let (session, projection) = self.dispatch(move |connection| {
            let tx = connection.transaction()?;
            let state = state_of(&tx, session_id)?.ok_or_else(not_found)?;
            if state == SESSION_DELETING && target != SESSION_DELETING {
                return Err(not_found());
            }
            let mut session = summary(&tx, session_id)?;
            if state == target {
                return Ok((session, None));
            }
            tx.execute(
                "UPDATE sessions SET state=?1, updated_at=?2 WHERE session_id=?3",
                params![target, UtcTimestamp::now().to_string(), session_id.to_string()],
            )?;
            target.clone_into(&mut session.state);
            let projection = crate::workspace::commit_projection(&tx)?;
            tx.commit()?;
            Ok((session, Some(projection)))
        })?;
        if let Some(projection) = projection {
            self.publish_and_mark(projection)?;
        }
        Ok(session)
    }
}

fn not_found() -> PersistenceError {
    PersistenceError::InvalidRequest("session.not_found".into())
}

fn state_of(
    connection: &Connection,
    session_id: SessionId,
) -> Result<Option<String>, PersistenceError> {
    Ok(connection
        .query_row(
            "SELECT state FROM sessions WHERE session_id=?1",
            [session_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?)
}

fn summary(
    connection: &Connection,
    session_id: SessionId,
) -> Result<SessionSummary, PersistenceError> {
    let (workspace, title, state) = connection.query_row(
        "SELECT workspace_id, title, state FROM sessions WHERE session_id=?1",
        [session_id.to_string()],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
    )?;
    Ok(SessionSummary {
        session_id,
        workspace_id: WorkspaceId::from_str(&workspace)?,
        title,
        state,
    })
}

/// One transaction. Foreign keys are checked at commit, so the order below only has to leave a
/// consistent store, and anything that still points at a removed row fails the whole purge.
fn purge_rows(
    tx: &Transaction<'_>,
    session: &str,
    scanned: &crate::registry_purge::Scanned,
) -> Result<(), PersistenceError> {
    tx.execute_batch("PRAGMA defer_foreign_keys=ON")?;
    let now = UtcTimestamp::now().to_string();
    tx.execute(
        "INSERT OR IGNORE INTO artifact_gc_queue(artifact_id, enqueued_at, reason)
         SELECT artifact_id, ?2, 'session_deleted' FROM artifact_owners
         WHERE owner_kind='session' AND owner_id=?1",
        params![session, now],
    )?;
    // Search rows are not reached by the cascades; they went before this transaction.
    // P03-era tool history references its batch without a cascade. The RESTRICT binding is
    // checked at once, so it goes before the attempts it points at.
    let attempts = "SELECT tool_execution_attempt_id FROM tool_execution_attempts
         WHERE tool_batch_id IN (SELECT tool_batch_id FROM tool_batches WHERE session_id=?1)";
    for table in
        ["tool_builtin_core_handler_dispatch_bindings", "tool_execution_recovery_diagnostics"]
    {
        tx.execute(
            &format!("DELETE FROM {table} WHERE tool_execution_attempt_id IN ({attempts})"),
            [session],
        )?;
    }
    for table in
        ["tool_sandbox_dispatch_bindings", "tool_results", "tool_execution_attempts", "tool_calls"]
    {
        tx.execute(
            &format!(
                "DELETE FROM {table} WHERE tool_batch_id IN (
                    SELECT tool_batch_id FROM tool_batches WHERE session_id=?1
                 )"
            ),
            [session],
        )?;
    }
    tx.execute("DELETE FROM tool_batches WHERE session_id=?1", [session])?;
    tx.execute("DELETE FROM events WHERE session_id=?1", [session])?;
    // Projections kept for this Session, and the older published history of every projection
    // that named it (`registry_purge::of_session`). The workspace/session projection is
    // rewritten below with the Session gone.
    scanned.delete_outbox(
        tx,
        session,
        "instr(projection_key, ?1) > 0 OR projection_kind='workspace_session'",
    )?;
    tx.execute("DELETE FROM projection_revisions WHERE instr(projection_key, ?1) > 0", [session])?;
    tx.execute("DELETE FROM sessions WHERE session_id=?1", [session])?;
    Ok(())
}
