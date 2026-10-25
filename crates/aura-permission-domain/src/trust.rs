use aura_domain_common::{ContentHash, TrustRecordId, UtcTimestamp};
use aura_security_contract::TrustState;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ExternalIsolationMode, SecuritySource};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalExtensionKind {
    CustomTool,
    Hook,
    McpServer,
    Skill,
    Plugin,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalTrustRecord {
    pub id: TrustRecordId,
    pub source: SecuritySource,
    pub state: TrustState,
    pub effective_fingerprint: Option<ContentHash>,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustFingerprintInput {
    pub source: SecuritySource,
    pub manifest: serde_json::Value,
    pub schema: Option<serde_json::Value>,
    pub runtime_entry: Option<String>,
    pub content_hashes: Vec<ContentHash>,
    pub origin: Option<String>,
    pub capabilities: Vec<String>,
    pub resource_policies: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalExtensionRiskView {
    pub source_kind: ExternalExtensionKind,
    pub display_name: String,
    pub path_or_origin: String,
    pub runtime_isolation: Option<ExternalIsolationMode>,
    pub summary: String,
    pub warnings: Vec<String>,
}
