//! A removed Workspace's own rows, once its Sessions have been purged: its file references,
//! directory grants, grants for the whole Workspace, tool pins, search index, the file changes
//! the user made in Aura's editor and their events.
use aura_domain_common::SessionId;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, Transaction};

use super::PurgeReport;
use super::rows::{IN, OPERATION_ARTIFACTS, OPERATION_ROWS, release};
use super::scope::{column, json, workspace_lifetime};

pub(super) fn sessions(db: &Connection, workspace: &str) -> StoreResult<Vec<SessionId>> {
    column(db, "SELECT id FROM sessions WHERE workspace_id=?1", &workspace.to_owned())?
        .into_iter()
        .map(|id| id.parse().map_err(|_| Error::Invalid("session ID")))
        .collect()
}

pub(super) fn delete(tx: &Transaction<'_>, workspace: &str) -> StoreResult<PurgeReport> {
    let w = &workspace.to_owned();
    if !sessions(tx, workspace)?.is_empty() {
        return Err(Error::Conflict);
    }
    let operations = column(
        tx,
        "SELECT operation_id FROM workspace_mutation_receipts WHERE workspace_id=?1",
        w,
    )?;
    let grants = workspace_grants(tx, workspace)?;
    let mut owners = operations.clone();
    owners.extend(grants.iter().cloned());
    owners.extend(column(tx, "SELECT id FROM external_grants WHERE workspace_id=?1", w)?);
    let mut events: Vec<String>;
    loop {
        events = if owners.is_empty() {
            Vec::new()
        } else {
            column(tx, &format!("SELECT event_id FROM events WHERE owner {IN}"), &json(&owners))?
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
            held.extend(column(tx, &sql.replace("{IN}", IN), &json(&operations))?);
        }
    }
    if !owners.is_empty() {
        held.extend(column(
            tx,
            &format!("SELECT artifact_id FROM artifact_owners WHERE owner {IN}"),
            &json(&owners),
        )?);
    }

    let mut rows = 0;
    let mut by = |sql: &str, ids: &[String]| -> StoreResult<()> {
        if !ids.is_empty() {
            rows += tx.execute(&sql.replace("{IN}", IN), [json(ids)])?;
        }
        Ok(())
    };
    by("DELETE FROM projection_outbox WHERE event_id {IN}", &events)?;
    by("DELETE FROM events WHERE event_id {IN}", &events)?;
    by("DELETE FROM artifact_owners WHERE owner {IN}", &owners)?;
    for sql in OPERATION_ROWS {
        by(sql, &operations)?;
    }
    by("DELETE FROM authorization_grants WHERE id {IN}", &grants)?;
    for sql in [
        "DELETE FROM file_references WHERE workspace_id=?1",
        "DELETE FROM external_grants WHERE workspace_id=?1",
        "DELETE FROM tool_pins WHERE owner_kind='workspace' AND owner_id=?1",
        "DELETE FROM workspace_search_fts WHERE workspace_id=?1",
        "DELETE FROM workspaces WHERE id=?1",
    ] {
        rows += tx.execute(sql, [w])?;
    }
    held.sort();
    held.dedup();
    let artifacts = release(tx, &held)?;
    Ok(PurgeReport { rows, artifacts })
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
