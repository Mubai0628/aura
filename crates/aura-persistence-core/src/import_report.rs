use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportDisposition {
    Imported,
    Skipped,
    Invalid,
    RequiresUserAction,
    Conflict,
}

/// Report keys are opaque digests/typed IDs, never row payloads, secret values,
/// Windows credential targets or diagnostic messages copied from legacy data.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportItem {
    pub kind: String,
    pub source_key: String,
    pub disposition: ImportDisposition,
    pub reason_code: String,
    pub record_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportReport {
    pub schema_version: aura_domain_common::ToolWireVersionV1,
    pub source_fingerprint: aura_domain_common::ContentHash,
    pub applied: bool,
    pub already_applied: bool,
    pub items: Vec<ImportItem>,
}
