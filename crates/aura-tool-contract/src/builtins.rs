use aura_domain_common::{ErrorCategory, ErrorCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    PackageResourceClaimKeyV1, ToolAudienceV1, ToolCanonicalName, ToolDefinitionSnapshotRef,
    ToolInputSchemaV1, ToolLoadStateV1, ToolRegistrySnapshotRef, ToolResultContractV1,
    ToolSchemaSnapshotV1, ToolSourceKindV1, ToolVersion,
};
use aura_domain_common::{ToolId, ToolInstanceId};

pub const TOOLS_SEARCH_MODEL_SUMMARY_V1: &str =
    "Search discoverable tools in the caller's exact registry snapshot.";
pub const TOOLS_SEARCH_MODEL_DESCRIPTION_V1: &str = concat!(
    "Searches the immutable Tool Registry Snapshot available to the current ",
    "agent by canonical name, display name, summary, and model description. ",
    "Returns exact ToolId and ToolVersion references and an opaque cursor. ",
    "It does not install, trust, pin, load, unload, or execute a tool. ",
    "Use tools.describe to inspect an exact result and tools.load to expose ",
    "selected tools to a later model step."
);
pub const TOOLS_DESCRIBE_MODEL_SUMMARY_V1: &str =
    "Describe an exact tool version from an exact registry snapshot.";
pub const TOOLS_DESCRIBE_MODEL_DESCRIPTION_V1: &str = concat!(
    "Returns the complete model-visible contract for one exact ToolId and ",
    "ToolVersion from the supplied immutable Tool Registry Snapshot. ",
    "It never substitutes a newer version. The result describes the frozen ",
    "schema, result contract, source, audience, capabilities, resource ",
    "requirements, and current loadability."
);
pub const TOOLS_LOAD_MODEL_SUMMARY_V1: &str =
    "Atomically load exact tool versions for later model steps in this TurnTask.";
pub const TOOLS_LOAD_MODEL_DESCRIPTION_V1: &str = concat!(
    "Atomically exposes one or more exact tool versions as TurnTask-scoped ",
    "ephemeral tools. All targets are validated before any change is committed. ",
    "The operation does not install, trust, enable, or pin tools. Successfully ",
    "loaded tools become available only in a newly created registry snapshot ",
    "for a later model step."
);
pub const TOOLS_UNLOAD_MODEL_SUMMARY_V1: &str =
    "Atomically unload TurnTask-scoped ephemeral tools for later model steps.";
pub const TOOLS_UNLOAD_MODEL_DESCRIPTION_V1: &str = concat!(
    "Atomically removes one or more exact TurnTask-scoped ephemeral tool ",
    "exposures. It cannot remove Core tools, Workspace pins, or Session pins. ",
    "A target that is already not ephemerally loaded is an idempotent no-op. ",
    "Any real change creates a new immutable registry snapshot."
);
pub const TOOLS_LIST_LOADED_MODEL_SUMMARY_V1: &str =
    "List the exact tools exposed in the supplied registry snapshot.";
pub const TOOLS_LIST_LOADED_MODEL_DESCRIPTION_V1: &str = concat!(
    "Lists the Core, pinned, and TurnTask-ephemeral tools exposed by one exact ",
    "immutable Tool Registry Snapshot. It does not search the global registry, ",
    "modify exposure, or substitute a newer snapshot."
);

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinToolCapabilityV1 {
    ToolRegistryRead,
    ToolEphemeralExposureWrite,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BuiltinToolScopeRequirementV1 {
    ToolRegistrySnapshot,
    TurnTaskToolExposure,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuiltinToolDefinitionV1 {
    pub builtin_key: ToolCanonicalName,
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub model_summary: String,
    pub model_description: String,
    pub strict: bool,
    pub source: ToolSourceKindV1,
    pub audience: ToolAudienceV1,
    pub capabilities: Vec<BuiltinToolCapabilityV1>,
    pub scope_requirements: Vec<BuiltinToolScopeRequirementV1>,
    pub input_schema: ToolInputSchemaV1,
    pub result_contract: ToolResultContractV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSafeIssueViewV1 {
    pub code: ErrorCode,
    pub category: ErrorCategory,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCapabilitySummaryV1 {
    pub capability: String,
    pub summary: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolResourceClaimSummaryV1 {
    pub claim_key: PackageResourceClaimKeyV1,
    pub resource_kind: String,
    pub access: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDescribeResultV1 {
    pub definition_ref: ToolDefinitionSnapshotRef,
    pub canonical_name: ToolCanonicalName,
    pub display_name: String,
    pub model_summary: String,
    pub model_description: String,
    pub schema: ToolSchemaSnapshotV1,
    pub result_contract: ToolResultContractV1,
    pub source: ToolSourceKindV1,
    pub audience: ToolAudienceV1,
    pub capability_summary: Vec<ToolCapabilitySummaryV1>,
    pub resource_claim_summary: Vec<ToolResourceClaimSummaryV1>,
    pub load_state: ToolLoadStateV1,
    pub currently_loadable: bool,
    pub safe_unavailability_reason: Option<ToolSafeIssueViewV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolListLoadedRequestV1 {
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolLoadedEntryV1 {
    pub definition_ref: ToolDefinitionSnapshotRef,
    pub tool_instance_id: ToolInstanceId,
    pub canonical_name: ToolCanonicalName,
    pub display_name: String,
    pub model_summary: String,
    pub source: ToolSourceKindV1,
    pub audience: ToolAudienceV1,
    pub load_state: ToolLoadStateV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolListLoadedResultV1 {
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub tools: Vec<ToolLoadedEntryV1>,
}
