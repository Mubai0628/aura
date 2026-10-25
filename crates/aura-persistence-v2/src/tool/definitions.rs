//! Exact domain contracts in the existing Tool tables. Neither reading nor
//! inserting a definition installs, enables, trusts, or exposes that Tool.
use aura_domain_common::{ToolId, ToolInstanceId};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_tool_contract::ToolVersion;
use aura_tool_domain::{ToolDefinitionRecordV1, ToolInstanceRecordV1};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub fn get(
    db: &Connection,
    id: ToolId,
    version: &ToolVersion,
) -> StoreResult<Option<ToolDefinitionRecordV1>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM tool_definitions WHERE tool_id=?1 AND version=?2",
            params![id.to_string(), record::encode(version)?],
            |r| r.get(0),
        )
        .optional()?;
    let value: Option<ToolDefinitionRecordV1> = raw.map(|r| record::decode(&r)).transpose()?;
    if let Some(value) = &value {
        value.validate().map_err(|_| Error::Conflict)?;
        if value.tool_id != id || value.tool_version != *version {
            return Err(Error::Conflict);
        }
    }
    Ok(value)
}
/// The stored instance record with this id, if any.
pub fn instance(db: &Connection, id: ToolInstanceId) -> StoreResult<Option<ToolInstanceRecordV1>> {
    let raw: Option<String> = db
        .query_row("SELECT payload FROM tool_instances WHERE id=?1", [id.to_string()], |r| r.get(0))
        .optional()?;
    let value: Option<ToolInstanceRecordV1> = raw.map(|r| record::decode(&r)).transpose()?;
    if value.as_ref().is_some_and(|value| value.tool_instance_id != id) {
        return Err(Error::Conflict);
    }
    Ok(value)
}
pub fn put(
    tx: &Transaction<'_>,
    definition: &ToolDefinitionRecordV1,
    instance: &ToolInstanceRecordV1,
) -> StoreResult<()> {
    definition.validate().map_err(|_| Error::Conflict)?;
    if definition.tool_id != instance.tool_id || definition.tool_version != instance.tool_version {
        return Err(Error::Conflict);
    }
    if let Some(previous) = get(tx, definition.tool_id, &definition.tool_version)? {
        if previous != *definition {
            return Err(Error::Conflict);
        }
    } else {
        tx.execute(
            "INSERT INTO tool_definitions VALUES(?1,?2,?3,?4)",
            params![
                definition.tool_id.to_string(),
                record::encode(&definition.tool_version)?,
                record::encode(&definition.definition_fingerprint)?,
                record::encode(definition)?
            ],
        )?;
    }
    let previous: Option<String> = tx
        .query_row(
            "SELECT payload FROM tool_instances WHERE id=?1",
            [instance.tool_instance_id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(previous) = previous {
        if record::decode::<ToolInstanceRecordV1>(&previous)? != *instance {
            return Err(Error::Conflict);
        }
    } else {
        tx.execute(
            "INSERT INTO tool_instances VALUES(?1,?2,?3,?4,?5)",
            params![
                instance.tool_instance_id.to_string(),
                instance.tool_id.to_string(),
                record::encode(&instance.tool_version)?,
                i64::try_from(instance.entity_revision.value()).map_err(|_| Error::Conflict)?,
                record::encode(instance)?
            ],
        )?;
    }
    Ok(())
}
