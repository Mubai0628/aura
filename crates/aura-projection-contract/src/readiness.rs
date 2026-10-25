use aura_domain_common::{Revision, ToolWireVersionV1};
use aura_execution_contract::BackendReadiness;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreHealth {
    Starting,
    Ready,
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityReadiness {
    Ready,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionReadiness {
    Disabled,
    Ready,
    Degraded,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendLifecycle {
    Dormant,
    Starting,
    Ready,
    Degraded,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityStatus {
    pub capability: String,
    pub readiness: CapabilityReadiness,
    pub reason_code: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackendStatus {
    pub backend: String,
    pub lifecycle: BackendLifecycle,
    pub readiness: BackendReadiness,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionStatus {
    pub extension: String,
    pub readiness: ExtensionReadiness,
    pub reason_code: String,
}

/// Core integrity, service availability and optional execution facilities are
/// separate facts. Backend readiness does not grant permission to dispatch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeReadiness {
    pub schema_version: ToolWireVersionV1,
    pub revision: Revision,
    pub core_health: CoreHealth,
    pub capabilities: Vec<CapabilityStatus>,
    pub backends: Vec<BackendStatus>,
    pub extensions: Vec<ExtensionStatus>,
}
