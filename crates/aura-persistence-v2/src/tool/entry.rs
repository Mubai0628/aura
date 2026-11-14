//! The unified entry: a step's `tools.call` calls rewritten into the tools they name, from
//! the step's own frozen registry ([`aura_tool_contract::expand_entry_calls`]), and arguments
//! written as a string of JSON read as their tool takes them. Admission, Hooks, history and
//! recovery all read a step's calls through [`expand`], so each derives exactly what admission
//! admitted.
use aura_domain_common::{ToolCallId, ToolId};
use aura_model_domain::ProviderRequestEvidence;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_tool_contract::{NormalizedToolCallV1, TOOLS_CALL_NAME, ToolRegistrySnapshotV1};
use rusqlite::Connection;
use std::collections::BTreeMap;

/// Rewrites the step's `tools.call` calls, then reads the arguments a model wrote as a string of
/// JSON as their tool's schema takes them. Returns what the result of each call read so tells the
/// model.
pub fn expand(
    db: &Connection,
    evidence: &ProviderRequestEvidence,
    calls: &mut [NormalizedToolCallV1],
) -> StoreResult<BTreeMap<ToolCallId, String>> {
    expand_entry(db, evidence, calls)?;
    super::argument_repair::repair(db, calls)
}

/// Rewrites the step's `tools.call` calls. The registry is read only when the request sent
/// `tools.call` and one of the calls is bound to it, so steps without one cost nothing.
pub(crate) fn expand_entry(
    db: &Connection,
    evidence: &ProviderRequestEvidence,
    calls: &mut [NormalizedToolCallV1],
) -> StoreResult<()> {
    let Some(entry) = evidence
        .tool_name_bindings
        .iter()
        .find(|binding| binding.canonical_name == TOOLS_CALL_NAME)
        .map(|binding| &binding.tool_definition)
    else {
        return Ok(());
    };
    if !calls.iter().any(|call| call.definition_ref.as_ref() == Some(entry)) {
        return Ok(());
    }
    let registry = super::registry(db, entry.registry_snapshot_ref.tool_registry_snapshot_id)?
        .ok_or(Error::Conflict)?;
    aura_tool_contract::expand_entry_calls(&registry, calls);
    Ok(())
}

/// The canonical name of `tool_id` in the snapshot's catalog: what a Hook matches a call by,
/// whether its definition was sent or the model reached it through `tools.call`.
pub fn catalog_name(registry: &ToolRegistrySnapshotV1, tool_id: ToolId) -> Option<String> {
    registry
        .catalog_entries
        .iter()
        .find(|entry| entry.tool_id == tool_id)
        .map(|entry| entry.canonical_name.to_string())
}
