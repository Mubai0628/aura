//! TurnTask-scoped exposure and opaque cursor records. The application builds
//! immutable Registries; this Store owns their atomic publication and CAS.
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, event, record};
use aura_tool_contract::*;
use aura_tool_domain::ToolEphemeralLoadRecordV1;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExposureState {
    pub scope: ToolRegistryScopeV1,
    pub revision: Revision,
    pub head: ToolRegistrySnapshotRef,
    pub loads: Vec<ToolEphemeralLoadRecordV1>,
}
fn owner(scope: &ToolRegistryScopeV1) -> StoreResult<String> {
    match scope {
        ToolRegistryScopeV1::Session { session_id, turn_task_id, agent_run_id, .. } => {
            Ok(format!("{session_id}:{turn_task_id}:{agent_run_id}"))
        }
        _ => Err(Error::Conflict),
    }
}
pub fn load(db: &Connection, scope: &ToolRegistryScopeV1) -> StoreResult<Option<ExposureState>> {
    let raw: Option<String> = db.query_row("SELECT payload FROM events WHERE owner=?1 AND kind='tool.exposure_state' AND version=1 ORDER BY sequence DESC LIMIT 1",[owner(scope)?],|r|r.get(0)).optional()?;
    let value: Option<ExposureState> = raw.map(|v| record::decode(&v)).transpose()?;
    if value.as_ref().is_some_and(|v| v.scope != *scope) {
        return Err(Error::Conflict);
    }
    Ok(value)
}
pub fn save(
    tx: &Transaction<'_>,
    previous: Option<&ExposureState>,
    next: &ExposureState,
    registry: &ToolRegistrySnapshotV1,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if load(tx, &next.scope)?.as_ref() != previous
        || registry.scope != next.scope
        || registry.snapshot_ref != next.head
        || next.revision.value() != previous.map_or(1, |v| v.revision.value() + 1)
    {
        return Err(Error::Conflict);
    }
    let ToolRegistryScopeV1::Session { session_id, turn_task_id, .. } = &next.scope else {
        return Err(Error::Conflict);
    };
    if !next.loads.windows(2).all(|p| p[0].tool_id < p[1].tool_id)
        || next.loads.iter().any(|v| v.session_id != *session_id || v.turn_task_id != *turn_task_id)
    {
        return Err(Error::Conflict);
    }
    super::insert_registry(tx, registry)?;
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &owner(&next.scope)?,
            kind: "tool.exposure_state",
            version: 1,
            payload: &serde_json::to_value(next)?,
            at,
        },
        "tool",
    )?;
    Ok(())
}
pub fn cursor(
    db: &Connection,
    scope: &ToolRegistryScopeV1,
    token: &ToolSearchCursorV1,
) -> StoreResult<Option<serde_json::Value>> {
    let raw: Option<String> = db.query_row("SELECT payload FROM events WHERE owner=?1 AND kind='tool.search_cursor' AND version=1 AND json_extract(payload,'$.token')=?2",params![owner(scope)?,token.as_str()],|r|r.get(0)).optional()?;
    Ok(raw
        .map(|v| serde_json::from_str::<serde_json::Value>(&v))
        .transpose()?
        .map(|v| v["binding"].clone()))
}
pub fn save_cursor(
    tx: &Transaction<'_>,
    scope: &ToolRegistryScopeV1,
    token: &ToolSearchCursorV1,
    binding: &serde_json::Value,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if cursor(tx, scope, token)?.is_some() {
        return Err(Error::Conflict);
    }
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &owner(scope)?,
            kind: "tool.search_cursor",
            version: 1,
            payload: &serde_json::json!({"token":token,"binding":binding}),
            at,
        },
        "tool",
    )?;
    Ok(())
}
