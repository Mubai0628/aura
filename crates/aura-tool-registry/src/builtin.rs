use std::{collections::BTreeSet, sync::OnceLock};

use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256, parse_json_strict};
use aura_domain_common::{ContentHash, Revision, SecurityOperationId, UtcTimestamp};
use aura_domain_common::{ToolBatchId, ToolCallId, ToolExecutionAttemptId, ToolId, ToolInstanceId};
use aura_permission_domain::ToolSecuritySourceRefV1;
use aura_security_contract::{
    InvocationIdentity, PermissionAllowAuthorizationBasisV1, PermissionSnapshot, ResolvedCapability,
};
use aura_tool_contract::{
    BuiltinToolCapabilityV1, BuiltinToolDefinitionV1, BuiltinToolScopeRequirementV1,
    SignedBuiltinToolContractFingerprintV1, TOOLS_DESCRIBE_MODEL_DESCRIPTION_V1,
    TOOLS_DESCRIBE_MODEL_SUMMARY_V1, TOOLS_LIST_LOADED_MODEL_DESCRIPTION_V1,
    TOOLS_LIST_LOADED_MODEL_SUMMARY_V1, TOOLS_LOAD_MODEL_DESCRIPTION_V1,
    TOOLS_LOAD_MODEL_SUMMARY_V1, TOOLS_SEARCH_MODEL_DESCRIPTION_V1, TOOLS_SEARCH_MODEL_SUMMARY_V1,
    TOOLS_UNLOAD_MODEL_DESCRIPTION_V1, TOOLS_UNLOAD_MODEL_SUMMARY_V1, ToolAudienceV1,
    ToolCanonicalName, ToolDescribeRequestV1, ToolDescribeResultV1, ToolInputSchemaV1,
    ToolListLoadedRequestV1, ToolListLoadedResultV1, ToolLoadRequestV1, ToolLoadResultV1,
    ToolResultContractV1, ToolRuntimeDescriptorV1, ToolSearchPageV1, ToolSearchRequestV1,
    ToolSourceContractFingerprintV1, ToolSourceKindV1, ToolStructuredResultSchemaV1, ToolVersion,
};
use aura_tool_domain::{
    ToolDefinitionRecordV1, ToolExecutionSemanticsV1, ToolInstanceOverridesV1,
    ToolInstanceRecordV1, ToolRuntimeHealthV1,
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const BUILTIN_TOOL_ID_REGISTRY_SCHEMA_VERSION_V1: u32 = 1;

/// The pinned digest of [`BUILTIN_TOOL_ID_REGISTRY_SOURCE`].
///
/// R4: this registry is a **hand-authored source** manifest, not a generated one - it is not in
/// `RUST_GENERATED_PATHS`, so no generator rewrites it. Adding the `agent.activity.query` entry is
/// therefore an authoring act on the source, and this digest is updated from the real bytes of that
/// file rather than being guessed. The file's own test asserts the two agree.
pub const BUILTIN_TOOL_ID_REGISTRY_SOURCE_SHA256_V1: &str =
    "bcd9125d2b0383677414d481a564b4bec8df0b79551de3da424da720b2efe221";

const BUILTIN_TOOL_ID_REGISTRY_SOURCE: &[u8] =
    include_bytes!("../../../resources/tools/builtin-tool-id-registry.v1.json");

pub const BUILTIN_TOOL_DEFINITION_SET_SCHEMA_VERSION_V1: u32 = 1;

pub const BUILTIN_CORE_HANDLER_DISPATCH_BINDING_VERSION_V1: u32 = 1;

pub const BUILTIN_TOOL_DEFINITION_SET_SOURCE_SHA256_V1: &str =
    "047d33cdb357130fa32dc106b030be018b39fbb287ae5f4c3d7f59370bc1f584";

const BUILTIN_TOOL_DEFINITION_SET_SOURCE: &[u8] =
    include_bytes!("../../../resources/tools/builtin-tool-definition-set.v1.json");

#[cfg(test)]
mod tests;

mod ids;
pub use ids::*;
mod dispatch;
pub use dispatch::*;
mod release;
pub use release::*;
mod definitions;
use definitions::*;
