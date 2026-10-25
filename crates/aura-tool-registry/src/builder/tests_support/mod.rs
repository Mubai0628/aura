use std::collections::BTreeSet;

use aura_domain_common::jcs::{CanonicalBlake3HashV1, CanonicalHashV1};
use aura_domain_common::{
    AgentRunId, PermissionSnapshotId, Revision, SessionId, TurnTaskId, UtcTimestamp, WorkspaceId,
};
use aura_domain_common::{ToolId, ToolInstanceId, ToolPackageId};
use aura_tool_contract::{
    PackageKey, PackageSecurityFingerprintV1, ToolAudienceV1, ToolCanonicalName, ToolExposureV1,
    ToolInputSchemaV1, ToolKey, ToolLoadStateV1, ToolPinResolutionStateV1, ToolPinV1,
    ToolRegistryScopeV1, ToolResultContractV1, ToolRuntimeDescriptorV1, ToolSettingsSnapshotV1,
    ToolSourceContractFingerprintV1, ToolSourceKindV1, ToolVersion,
};
use aura_tool_domain::{
    ToolDefinitionRecordV1, ToolEphemeralLoadRecordV1, ToolExecutionSemanticsV1,
    ToolInstanceOverridesV1, ToolInstanceRecordV1, ToolPackageTrustRecordV1, ToolRuntimeHealthV1,
};
use serde_json::json;

use super::{
    ChildRegistryDerivationInputV1, ChildRegistryExclusionReasonV1, ChildRegistryParentToolV1,
    ParentRegistryBuildInputV1, RegistryBuildError, RegistryCandidateV1, RegistryExclusionReasonV1,
    ToolSourceEligibilityEvidenceV1, build_parent_registry_snapshot,
    derive_child_registry_snapshot_v1, is_child_hard_denied,
};
mod support;
use support::*;
mod derivation;
mod selection;

pub(crate) use support::{base_input, package_candidate, search_fixture_input};
