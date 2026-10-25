use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::ErrorCode;
use aura_domain_common::ToolCallId;
use aura_domain_common::jcs::{canonical_json_jcs, canonical_sha256};
use aura_permission_domain::{SecuritySource, ToolSecuritySourceRefV1};
use aura_tool_contract::{
    CompiledToolSchemaV1, MAX_PROVIDER_ARGUMENTS_BYTES, NormalizedToolCallInputIssueV1,
    NormalizedToolCallInputStateV1, ToolBatchErrorCode, ToolBatchRequestV1,
    ToolDefinitionSnapshotRef, ToolNotExecutedReasonV1, ToolRegistryErrorCode,
    ToolRegistrySnapshotV1, ToolSchemaErrorCode, ToolSchemaValidationReportV1,
};
use aura_tool_domain::ToolDefinitionRecordV1;
use thiserror::Error;

#[cfg(test)]
mod tests;

mod types;
pub use types::*;
mod authority;
pub use authority::*;
mod batch;
pub use batch::admit_tool_batch;
mod validation;
pub use validation::validate_unique_admitted_results;
use validation::*;
mod rejection;
use rejection::*;
