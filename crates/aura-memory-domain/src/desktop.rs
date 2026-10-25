//! Desktop management contracts. No caller-selected absolute path or Vault root.
use crate::*;
use aura_domain_common::WorkspaceId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryDesktopRequest {
    pub workspace_id: Option<WorkspaceId>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryVaultRequest {
    pub workspace_id: Option<WorkspaceId>,
    pub scope: MemoryScope,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryVaultStatus {
    pub scope: MemoryScope,
    pub directory: String,
    pub exists: bool,
    pub valid: u64,
    pub invalid: u64,
    pub rebuilding: bool,
    pub watcher_error: Option<String>,
    pub recent_changes: Vec<MemoryHistory>,
    /// The plain text log of this Vault's finished maintenance runs, once one was written.
    #[serde(default)]
    pub maintenance_log: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryDesktopSnapshot {
    pub vaults: Vec<MemoryVaultStatus>,
    pub recent_runs: Vec<MemoryMaintenanceRun>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryDesktopRepairApply {
    pub vault: MemoryVaultRequest,
    pub preview_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryDesktopActionResult {
    pub completed: bool,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryExportHandle {
    pub export_id: String,
    pub entries: u32,
    pub total_bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryExportEntryRequest {
    pub export_id: String,
    pub index: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryExportEntry {
    pub path: String,
    pub directory: bool,
    pub length: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryExportChunkRequest {
    pub export_id: String,
    pub index: u32,
    pub offset: u64,
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryExportChunk {
    pub offset: u64,
    pub next_offset: u64,
    pub bytes: Vec<u8>,
    pub eof: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryExportCloseRequest {
    pub export_id: String,
}
