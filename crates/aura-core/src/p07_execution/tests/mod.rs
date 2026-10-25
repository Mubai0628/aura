use aura_tool_contract::{ToolBatchRequestV1, ToolSettingsSnapshotV1};
use aura_tool_domain::ToolDefinitionRecordV1;
use aura_tool_scheduler::{BatchAdmissionAuthorityV1, FrozenToolCallPlanningInputV1};
use std::path::PathBuf;

use aura_artifact_contract::{
    ArtifactAccess, ArtifactOwnerRef, ArtifactScope, ToolArtifactOwnerRef,
};
use aura_context_fixtures::{P06PreparedFixture, p06_prepared_tool_continuation_fixture};
use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{
    AgentRunId, CommandInvocationId, ModelStepId, Revision, SecurityOperationId, SessionId,
    TurnTaskId, WorkspaceId,
};
use aura_domain_common::{ToolBatchId, ToolCallId, ToolId, ToolInstanceId, ToolRegistrySnapshotId};
use aura_execution_contract::{
    AttemptSideEffectProofV1, SideEffectEvidenceProducerV1, VerifiedAttemptSideEffectEvidenceV1,
};
use aura_model_domain::LogicalConversationItem;
use aura_permission_domain::{PermissionDecision, ResolvedScope};
use aura_security_contract::TrustState;
use aura_security_contract::{InvocationActor, InvocationIdentity};
use aura_tool_contract::{
    EffectiveToolBindingSnapshot, NormalizedMcpToolContractFingerprintV1,
    NormalizedToolCallInputStateV1, NormalizedToolCallV1, ToolAudienceV1, ToolCanonicalName,
    ToolDefinitionSnapshotRef, ToolDescribeRequestV1, ToolExposureV1, ToolInputSchemaV1,
    ToolListLoadedRequestV1, ToolLoadRequestV1, ToolLoadStateV1, ToolNotExecutedReasonV1,
    ToolRegistryCatalogEntryV1, ToolRegistryScopeV1, ToolRegistrySnapshotRef, ToolResultContractV1,
    ToolRuntimeDescriptorV1, ToolSchemaSnapshotV1, ToolSearchQueryV1, ToolSearchRequestV1,
    ToolSourceContractFingerprintV1, ToolSourceKindV1, ToolVersion,
};
use aura_tool_domain::{
    ToolExecutionSemanticsV1, ToolInstanceOverridesV1, ToolInstanceRecordV1, ToolRuntimeHealthV1,
};
use aura_tool_registry::{
    BuiltinCoreHandlerV1, RegistryCandidateV1, ToolSourceEligibilityEvidenceV1,
    embedded_builtin_tool_definition_set_v1,
};
use aura_tool_scheduler::{BatchAdmissionAuthorityIssueV1, ToolCallResourcePreflightV1};
use rusqlite::Connection;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::watch;

use aura_persistence::{
    ArtifactWriteRequest, PersistenceOptions, PersistenceStartup, ToolRegistryBuildRequestV1,
};

use super::*;
use crate::{
    p07_builtin::{ToolBuiltinAssemblyInputV1, ToolBuiltinAssemblyServiceV1},
    p07_continuation::load_p08_tool_continuation_authority_v1,
    p07_registry::ToolRegistryControlServiceV1,
    p07_result::{
        CompleteDispatchedToolAttemptInputV1, CompleteDispatchedToolAttemptOutcomeV1,
        ToolAttemptCompletionServiceV1,
    },
    p07_test_support::{
        P08ContractTestDriverV1, commit_p06_projection_fixture, registry_request,
        seed_session_scope,
    },
};

// Behavior tests are real child modules; shared fixtures are imported explicitly.
mod admission;
#[path = "support/admission.rs"]
mod admission_support;
mod builtin;
#[path = "support/builtin.rs"]
mod builtin_support;
#[path = "support/continuation.rs"]
mod continuation_support;

// Retired the P07 package command, stdio and HTTP execution with the old package
// path; only the signed Built-in vertical remains.

use admission_support::*;

use builtin_support::*;

#[cfg(feature = "p08-contract-driver")]
pub(crate) use builtin::run_p08_builtin_vertical_fixture_v1;
