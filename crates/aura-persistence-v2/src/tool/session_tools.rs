//! Only explicit model load/unload commands write session dynamic membership.
//! Runtime exposure snapshots remain separately immutable and turn scoped.
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, event, record};
use aura_tool_contract::SessionDynamicTools;
use rusqlite::{Connection, OptionalExtension, Transaction};

pub fn load(db: &Connection, session: SessionId) -> StoreResult<Option<SessionDynamicTools>> {
    let raw: Option<String> = db.query_row(
        "SELECT payload FROM events WHERE owner=?1 AND kind='tool.session_dynamic' AND version=1 ORDER BY sequence DESC LIMIT 1",
        [format!("agent-tools:{session}")], |row| row.get(0)).optional()?;
    let value: Option<SessionDynamicTools> = raw.map(|v| record::decode(&v)).transpose()?;
    if value
        .as_ref()
        .is_some_and(|v| v.session_id != session || !v.tool_ids.windows(2).all(|p| p[0] < p[1]))
    {
        return Err(Error::Conflict);
    }
    Ok(value)
}

pub fn save(
    tx: &Transaction<'_>,
    previous: Option<&SessionDynamicTools>,
    next: &SessionDynamicTools,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if crate::workspace::session(tx, next.session_id)?.is_none()
        || load(tx, next.session_id)?.as_ref() != previous
        || next.revision.value() != previous.map_or(1, |v| v.revision.value() + 1)
        || !next.tool_ids.windows(2).all(|p| p[0] < p[1])
    {
        return Err(Error::Conflict);
    }
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &format!("agent-tools:{}", next.session_id),
            kind: "tool.session_dynamic",
            version: 1,
            payload: &serde_json::to_value(next)?,
            at,
        },
        "tool",
    )?;
    Ok(())
}
