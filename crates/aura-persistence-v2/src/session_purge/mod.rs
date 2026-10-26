//! Deleting a Session's records from the execution store.
//!
//! The owner decided that deleting a Session deletes everything Aura holds for it:
//! its conversation, its process and its audit events. Everything is found from the Session
//! outward (`scope`) on a reader, before anything is deleted (`plan`); the writer then deletes the
//! planned rows in short transactions (`steps`), children before the rows they point at
//! (`tables`), with each statement checking the foreign keys that point at its table (`edges`).
//! The store's two delete guards are lifted only inside a step that needs it (`guards`). Rows
//! something outside the plan still points at are kept, and so are rows written after the plan
//! was read; planning again finds what is left. Purging a Session that is already gone deletes
//! nothing, so an interrupted deletion can simply run again.
mod collect;
mod edges;
mod guards;
mod plan;
mod scope;
mod steps;
mod tables;
mod workspace;

use std::time::Duration;

use aura_domain_common::{SessionId, WorkspaceId};
use aura_persistence_core::{DatabaseConnection, StoreResult};
use rusqlite::Connection;

pub use plan::PurgePlan;
pub use steps::step;

/// What one purge removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PurgeReport {
    pub rows: usize,
    pub artifacts: usize,
}

/// Every execution-store row of the Session, with its events and the artifacts only it held, read
/// on `reader` in one read transaction.
pub fn plan_session(reader: &Connection, session: SessionId) -> StoreResult<PurgePlan> {
    let snapshot = reader.unchecked_transaction()?;
    let scope = scope::of_session(&snapshot, &session.to_string())?;
    let held = tables::held(&snapshot, &scope)?;
    PurgePlan::new(&snapshot, &tables::session(&scope), held)
}

/// A removed Workspace's own rows, once its Sessions have been purged.
pub fn plan_workspace(reader: &Connection, workspace: WorkspaceId) -> StoreResult<PurgePlan> {
    let snapshot = reader.unchecked_transaction()?;
    workspace::plan(&snapshot, &workspace.to_string())
}

/// Plans and deletes on one connection, as many times as a plan leaves rows behind: for tests
/// and tools that own the store alone.
pub fn purge_now(
    db: &mut DatabaseConnection,
    plan: impl Fn(&Connection) -> StoreResult<PurgePlan>,
) -> StoreResult<PurgeReport> {
    let mut report = PurgeReport::default();
    for _ in 0..4 {
        let mut planned = plan(db.connection())?;
        while !planned.done() {
            step(db, &mut planned, Duration::from_secs(60))?;
        }
        report.rows += planned.report().rows;
        report.artifacts += planned.report().artifacts;
        if planned.skipped() == 0 {
            return Ok(report);
        }
    }
    Err(aura_persistence_core::PersistenceCoreError::Conflict)
}

/// The Sessions the execution store still has for a Workspace.
pub fn workspace_sessions(
    db: &rusqlite::Connection,
    workspace: WorkspaceId,
) -> StoreResult<Vec<SessionId>> {
    workspace::sessions(db, &workspace.to_string())
}
