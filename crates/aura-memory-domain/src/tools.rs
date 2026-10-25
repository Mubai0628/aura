use crate::{MemoryScope, MemorySelector, MemoryStatus};
use aura_domain_common::{MemoryId, WorkspacePath};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemorySearchInput {
    pub scope: MemoryScope,
    pub query: String,
    pub category: Option<String>,
    pub limit: Option<u32>,
    #[serde(default)]
    pub include_inactive: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryReadInput {
    pub target: MemorySelector,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecentInput {
    pub scope: MemoryScope,
    pub limit: Option<u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryScopeInput {
    pub scope: MemoryScope,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryCreateInput {
    pub scope: MemoryScope,
    pub items: Vec<MemoryCreateItem>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryCreateItem {
    pub path: WorkspacePath,
    pub category: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 0 to 1: how sure the note is.
    #[schemars(range(min = 0, max = 1))]
    pub confidence: Option<f64>,
    /// 1 to 5: 3 is ordinary, 5 is critical.
    #[schemars(range(min = 1, max = 5))]
    pub importance: Option<u8>,
    /// 0 or more: how strongly recall prefers the note (1 is ordinary).
    #[schemars(range(min = 0))]
    pub recall_weight: Option<f64>,
    #[serde(default)]
    pub related_ids: Vec<MemoryId>,
    #[serde(default)]
    pub supersedes_ids: Vec<MemoryId>,
    #[serde(default)]
    pub conflicts_with_ids: Vec<MemoryId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryTarget {
    Id { id: MemoryId },
    Path { path: WorkspacePath },
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryUpdateInput {
    pub scope: MemoryScope,
    pub items: Vec<MemoryUpdateItem>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryUpdateItem {
    pub target: MemoryTarget,
    pub expected_hash: String,
    pub body: Option<String>,
    pub category: Option<String>,
    /// 0 to 1: how sure the note is.
    #[schemars(range(min = 0, max = 1))]
    pub confidence: Option<f64>,
    /// 1 to 5: 3 is ordinary, 5 is critical.
    #[schemars(range(min = 1, max = 5))]
    pub importance: Option<u8>,
    /// 0 or more: how strongly recall prefers the note (1 is ordinary).
    #[schemars(range(min = 0))]
    pub recall_weight: Option<f64>,
    pub tags: Option<Vec<String>>,
    pub related_ids: Option<Vec<MemoryId>>,
    pub supersedes_ids: Option<Vec<MemoryId>>,
    pub conflicts_with_ids: Option<Vec<MemoryId>>,
    pub status: Option<MemoryStatus>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRelation {
    Related,
    Supersedes,
    Conflicts,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryLinkInput {
    pub scope: MemoryScope,
    pub source: MemoryId,
    pub target: MemoryId,
    pub relation: MemoryRelation,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryStateInput {
    pub scope: MemoryScope,
    pub targets: Vec<MemoryTarget>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryDeleteInput {
    pub scope: MemoryScope,
    pub items: Vec<MemoryDeleteItem>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryDeleteItem {
    pub target: MemoryTarget,
    pub expected_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryRepairInput {
    pub scope: MemoryScope,
    pub phase: MemoryRepairPhase,
    pub preview_token: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRepairPhase {
    Preview,
    Apply,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryReceipt {
    pub id: MemoryId,
    pub scope: MemoryScope,
    pub path: WorkspacePath,
    pub title: String,
    pub hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutationResult {
    pub entries: Vec<MemoryReceipt>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryValidation {
    pub valid: u64,
    pub invalid: Vec<MemoryInvalidFile>,
    pub rebuilding: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryInvalidFile {
    pub path: String,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryRepairChange {
    pub path: WorkspacePath,
    pub expected_hash: Option<String>,
    pub proposed_hash: String,
    pub changes: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryRepairPreview {
    pub preview_token: String,
    pub changes: Vec<MemoryRepairChange>,
    pub manual: Vec<MemoryInvalidFile>,
    pub rebuild_index: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryRepairResult {
    pub preview: MemoryRepairPreview,
    pub applied: bool,
}
