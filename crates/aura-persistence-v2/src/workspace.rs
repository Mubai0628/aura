use aura_domain_common::{SessionId, UtcTimestamp, WorkspaceId};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_workspace_domain::WorkspaceSummary;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub fn insert(tx: &Transaction<'_>, workspace: &WorkspaceSummary) -> StoreResult<()> {
    if workspace.display_name.trim().is_empty() || workspace.revision.value() == 0 {
        return Err(Error::Invalid("workspace metadata"));
    }
    tx.execute(
        "INSERT INTO workspaces VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            workspace.workspace_id.to_string(),
            workspace.display_name,
            workspace.canonical_root,
            serde_json::to_value(workspace.state)?.as_str(),
            record::sql_u64(workspace.revision.value())?,
            record::encode(workspace)?
        ],
    )?;
    Ok(())
}

pub fn get(connection: &Connection, id: WorkspaceId) -> StoreResult<Option<WorkspaceSummary>> {
    let value: Option<String> = connection
        .query_row("SELECT payload FROM workspaces WHERE id=?1", [id.to_string()], |r| r.get(0))
        .optional()?;
    value.map(|v| record::decode(&v)).transpose()
}

pub fn replace(
    tx: &Transaction<'_>,
    previous_revision: u64,
    workspace: &WorkspaceSummary,
) -> StoreResult<()> {
    if workspace.revision.value() != previous_revision + 1 {
        return Err(Error::Conflict);
    }
    record::changed(tx.execute("UPDATE workspaces SET display_name=?2,canonical_root=?3,state=?4,revision=?5,payload=?6 WHERE id=?1 AND revision=?7",params![workspace.workspace_id.to_string(),workspace.display_name,workspace.canonical_root,serde_json::to_value(workspace.state)?.as_str(),record::sql_u64(workspace.revision.value())?,record::encode(workspace)?,record::sql_u64(previous_revision)?])?)
}

pub struct SessionMetadata {
    pub id: SessionId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}
/// Mirrors one primary-registry Session into the V2 execution store.
///
/// The primary registry owns naming. A Session created for a fresh draft is
/// persisted there with an empty title and is named only after its first
/// accepted message, so "unnamed" is a valid mirrored state (the Renderer
/// presents it as 新会话). Requiring a non-empty title here made every first
/// send impossible: the mirror insert failed, no V2 Session row existed, and
/// every `agent.submit` for that Session was refused before it could name
/// itself. Timestamp ordering is still enforced.
pub fn insert_session(tx: &Transaction<'_>, session: &SessionMetadata) -> StoreResult<()> {
    if session.updated_at < session.created_at {
        return Err(Error::Invalid("session metadata"));
    }
    // Imported or newly constructed metadata never resurrects active execution.
    tx.execute(
        "INSERT INTO sessions VALUES(?1,?2,?3,'idle',?4,?5)",
        params![
            session.id.to_string(),
            session.workspace_id.to_string(),
            session.title,
            session.created_at.to_string(),
            session.updated_at.to_string()
        ],
    )?;
    Ok(())
}

/// Mirrors a primary-registry rename so the mirrored title cannot drift from
/// the authority after the first accepted message names the Session.
pub fn rename_session(
    tx: &Transaction<'_>,
    id: SessionId,
    title: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    record::changed(tx.execute(
        "UPDATE sessions SET title=?2,updated_at=?3 WHERE id=?1",
        params![id.to_string(), title, at.to_string()],
    )?)
}

pub fn session(connection: &Connection, id: SessionId) -> StoreResult<Option<SessionMetadata>> {
    let row: Option<(String, String, String, String)> = connection
        .query_row(
            "SELECT workspace_id,title,created_at,updated_at FROM sessions WHERE id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    row.map(|(workspace, title, created, updated)| {
        Ok(SessionMetadata {
            id,
            workspace_id: workspace.parse().map_err(|_| Error::Invalid("workspace ID"))?,
            title,
            created_at: created.parse().map_err(|_| Error::Invalid("session time"))?,
            updated_at: updated.parse().map_err(|_| Error::Invalid("session time"))?,
        })
    })
    .transpose()
}
