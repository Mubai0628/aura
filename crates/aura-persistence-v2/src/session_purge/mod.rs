//! Deleting a Session's records from the execution store.
//!
//! The owner decided that deleting a Session deletes everything Aura holds for it:
//! its conversation, its process and its audit events. Everything is found from the Session
//! outward (`scope`), deleted in one transaction (`rows`), and the store's two delete guards are
//! lifted only inside that transaction (`guards`), where the lookup indexes the deletes need also
//! exist (`lookups`). Purging a Session that is already gone deletes nothing, so an interrupted
//! deletion can simply run again.
mod guards;
mod lookups;
mod rows;
mod scope;
mod workspace;

use aura_domain_common::{SessionId, WorkspaceId};
use aura_persistence_core::StoreResult;
use rusqlite::Transaction;

/// What one purge removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PurgeReport {
    pub rows: usize,
    pub artifacts: usize,
}

/// Deletes every execution-store row of the Session, with its events and the artifacts nothing
/// else owns.
pub fn purge_session(tx: &Transaction<'_>, session: SessionId) -> StoreResult<PurgeReport> {
    let scope = scope::of_session(tx, &session.to_string())?;
    let lifted = guards::lift(tx)?;
    let lookups = lookups::build(tx)?;
    let report = rows::delete(tx, &scope)?;
    lookups::remove(tx, lookups)?;
    guards::restore(tx, lifted)?;
    Ok(report)
}

/// Deletes a removed Workspace's own rows. Its Sessions are purged first, one by one.
pub fn purge_workspace(tx: &Transaction<'_>, workspace: WorkspaceId) -> StoreResult<PurgeReport> {
    let lifted = guards::lift(tx)?;
    let lookups = lookups::build(tx)?;
    let report = workspace::delete(tx, &workspace.to_string())?;
    lookups::remove(tx, lookups)?;
    guards::restore(tx, lifted)?;
    Ok(report)
}

/// The Sessions the execution store still has for a Workspace.
pub fn workspace_sessions(
    db: &rusqlite::Connection,
    workspace: WorkspaceId,
) -> StoreResult<Vec<SessionId>> {
    workspace::sessions(db, &workspace.to_string())
}
