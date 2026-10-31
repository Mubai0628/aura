//! Package tools in Registry snapshots. Package tools are never Core: like any
//! non-built-in tool they are discoverable, and `tools.load` brings them into a
//! session. Their definitions are stored with the snapshot that names them, so
//! search, describe and admission read the exact records the snapshot froze.
use super::InstalledPackageTool;
use aura_persistence_core::{StoreResult, Transaction};
use aura_tool_contract::ToolRegistrySnapshotV1;
use aura_tool_domain::{ToolDefinitionRecordV1, ToolInstanceRecordV1};

/// The definition and instance records of the package tools a snapshot names.
pub type SnapshotPackageTools = Vec<(ToolDefinitionRecordV1, ToolInstanceRecordV1)>;

#[must_use]
pub fn named_by(
    tools: &[InstalledPackageTool],
    registry: &ToolRegistrySnapshotV1,
) -> SnapshotPackageTools {
    tools
        .iter()
        .filter(|tool| {
            registry.catalog_entries.iter().any(|entry| {
                entry.tool_id == tool.definition.tool_id
                    && entry.tool_version == tool.definition.tool_version
                    && entry.definition_fingerprint == tool.definition.definition_fingerprint
            })
        })
        .map(|tool| (tool.definition.clone(), tool.instance.clone()))
        .collect()
}

/// Stores the records with the snapshot. Records are immutable per tool version
/// and install, so storing them again is a no-op and a mismatch is a conflict.
pub fn store(tx: &Transaction<'_>, tools: &SnapshotPackageTools) -> StoreResult<()> {
    for (definition, instance) in tools {
        aura_persistence_v2::tool::definitions::put(tx, definition, instance)?;
    }
    Ok(())
}
