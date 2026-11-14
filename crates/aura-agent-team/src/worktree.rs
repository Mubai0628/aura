//! The worktrees in the execution store: one row for each, kept after it is merged or discarded so
//! the conversation that worked in it still names its branch, and the folder chosen for new ones.
use crate::WorktreeState;
use aura_domain_common::{SessionId, UtcTimestamp, WorkspaceId};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

const COLUMNS: &str = "workspace_id, session_id, parent_session_id, parent_workspace_id, state, \
     revision, payload, created_at, updated_at";

/// A worktree a conversation works in: a sub-agent's, or one the user started a conversation in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    /// The hidden workspace its folder is registered as.
    pub workspace_id: WorkspaceId,
    /// The conversation that works in it.
    pub session_id: SessionId,
    /// The conversation that started the sub-agent; none for a conversation of the user's.
    pub parent_session_id: Option<SessionId>,
    /// The workspace it was made from and merges into.
    pub parent_workspace_id: WorkspaceId,
    pub state: WorktreeState,
    pub revision: u64,
    pub details: WorktreeDetails,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorktreeDetails {
    pub path: String,
    pub branch: String,
    /// The commit it started from.
    pub base: String,
}

pub fn insert_worktree(tx: &Transaction<'_>, worktree: &Worktree) -> StoreResult<()> {
    if worktree.revision != 1 || worktree.state != WorktreeState::Active {
        return Err(Error::Invalid("worktree start"));
    }
    tx.execute(
        &format!("INSERT INTO agent_worktrees({COLUMNS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)"),
        params![
            worktree.workspace_id.to_string(),
            worktree.session_id.to_string(),
            worktree.parent_session_id.map(|id| id.to_string()),
            worktree.parent_workspace_id.to_string(),
            worktree.state.as_str(),
            1_i64,
            serde_json::to_string(&worktree.details)?,
            worktree.created_at.to_string(),
            worktree.updated_at.to_string(),
        ],
    )?;
    Ok(())
}

/// Moves `worktree` to `state`, one revision on.
pub fn set_state(
    tx: &Transaction<'_>,
    worktree: &Worktree,
    state: WorktreeState,
) -> StoreResult<Worktree> {
    let next = Worktree {
        state,
        revision: worktree.revision + 1,
        updated_at: UtcTimestamp::now(),
        ..worktree.clone()
    };
    let changed = tx.execute(
        "UPDATE agent_worktrees SET state=?2, revision=?3, updated_at=?4 \
         WHERE workspace_id=?1 AND revision=?5",
        params![
            next.workspace_id.to_string(),
            next.state.as_str(),
            i64::try_from(next.revision).map_err(|_| Error::Invalid("worktree revision"))?,
            next.updated_at.to_string(),
            i64::try_from(worktree.revision).map_err(|_| Error::Invalid("worktree revision"))?,
        ],
    )?;
    if changed == 1 { Ok(next) } else { Err(Error::Conflict) }
}

/// The worktree conversation `session` works in.
pub fn worktree_of(db: &Connection, session: SessionId) -> StoreResult<Option<Worktree>> {
    db.query_row(
        &format!("SELECT {COLUMNS} FROM agent_worktrees WHERE session_id=?1"),
        [session.to_string()],
        worktree_row,
    )
    .optional()?
    .transpose()
}

/// The worktree registered as `workspace`.
pub fn worktree_by_workspace(
    db: &Connection,
    workspace: WorkspaceId,
) -> StoreResult<Option<Worktree>> {
    db.query_row(
        &format!("SELECT {COLUMNS} FROM agent_worktrees WHERE workspace_id=?1"),
        [workspace.to_string()],
        worktree_row,
    )
    .optional()?
    .transpose()
}

/// The worktrees of the sub-agents conversation `parent` started, oldest first.
pub fn worktrees_of_parent(db: &Connection, parent: SessionId) -> StoreResult<Vec<Worktree>> {
    let mut statement = db.prepare(&format!(
        "SELECT {COLUMNS} FROM agent_worktrees WHERE parent_session_id=?1 ORDER BY created_at"
    ))?;
    let rows = statement.query_map([parent.to_string()], worktree_row)?;
    rows.map(|row| row?).collect()
}

/// Every worktree still in use, for the links between workspaces made at start.
pub fn active_worktrees(db: &Connection) -> StoreResult<Vec<Worktree>> {
    let mut statement = db.prepare(&format!(
        "SELECT {COLUMNS} FROM agent_worktrees WHERE state='active' ORDER BY created_at"
    ))?;
    let rows = statement.query_map([], worktree_row)?;
    rows.map(|row| row?).collect()
}

/// Every worktree, merged and discarded ones too: their workspaces stay listed under the workspace
/// they came from.
pub fn all_worktrees(db: &Connection) -> StoreResult<Vec<Worktree>> {
    let mut statement =
        db.prepare(&format!("SELECT {COLUMNS} FROM agent_worktrees ORDER BY created_at"))?;
    let rows = statement.query_map([], worktree_row)?;
    rows.map(|row| row?).collect()
}

/// The folder the user chose for new worktrees and the settings revision; none and 0 when the
/// default is used and nothing was ever chosen.
pub fn location(db: &Connection) -> StoreResult<(Option<String>, u64)> {
    let row: Option<(i64, String)> = db
        .query_row("SELECT revision, payload FROM agent_worktree_settings WHERE id=1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()?;
    let Some((revision, payload)) = row else { return Ok((None, 0)) };
    let settings: LocationSettings = serde_json::from_str(&payload)?;
    Ok((settings.location, u64::try_from(revision).map_err(|_| Error::Invalid("revision"))?))
}

/// Chooses the folder for new worktrees, or the default with `None`, at the revision the caller
/// read. Returns the new revision.
pub fn set_location(tx: &Transaction<'_>, chosen: Option<&str>, expected: u64) -> StoreResult<u64> {
    let (_, current) = location(tx)?;
    if current != expected {
        return Err(Error::Conflict);
    }
    let next = current + 1;
    let payload = serde_json::to_string(&LocationSettings { location: chosen.map(str::to_owned) })?;
    tx.execute(
        "INSERT INTO agent_worktree_settings(id, revision, payload, updated_at) VALUES(1,?1,?2,?3) \
         ON CONFLICT(id) DO UPDATE SET revision=excluded.revision, payload=excluded.payload, \
         updated_at=excluded.updated_at",
        params![
            i64::try_from(next).map_err(|_| Error::Invalid("revision"))?,
            payload,
            UtcTimestamp::now().to_string()
        ],
    )?;
    Ok(next)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LocationSettings {
    location: Option<String>,
}

fn worktree_row(row: &Row<'_>) -> rusqlite::Result<StoreResult<Worktree>> {
    let fields: (String, String, Option<String>, String, String, i64, String, String, String) = (
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    );
    Ok((|| {
        let (workspace, session, parent, parent_workspace, state, revision, payload, at, updated) =
            fields;
        let id = |text: &str| text.parse().map_err(|_| Error::Invalid("worktree identity"));
        Ok(Worktree {
            workspace_id: workspace.parse().map_err(|_| Error::Invalid("worktree workspace"))?,
            session_id: id(&session)?,
            parent_session_id: parent.as_deref().map(id).transpose()?,
            parent_workspace_id: parent_workspace
                .parse()
                .map_err(|_| Error::Invalid("worktree parent workspace"))?,
            state: WorktreeState::parse(&state).ok_or(Error::Invalid("worktree state"))?,
            revision: u64::try_from(revision).map_err(|_| Error::Invalid("worktree revision"))?,
            details: serde_json::from_str(&payload)?,
            created_at: at.parse().map_err(|_| Error::Invalid("worktree time"))?,
            updated_at: updated.parse().map_err(|_| Error::Invalid("worktree time"))?,
        })
    })())
}
