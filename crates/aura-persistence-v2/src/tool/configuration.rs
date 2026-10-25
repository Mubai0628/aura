//! Durable identity for a user-owned global settings command. The settings file
//! retains its own CAS/atomic replacement; retries cannot overwrite newer edits.
use aura_domain_common::{CommandInvocationId, EventId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, event, record};
use aura_tool_contract::{AgentToolSettingsUpdate, AgentToolSettingsView};
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationCommand {
    pub input: AgentToolSettingsUpdate,
    pub result: Option<AgentToolSettingsView>,
}

pub fn load(
    db: &Connection,
    command: CommandInvocationId,
) -> StoreResult<Option<ConfigurationCommand>> {
    let payload: Option<String> = db.query_row(
        "SELECT payload FROM events WHERE owner=?1 AND kind='tool.configuration_command' AND version=1 ORDER BY sequence DESC LIMIT 1",
        [command.to_string()], |row| row.get(0)).optional()?;
    payload.map(|p| record::decode(&p)).transpose()
}

pub fn save(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    value: &ConfigurationCommand,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if let Some(previous) = load(tx, command)? {
        if previous == *value {
            return Ok(());
        }
        if previous.input != value.input || previous.result.is_some() {
            return Err(Error::Conflict);
        }
    }
    if let Some(result) = &value.result
        && (result.fixed_tool_ids != value.input.fixed_tool_ids
            || result.max_dynamic_tool_schemas != value.input.max_dynamic_tool_schemas
            || ![value.input.expected_revision.value(), value.input.expected_revision.value() + 1]
                .contains(&result.revision.value()))
    {
        return Err(Error::Conflict);
    }
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &command.to_string(),
            kind: "tool.configuration_command",
            version: 1,
            payload: &serde_json::to_value(value)?,
            at,
        },
        "tool",
    )?;
    Ok(())
}
