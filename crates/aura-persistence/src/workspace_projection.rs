//! The workspace and session projection as the registry holds it, and the links that list a
//! worktree's workspace under the workspace it was made from. The links are named while Core
//! runs: the execution store keeps the worktrees and names them again at start. They are added to
//! what is read and published, never to what the outbox stores, so earlier builds read the outbox
//! as before.
use std::{collections::HashMap, str::FromStr, sync::Mutex};

use aura_domain_common::{FileIdentity, Revision, SessionId, WorkspaceId};
use aura_workspace_domain::{
    CatalogFreshness, SessionSummary, WorkspaceSessionProjection, WorkspaceState, WorkspaceSummary,
    WorkspaceWorktreeLink,
};
use rusqlite::Connection;

use crate::PersistenceError;

/// Worktree workspaces and the workspace each was made from.
#[derive(Default)]
pub(crate) struct WorktreeLinks(Mutex<HashMap<WorkspaceId, WorkspaceId>>);

impl WorktreeLinks {
    pub(crate) fn link(
        &self,
        workspace: WorkspaceId,
        parent: WorkspaceId,
    ) -> Result<(), PersistenceError> {
        if workspace == parent {
            return Err(PersistenceError::InvalidRequest(
                "a worktree is not listed under itself".to_owned(),
            ));
        }
        self.0
            .lock()
            .map_err(|_| PersistenceError::SubscriptionLockPoisoned)?
            .insert(workspace, parent);
        Ok(())
    }

    /// The workspace `workspace` was made from, when it is a worktree's.
    pub(crate) fn parent(&self, workspace: WorkspaceId) -> Option<WorkspaceId> {
        let links = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        links.get(&workspace).copied()
    }

    /// Names the link of every workspace `projection` lists that is a worktree.
    pub(crate) fn decorate(&self, projection: &mut WorkspaceSessionProjection) {
        let links = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        projection.worktrees = projection
            .workspaces
            .iter()
            .filter_map(|workspace| {
                links.get(&workspace.workspace_id).map(|parent| WorkspaceWorktreeLink {
                    workspace_id: workspace.workspace_id,
                    parent_workspace_id: *parent,
                })
            })
            .collect();
    }
}

pub(crate) fn load_workspace_session_projection(
    connection: &Connection,
) -> Result<WorkspaceSessionProjection, PersistenceError> {
    let mut workspace_statement = connection.prepare(
        "SELECT workspace_id, display_name, selected_root, canonical_root,
                root_volume_serial, root_file_id, state, revision,
                catalog_freshness, authorization_epoch
         FROM workspaces ORDER BY created_at, workspace_id",
    )?;
    let workspace_rows = workspace_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, i64>(9)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut workspaces = Vec::with_capacity(workspace_rows.len());
    for (
        workspace_id,
        display_name,
        selected_root,
        canonical_root,
        volume_serial_number,
        file_id,
        state,
        revision,
        catalog_freshness,
        authorization_epoch,
    ) in workspace_rows
    {
        workspaces.push(WorkspaceSummary {
            workspace_id: WorkspaceId::from_str(&workspace_id)?,
            display_name,
            selected_root,
            canonical_root,
            root_identity: match (volume_serial_number, file_id) {
                (Some(volume_serial_number), Some(file_id)) => {
                    Some(FileIdentity { volume_serial_number, file_id })
                }
                (None, None) => None,
                _ => {
                    return Err(PersistenceError::InvalidRequest(
                        "persisted workspace root identity is incomplete".to_owned(),
                    ));
                }
            },
            state: parse_workspace_state(&state)?,
            revision: Revision::new(u64::try_from(revision).map_err(|_| {
                PersistenceError::InvalidRequest("negative workspace revision".to_owned())
            })?),
            catalog_freshness: parse_catalog_freshness(&catalog_freshness)?,
            authorization_epoch: Revision::new(u64::try_from(authorization_epoch).map_err(
                |_| PersistenceError::InvalidRequest("negative authorization epoch".to_owned()),
            )?),
        });
    }

    let mut session_statement = connection.prepare(
        "SELECT session_id, workspace_id, title, state
         FROM sessions WHERE state<>'deleting' ORDER BY created_at, session_id",
    )?;
    let session_rows = session_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut sessions = Vec::with_capacity(session_rows.len());
    for (session_id, workspace_id, title, state) in session_rows {
        sessions.push(SessionSummary {
            session_id: SessionId::from_str(&session_id)?,
            workspace_id: WorkspaceId::from_str(&workspace_id)?,
            title,
            state,
        });
    }
    Ok(WorkspaceSessionProjection { workspaces, sessions, worktrees: Vec::new() })
}

pub(crate) const fn workspace_state_name(state: WorkspaceState) -> &'static str {
    match state {
        WorkspaceState::Online => "online",
        WorkspaceState::Offline => "offline",
        WorkspaceState::Archived => "archived",
        WorkspaceState::ReconciliationRequired => "reconciliation_required",
    }
}

pub(crate) fn parse_workspace_state(value: &str) -> Result<WorkspaceState, PersistenceError> {
    match value {
        "online" => Ok(WorkspaceState::Online),
        "offline" => Ok(WorkspaceState::Offline),
        "archived" => Ok(WorkspaceState::Archived),
        "reconciliation_required" => Ok(WorkspaceState::ReconciliationRequired),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "unknown persisted workspace state: {value}"
        ))),
    }
}

pub(crate) const fn catalog_freshness_name(value: CatalogFreshness) -> &'static str {
    match value {
        CatalogFreshness::Unavailable => "unavailable",
        CatalogFreshness::Rebuilding => "rebuilding",
        CatalogFreshness::Fresh => "fresh",
        CatalogFreshness::Stale => "stale",
    }
}

pub(crate) fn parse_catalog_freshness(value: &str) -> Result<CatalogFreshness, PersistenceError> {
    match value {
        "unavailable" => Ok(CatalogFreshness::Unavailable),
        "rebuilding" => Ok(CatalogFreshness::Rebuilding),
        "fresh" => Ok(CatalogFreshness::Fresh),
        "stale" => Ok(CatalogFreshness::Stale),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "unknown persisted catalog freshness: {value}"
        ))),
    }
}
