use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{
    AgentRunId, CommandInvocationId, ModelStepId, Revision, SessionId, TurnTaskId, UtcTimestamp,
    WorkspaceId,
};
use aura_domain_common::{ToolBatchId, ToolCallId, ToolId, ToolInstanceId, ToolRegistrySnapshotId};
use aura_tool_contract::{
    EffectiveToolBindingSnapshot, NormalizedToolCallInputIssueV1, NormalizedToolCallInputStateV1,
    NormalizedToolCallV1, ToolAudienceV1, ToolCanonicalName, ToolDefinitionSnapshotRef,
    ToolExposureV1, ToolInputSchemaV1, ToolLoadStateV1, ToolRegistryCatalogEntryV1,
    ToolRegistryScopeV1, ToolResultContractV1, ToolRuntimeDescriptorV1, ToolSchemaSnapshotV1,
    ToolSourceKindV1, ToolVersion,
};
use aura_tool_domain::ToolExecutionSemanticsV1;
use serde_json::{Value, json};

use super::*;
mod support;
use support::*;
mod entry;
mod envelope;
mod identity;
mod schema;
mod unknown;
