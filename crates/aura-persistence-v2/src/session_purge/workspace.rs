//! A removed Workspace's own rows, once its Sessions have been purged: its file references,
//! directory grants, grants for the whole Workspace, tool pins, search index, the file changes
//! the user made in Aura's editor and their events.
use aura_domain_common::SessionId;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Connection;

use super::plan::PurgePlan;
use super::scope::{column, json, workspace_lifetime};
use super::tables::{self, IN, OPERATION_ARTIFACTS, OPERATION_ROWS, Spec};

pub(super) fn sessions(db: &Connection, workspace: &str) -> StoreResult<Vec<SessionId>> {
    column(db, "SELECT id FROM sessions WHERE workspace_id=?1", &workspace.to_owned())?
        .into_iter()
        .map(|id| id.parse().map_err(|_| Error::Invalid("session ID")))
        .collect()
}

pub(super) fn plan(db: &Connection, workspace: &str) -> StoreResult<PurgePlan> {
    let w = vec![workspace.to_owned()];
    if !sessions(db, workspace)?.is_empty() {
        return Err(Error::Conflict);
    }
    let operations = column(
        db,
        "SELECT operation_id FROM workspace_mutation_receipts WHERE workspace_id=?1",
        &w[0],
    )?;
    let grants = workspace_grants(db, workspace)?;
    let mut owners = operations.clone();
    owners.extend(grants.iter().cloned());
    owners.extend(column(db, "SELECT id FROM external_grants WHERE workspace_id=?1", &w[0])?);
    let mut events: Vec<String>;
    loop {
        events = if owners.is_empty() {
            Vec::new()
        } else {
            column(db, &format!("SELECT event_id FROM events WHERE owner {IN}"), &json(&owners))?
        };
        let before = owners.len();
        for id in &events {
            if !owners.contains(id) {
                owners.push(id.clone());
            }
        }
        if owners.len() == before {
            break;
        }
    }
    let mut held = Vec::new();
    if !operations.is_empty() {
        for sql in OPERATION_ARTIFACTS {
            held.extend(column(db, &sql.replace("{IN}", IN), &json(&operations))?);
        }
    }
    if !owners.is_empty() {
        held.extend(column(
            db,
            &format!("SELECT artifact_id FROM artifact_owners WHERE owner {IN}"),
            &json(&owners),
        )?);
    }
    held.sort();
    held.dedup();

    let by = |table, key, ids| Spec { table, key, ids, extra: "", shared: false, reversed: false };
    let mut specs: Vec<Spec<'_>> = tables::events(&events).into();
    specs.push(by("artifact_owners", "owner", &owners));
    specs.extend(OPERATION_ROWS.map(|(table, key)| by(table, key, &operations)));
    specs.extend([
        by("authorization_grants", "id", &grants),
        by("file_references", "workspace_id", &w),
        by("external_grants", "workspace_id", &w),
        Spec { extra: "AND owner_kind='workspace'", ..by("tool_pins", "owner_id", &w) },
        by("workspace_search_fts", "workspace_id", &w),
        by("workspaces", "id", &w),
    ]);
    PurgePlan::new(db, &specs, held)
}

/// Grants whose lifetime is this whole Workspace.
fn workspace_grants(db: &Connection, workspace: &str) -> StoreResult<Vec<String>> {
    let mut statement = db.prepare(
        "SELECT id, payload FROM authorization_grants
         WHERE json_extract(payload,'$.scope.lifetime.workspace_id')=?1",
    )?;
    let rows = statement
        .query_map([workspace], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut found = Vec::new();
    for row in rows {
        let (id, payload) = row?;
        if workspace_lifetime(&payload) {
            found.push(id);
        }
    }
    Ok(found)
}
