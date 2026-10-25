#![forbid(unsafe_code)]
mod credential_slot;
pub use credential_slot::ToolCredentialSlotBindingSummaryV1;

mod agent_activity;
mod execution;
pub use agent_activity::*;

pub use execution::*;

use aura_domain_common::CanonicalSemVer;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    AgentRunId, CredentialId, CredentialRevision, ErrorCategory, ErrorCode, ModelStepId, Revision,
    SessionId, SessionSequence, TurnTaskId, UtcTimestamp, WorkspaceId,
};
use aura_domain_common::{
    ToolBatchId, ToolCallId, ToolConformanceRunId, ToolDraftId, ToolExecutionAttemptId, ToolId,
    ToolInstanceId, ToolPackageId,
};
use aura_execution_contract::SideEffectStateV1;
use aura_tool_contract::{
    CredentialSlotKey, EffectiveToolBindingSnapshot, NormalizedMcpToolContractFingerprintV1,
    NormalizedPluginToolContractFingerprintV1, NormalizedToolCallInputIssueV1,
    PackageFingerprintV1, PackageKey, PackageSecurityFingerprintV1,
    SignedBuiltinToolContractFingerprintV1, ToolActivityCursorV1, ToolAudienceV1,
    ToolBatchStatusV1, ToolCallStatusV1, ToolCancellationReasonV1, ToolCanonicalName,
    ToolDefinitionSnapshotRef, ToolExecutionAttemptStatusV1, ToolExposureV1,
    ToolIndeterminateReasonV1, ToolNotExecutedReasonV1, ToolPinResolutionStateV1, ToolPinV1,
    ToolRegistrySnapshotRef, ToolResultContractV1, ToolSchemaSnapshotV1, ToolSettingsSnapshotV1,
    ToolSourceKindV1, ToolVersion, UserReadableToolArtifactView,
};
pub use aura_tool_domain::ToolCredentialPlacementV1;
use aura_tool_domain::{
    CanonicalToolPackageInstanceConfigurationSchemaV1, ToolConformanceStatusV1,
    ToolDraftApprovalEvidenceV1, ToolDraftKindV1, ToolDraftPreviewV1, ToolDraftStatusV1,
    ToolExecutionSemanticsV1, ToolLongOperationRefV1, ToolLongOperationStateV1,
    ToolPackageDeleteApprovalEvidenceV1, ToolPackageDeleteDraftSnapshotV1, ToolPackageLifecycleV1,
    ToolRuntimeHealthV1,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Renderer-safe error facts. Internal diagnostic details and raw failures are
/// intentionally absent from every P07 projection.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolProjectionErrorView {
    pub code: ErrorCode,
    pub category: ErrorCategory,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageSummaryView {
    pub tool_package_id: ToolPackageId,
    pub package_key: PackageKey,
    pub active_version: Option<CanonicalSemVer>,
    pub lifecycle: ToolPackageLifecycleV1,
    pub deletion_pending: bool,
    pub trusted: bool,
    pub tool_count: u32,
    pub enabled_tool_count: u32,
    pub eligible_tool_count: u32,
    pub entity_revision: Revision,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftSummaryView {
    pub tool_draft_id: ToolDraftId,
    pub kind: ToolDraftKindV1,
    pub status: ToolDraftStatusV1,
    pub entity_revision: Revision,
    pub updated_at: UtcTimestamp,
    pub safe_error: Option<ToolProjectionErrorView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSettingsProjection {
    pub settings: ToolSettingsSnapshotV1,
    pub configuration_issue: Option<ToolProjectionErrorView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageListProjection {
    pub packages: Vec<ToolPackageSummaryView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftListProjection {
    pub drafts: Vec<ToolDraftSummaryView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceToolPinsProjection {
    pub workspace_id: WorkspaceId,
    pub pin_set_revision: Revision,
    pub pins: Vec<ToolPinProjectionEntryV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPinProjectionEntryV1 {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub pin_revision: Revision,
    pub state: ToolPinResolutionStateV1,
    pub safe_issue: Option<ToolProjectionErrorView>,
    pub resolved_binding: Option<EffectiveToolBindingSnapshot>,
}

impl From<ToolPinV1> for ToolPinProjectionEntryV1 {
    fn from(pin: ToolPinV1) -> Self {
        Self {
            tool_id: pin.tool_id,
            tool_version: pin.tool_version,
            pin_revision: pin.revision,
            state: pin.state,
            safe_issue: pin.safe_reason.map(|message| ToolProjectionErrorView {
                code: ErrorCode::new("tool.pin.unresolved")
                    .expect("frozen Tool Pin projection error code"),
                category: ErrorCategory::Conflict,
                message,
                retryable: false,
            }),
            resolved_binding: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionToolPinsProjectionV1 {
    pub session_id: SessionId,
    pub pin_set_revision: Revision,
    pub pins: Vec<ToolPinProjectionEntryV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceToolRegistrySummaryProjection {
    pub workspace_id: WorkspaceId,
    pub registry_snapshot_ref: Option<ToolRegistrySnapshotRef>,
    pub discoverable_tool_count: u32,
    pub exposed_tool_count: u32,
    pub unresolved_pin_count: u32,
    pub capacity_remediation_required: bool,
    pub safe_issue: Option<ToolProjectionErrorView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveToolView {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub tool_instance_id: ToolInstanceId,
    pub canonical_name: ToolCanonicalName,
    pub display_name: String,
    pub summary: String,
    pub source: ToolSourceKindV1,
    pub exposure: ToolExposureV1,
    pub audience: ToolAudienceV1,
    pub enabled: bool,
    pub trusted: bool,
    pub runtime_health: ToolRuntimeHealthV1,
    pub pin_state: Option<ToolPinResolutionStateV1>,
    pub eligible: bool,
    pub safe_ineligibility_reason: Option<ToolProjectionErrorView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEffectiveToolRegistryProjection {
    pub session_id: SessionId,
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub tools: Vec<EffectiveToolView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionLoadedToolsProjection {
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub tools: Vec<EffectiveToolView>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolResultOutcomeSummaryView {
    Succeeded,
    OperationRejected,
    Failed,
    Cancelled { reason: ToolCancellationReasonV1 },
    NotExecuted { reason: ToolNotExecutedReasonV1 },
    Indeterminate { reason: ToolIndeterminateReasonV1 },
}

/// Why a terminal call ended the way it did, in the terms the session shows.
///
/// It is the stored error's own category — never a reading of the message text — so the
/// row, the group header and the chip can name the same reason the Store recorded, and a
/// `not_found` never reads as a permission problem. `Other` is the honest answer for a
/// category this vocabulary does not distinguish; a legacy row with no error carries
/// `None` instead, which is a different fact from "something else went wrong".
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallErrorClassV1 {
    /// The named object does not exist.
    NotFound,
    /// The call was refused by policy, scope or credentials.
    Permission,
    /// The state the call assumed had changed.
    Conflict,
    /// The request itself was unusable: arguments, envelope or schema.
    InvalidRequest,
    /// The capability, backend or dependency was not available.
    Unavailable,
    /// A category this vocabulary does not further distinguish.
    Other,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCallActivitySummaryView {
    pub tool_call_id: ToolCallId,
    pub tool_batch_id: ToolBatchId,
    pub ordinal: u32,
    /// Null only for a ToolNotFound call whose Provider name was absent from the frozen request.
    #[serde(deserialize_with = "aura_domain_common::nullable::required")]
    #[schemars(with = "aura_domain_common::nullable::Required<ToolId>", required)]
    pub tool_id: Option<ToolId>,
    #[serde(deserialize_with = "aura_domain_common::nullable::required")]
    #[schemars(with = "aura_domain_common::nullable::Required<ToolVersion>", required)]
    pub tool_version: Option<ToolVersion>,
    pub display_name: String,
    pub status: ToolCallStatusV1,
    pub outcome: Option<ToolResultOutcomeSummaryView>,
    /// The classified reason for a terminal outcome, when the call recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_class: Option<ToolCallErrorClassV1>,
    pub side_effect_state: Option<SideEffectStateV1>,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchActivitySummaryView {
    pub tool_batch_id: ToolBatchId,
    pub status: ToolBatchStatusV1,
    pub call_count: u32,
    pub completed_call_count: u32,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionToolActivityProjection {
    pub session_id: SessionId,
    pub active_batches: Vec<ToolBatchActivitySummaryView>,
    pub recent_calls: Vec<ToolCallActivitySummaryView>,
    pub older_activity_cursor: Option<ToolActivityCursorV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageVersionSummaryView {
    pub package_version: CanonicalSemVer,
    pub package_fingerprint: PackageFingerprintV1,
    pub package_security_fingerprint: PackageSecurityFingerprintV1,
    pub conformance_run_ids: Vec<ToolConformanceRunId>,
    pub conformance_status: ToolConformanceStatusV1,
    pub installed_at: UtcTimestamp,
    pub payload_available: bool,
    pub payload_gc_eligible_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinitionSummaryView {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub canonical_name: ToolCanonicalName,
    pub display_name: String,
    pub model_summary: String,
    pub audience: ToolAudienceV1,
    pub entity_revision: Revision,
    pub instances: Vec<ToolDefinitionInstanceView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinitionVersionView {
    pub tool_version: ToolVersion,
    pub model_description: String,
    pub model_summary: String,
    pub schema: ToolSchemaSnapshotV1,
    pub result_contract: ToolResultContractV1,
    pub source: ToolSourceKindV1,
    pub audience: ToolAudienceV1,
    pub execution_semantics: ToolExecutionSemanticsV1,
    pub instance_configuration_schema: CanonicalToolPackageInstanceConfigurationSchemaV1,
    pub capability_count: u32,
    pub resource_claim_count: u32,
    pub payload_available: bool,
    pub instances: Vec<ToolDefinitionInstanceView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinitionInstanceView {
    pub tool_instance_id: ToolInstanceId,
    pub tool_version: ToolVersion,
    pub enabled: bool,
    pub trusted: bool,
    pub runtime_health: ToolRuntimeHealthV1,
    pub ready: bool,
    pub entity_revision: Revision,
    pub configuration: Option<ToolInstanceConfigurationProjectionV1>,
    pub credential_slot_binding: Option<ToolCredentialSlotBindingSummaryV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstanceConfigurationProjectionV1 {
    pub safe_non_secret_value: Value,
    pub configuration_revision: Revision,
    pub configuration_hash: CanonicalHashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBoundCredentialSummaryV1 {
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub binding_revision: Revision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCredentialCandidateCompatibilityV1 {
    Compatible,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolCredentialScopeSummaryV1 {
    User,
    Workspace,
    Session,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialCandidateViewV1 {
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub display_name: String,
    pub credential_kind: aura_tool_domain::ToolCredentialKindV1,
    pub state: aura_tool_domain::ToolCredentialStateV1,
    pub compatibility: ToolCredentialCandidateCompatibilityV1,
    pub scope_summary: ToolCredentialScopeSummaryV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialCandidatesProjectionV1 {
    pub tool_instance_id: ToolInstanceId,
    pub instance_revision: Revision,
    pub credential_slot_key: CredentialSlotKey,
    pub placement: ToolCredentialPlacementV1,
    pub workspace_id: Option<WorkspaceId>,
    pub session_id: Option<SessionId>,
    pub candidate_set_revision: Revision,
    pub candidates: Vec<ToolCredentialCandidateViewV1>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolLongOperationProjectionV1 {
    pub operation: ToolLongOperationRefV1,
    pub state: ToolLongOperationStateV1,
    pub operational_revision: Revision,
    pub can_cancel: bool,
    pub safe_error: Option<ToolProjectionErrorView>,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDefinitionSelectionBasisV1 {
    SignedBuiltinRelease,
    ActivePackageVersion,
    ExternalSourceAuthority,
    ExplicitInstanceSelection,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDefinitionSelectionIssueV1 {
    ReleaseDefinitionUnavailable,
    ActivePackageVersionUnavailable,
    ExternalSourceAuthorityUnavailable,
    NoReadyInstance,
    ExplicitSelectionUnavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SignedBuiltinReleaseHashV1(pub CanonicalHashV1);

impl SignedBuiltinReleaseHashV1 {
    #[must_use]
    pub const fn new(hash: CanonicalHashV1) -> Self {
        Self(hash)
    }

    #[must_use]
    pub const fn hash(&self) -> &CanonicalHashV1 {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpToolSourceAuthorityRefV1 {
    pub source_contract_key: String,
    pub authority_revision: Revision,
    pub normalized_mcp_tool_contract_fingerprint: NormalizedMcpToolContractFingerprintV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginToolSourceAuthorityRefV1 {
    pub source_contract_key: String,
    pub authority_revision: Revision,
    pub normalized_plugin_tool_contract_fingerprint: NormalizedPluginToolContractFingerprintV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "source_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolDefinitionExternalSourceAuthorityRefV1 {
    Mcp { authority: McpToolSourceAuthorityRefV1 },
    Plugin { authority: PluginToolSourceAuthorityRefV1 },
}

/// Source-owned identity used only by the global Tool Definition projection.
///
/// This reference deliberately contains no `trs_` binding. Callers must
/// resolve it again through the target Workspace, Session, or child Registry
/// before Load or Execute can reach admission.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolDefinitionSourceSelectionRefV1 {
    SignedBuiltinRelease {
        tool_id: ToolId,
        tool_version: ToolVersion,
        tool_instance_id: ToolInstanceId,
        tool_instance_revision: Revision,
        signed_builtin_release_hash: SignedBuiltinReleaseHashV1,
        definition_fingerprint: CanonicalHashV1,
        source_contract_fingerprint: SignedBuiltinToolContractFingerprintV1,
    },
    ActivePackageVersion {
        tool_package_id: ToolPackageId,
        package_version: CanonicalSemVer,
        package_revision: Revision,
        tool_id: ToolId,
        tool_version: ToolVersion,
        tool_instance_id: ToolInstanceId,
        tool_instance_revision: Revision,
        definition_fingerprint: CanonicalHashV1,
        package_fingerprint: PackageFingerprintV1,
        package_security_fingerprint: PackageSecurityFingerprintV1,
    },
    ExternalSourceAuthority {
        tool_id: ToolId,
        tool_version: ToolVersion,
        tool_instance_id: ToolInstanceId,
        tool_instance_revision: Revision,
        definition_fingerprint: CanonicalHashV1,
        authority: ToolDefinitionExternalSourceAuthorityRefV1,
    },
}

impl ToolDefinitionSourceSelectionRefV1 {
    #[must_use]
    pub const fn tool_id(&self) -> ToolId {
        match self {
            Self::SignedBuiltinRelease { tool_id, .. }
            | Self::ActivePackageVersion { tool_id, .. }
            | Self::ExternalSourceAuthority { tool_id, .. } => *tool_id,
        }
    }

    #[must_use]
    pub const fn tool_instance_id(&self) -> ToolInstanceId {
        match self {
            Self::SignedBuiltinRelease { tool_instance_id, .. }
            | Self::ActivePackageVersion { tool_instance_id, .. }
            | Self::ExternalSourceAuthority { tool_instance_id, .. } => *tool_instance_id,
        }
    }

    #[must_use]
    pub const fn source_basis(&self) -> ToolDefinitionSelectionBasisV1 {
        match self {
            Self::SignedBuiltinRelease { .. } => {
                ToolDefinitionSelectionBasisV1::SignedBuiltinRelease
            }
            Self::ActivePackageVersion { .. } => {
                ToolDefinitionSelectionBasisV1::ActivePackageVersion
            }
            Self::ExternalSourceAuthority { .. } => {
                ToolDefinitionSelectionBasisV1::ExternalSourceAuthority
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolDefinitionSelectionCandidateV1 {
    pub source_selection_ref: ToolDefinitionSourceSelectionRefV1,
    pub ready: bool,
    pub explicitly_selected: bool,
}

/// Resolves global display/configuration selection without creating or
/// guessing a scoped Registry Snapshot. Multiple viable instances stay
/// ambiguous unless exactly one has an explicit source-owned selection.
#[must_use]
pub fn resolve_tool_definition_current_selection_v1(
    candidates: &[ToolDefinitionSelectionCandidateV1],
) -> ToolDefinitionCurrentSelectionV1 {
    let mut ready = candidates.iter().filter(|candidate| candidate.ready).collect::<Vec<_>>();
    ready.sort_by_key(|candidate| candidate.source_selection_ref.tool_instance_id());
    let explicitly_selected =
        ready.iter().copied().filter(|candidate| candidate.explicitly_selected).collect::<Vec<_>>();
    match explicitly_selected.as_slice() {
        [candidate] => ToolDefinitionCurrentSelectionV1::Resolved {
            source_selection_ref: candidate.source_selection_ref.clone(),
            selection_basis: ToolDefinitionSelectionBasisV1::ExplicitInstanceSelection,
        },
        [] => match ready.as_slice() {
            [] => ToolDefinitionCurrentSelectionV1::Unavailable {
                reason: ToolDefinitionSelectionIssueV1::NoReadyInstance,
            },
            [candidate] => ToolDefinitionCurrentSelectionV1::Resolved {
                source_selection_ref: candidate.source_selection_ref.clone(),
                selection_basis: candidate.source_selection_ref.source_basis(),
            },
            _ => ToolDefinitionCurrentSelectionV1::Ambiguous {
                candidates: ready
                    .into_iter()
                    .map(|candidate| candidate.source_selection_ref.clone())
                    .collect(),
            },
        },
        _ => ToolDefinitionCurrentSelectionV1::Ambiguous {
            candidates: explicitly_selected
                .into_iter()
                .map(|candidate| candidate.source_selection_ref.clone())
                .collect(),
        },
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
// This projection DTO deliberately keeps the frozen public source-reference
// type unboxed; changing its Rust shape to silence a size hint would add no
// value on this low-frequency settings boundary.
#[allow(clippy::large_enum_variant)]
pub enum ToolDefinitionCurrentSelectionV1 {
    Resolved {
        source_selection_ref: ToolDefinitionSourceSelectionRefV1,
        selection_basis: ToolDefinitionSelectionBasisV1,
    },
    Unavailable {
        reason: ToolDefinitionSelectionIssueV1,
    },
    Ambiguous {
        candidates: Vec<ToolDefinitionSourceSelectionRefV1>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageProjection {
    pub tool_package_id: ToolPackageId,
    pub package_key: PackageKey,
    pub active_version: Option<CanonicalSemVer>,
    pub lifecycle: ToolPackageLifecycleV1,
    pub deletion_pending: bool,
    pub active_security_fingerprint: Option<PackageSecurityFingerprintV1>,
    pub trusted_security_fingerprint: Option<PackageSecurityFingerprintV1>,
    pub versions: Vec<ToolPackageVersionSummaryView>,
    pub tools: Vec<ToolDefinitionSummaryView>,
    pub entity_revision: Revision,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub safe_error: Option<ToolProjectionErrorView>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinitionProjection {
    pub tool_id: ToolId,
    pub canonical_name: ToolCanonicalName,
    pub display_name: String,
    pub versions: Vec<ToolDefinitionVersionView>,
    pub current_selection: ToolDefinitionCurrentSelectionV1,
    pub entity_revision: Revision,
    pub safe_error: Option<ToolProjectionErrorView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCallActivityHistoryViewV1 {
    pub session_sequence: SessionSequence,
    pub call: ToolCallActivitySummaryView,
    pub definition_ref: ToolDefinitionSnapshotRef,
    pub tool_instance_id: ToolInstanceId,
    pub source: ToolSourceKindV1,
    pub artifact_unavailable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolActivityPageV1 {
    pub session_id: SessionId,
    pub cutoff_session_sequence: SessionSequence,
    pub items: Vec<ToolCallActivityHistoryViewV1>,
    pub next_cursor: Option<ToolActivityCursorV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftProjection {
    pub tool_draft_id: ToolDraftId,
    pub kind: ToolDraftKindV1,
    pub status: ToolDraftStatusV1,
    pub current_draft_revision: Revision,
    pub draft_hash: aura_domain_common::jcs::CanonicalSha256HashV1,
    pub entity_revision: Revision,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub conformance_run_id: Option<ToolConformanceRunId>,
    pub preview: Option<ToolDraftPreviewV1>,
    pub approval: Option<ToolDraftApprovalEvidenceV1>,
    pub package_delete_snapshot: Option<ToolPackageDeleteDraftSnapshotV1>,
    pub package_delete_approval: Option<ToolPackageDeleteApprovalEvidenceV1>,
    pub safe_error: Option<ToolProjectionErrorView>,
    pub user_readable_artifacts: Vec<UserReadableToolArtifactView>,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolConformanceRunProjection {
    pub tool_conformance_run_id: ToolConformanceRunId,
    pub tool_draft_id: Option<ToolDraftId>,
    pub package_fingerprint: PackageFingerprintV1,
    pub status: ToolConformanceStatusV1,
    pub entity_revision: Revision,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
    pub safe_error: Option<ToolProjectionErrorView>,
    pub user_readable_artifacts: Vec<UserReadableToolArtifactView>,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchProjection {
    pub tool_batch_id: ToolBatchId,
    pub batch_content_hash: CanonicalHashV1,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub model_step_id: ModelStepId,
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub control_state_revision: Revision,
    pub control_state_hash: CanonicalHashV1,
    pub status: ToolBatchStatusV1,
    pub calls: Vec<ToolCallActivitySummaryView>,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
    pub safe_error: Option<ToolProjectionErrorView>,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCallProjection {
    pub tool_call_id: ToolCallId,
    pub tool_batch_id: ToolBatchId,
    pub ordinal: u32,
    /// Null only for a ToolNotFound call whose Provider name was absent from the frozen request.
    #[serde(deserialize_with = "aura_domain_common::nullable::required")]
    #[schemars(
        with = "aura_domain_common::nullable::Required<ToolDefinitionSnapshotRef>",
        required
    )]
    pub definition_ref: Option<ToolDefinitionSnapshotRef>,
    pub purpose: Option<String>,
    pub invalid_input_issue: Option<NormalizedToolCallInputIssueV1>,
    pub status: ToolCallStatusV1,
    pub outcome: Option<ToolResultOutcomeSummaryView>,
    pub side_effect_state: Option<SideEffectStateV1>,
    pub attempt_ids: Vec<ToolExecutionAttemptId>,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
    pub safe_error: Option<ToolProjectionErrorView>,
    pub model_visible_result: Option<UserReadableToolArtifactView>,
    pub user_audit_result: Option<UserReadableToolArtifactView>,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolAttemptProgressView {
    pub sequence: u64,
    pub percentage: Option<u8>,
    pub message: Option<String>,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionAttemptProjection {
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub tool_call_id: ToolCallId,
    pub attempt_number: u8,
    pub status: ToolExecutionAttemptStatusV1,
    pub side_effect_state: SideEffectStateV1,
    pub progress: Option<ToolAttemptProgressView>,
    pub started_at: UtcTimestamp,
    pub dispatch_prepared_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
    pub safe_error: Option<ToolProjectionErrorView>,
    pub user_readable_artifacts: Vec<UserReadableToolArtifactView>,
    pub can_cancel: bool,
}

#[cfg(test)]
mod tests {
    use aura_domain_common::CanonicalSemVer;
    use aura_domain_common::jcs::{CanonicalBlake3HashV1, CanonicalHashV1};
    use aura_domain_common::{Revision, WorkspaceId};
    use aura_domain_common::{ToolInstanceId, ToolPackageId};
    use aura_tool_contract::{
        NormalizedMcpToolContractFingerprintV1, PackageFingerprintV1, PackageSecurityFingerprintV1,
        SignedBuiltinToolContractFingerprintV1, ToolPinResolutionStateV1, ToolPinV1, ToolVersion,
    };

    use super::{
        McpToolSourceAuthorityRefV1, SignedBuiltinReleaseHashV1, ToolDefinitionCurrentSelectionV1,
        ToolDefinitionExternalSourceAuthorityRefV1, ToolDefinitionSelectionBasisV1,
        ToolDefinitionSelectionCandidateV1, ToolDefinitionSourceSelectionRefV1,
        WorkspaceToolPinsProjection, resolve_tool_definition_current_selection_v1,
    };

    #[test]
    fn workspace_pin_projection_keeps_exact_tool_version() {
        let projection = WorkspaceToolPinsProjection {
            workspace_id: WorkspaceId::new(),
            pin_set_revision: Revision::INITIAL,
            pins: vec![
                ToolPinV1 {
                    tool_id: aura_domain_common::ToolId::new(),
                    tool_version: ToolVersion::new(None, Revision::INITIAL).expect("tool version"),
                    state: ToolPinResolutionStateV1::Resolved,
                    safe_reason: None,
                    revision: Revision::INITIAL,
                }
                .into(),
            ],
        };
        assert_eq!(projection.pins.len(), 1);
    }

    fn builtin_ref(instance_byte: u8) -> ToolDefinitionSourceSelectionRefV1 {
        ToolDefinitionSourceSelectionRefV1::SignedBuiltinRelease {
            tool_id: aura_domain_common::ToolId::from_bytes([1; 16]),
            tool_version: ToolVersion::new(None, Revision::INITIAL).expect("version"),
            tool_instance_id: ToolInstanceId::from_bytes([instance_byte; 16]),
            tool_instance_revision: Revision::INITIAL,
            signed_builtin_release_hash: SignedBuiltinReleaseHashV1::new(CanonicalHashV1::sha256(
                b"release",
            )),
            definition_fingerprint: CanonicalHashV1::sha256(b"definition"),
            source_contract_fingerprint: SignedBuiltinToolContractFingerprintV1::new(
                CanonicalHashV1::sha256(b"source"),
            ),
        }
    }

    #[test]
    fn global_source_refs_are_typed_and_never_contain_a_registry_snapshot() {
        let package = ToolDefinitionSourceSelectionRefV1::ActivePackageVersion {
            tool_package_id: ToolPackageId::from_bytes([2; 16]),
            package_version: CanonicalSemVer::new("1.2.3").expect("semver"),
            package_revision: Revision::new(4),
            tool_id: aura_domain_common::ToolId::from_bytes([3; 16]),
            tool_version: ToolVersion::new(
                Some(CanonicalSemVer::new("1.2.3").expect("semver")),
                Revision::new(7),
            )
            .expect("version"),
            tool_instance_id: ToolInstanceId::from_bytes([4; 16]),
            tool_instance_revision: Revision::new(9),
            definition_fingerprint: CanonicalHashV1::sha256(b"package definition"),
            package_fingerprint: PackageFingerprintV1(
                aura_domain_common::jcs::CanonicalSha256HashV1::sha256(b"package"),
            ),
            package_security_fingerprint: PackageSecurityFingerprintV1(
                CanonicalBlake3HashV1::blake3(b"security"),
            ),
        };
        let external = ToolDefinitionSourceSelectionRefV1::ExternalSourceAuthority {
            tool_id: aura_domain_common::ToolId::from_bytes([5; 16]),
            tool_version: ToolVersion::new(None, Revision::INITIAL).expect("version"),
            tool_instance_id: ToolInstanceId::from_bytes([6; 16]),
            tool_instance_revision: Revision::new(2),
            definition_fingerprint: CanonicalHashV1::sha256(b"external definition"),
            authority: ToolDefinitionExternalSourceAuthorityRefV1::Mcp {
                authority: McpToolSourceAuthorityRefV1 {
                    source_contract_key: "mcp.fixture.v1".to_owned(),
                    authority_revision: Revision::new(3),
                    normalized_mcp_tool_contract_fingerprint:
                        NormalizedMcpToolContractFingerprintV1::new(CanonicalHashV1::sha256(b"mcp")),
                },
            },
        };
        for source_ref in [builtin_ref(7), package, external] {
            let json = serde_json::to_string(&source_ref).expect("source ref");
            assert!(!json.contains("registry_snapshot_ref"));
            assert!(!json.contains("tool_registry_snapshot_id"));
        }
    }

    #[test]
    fn multiple_ready_instances_are_ambiguous_until_one_is_explicit() {
        let first = ToolDefinitionSelectionCandidateV1 {
            source_selection_ref: builtin_ref(10),
            ready: true,
            explicitly_selected: false,
        };
        let second = ToolDefinitionSelectionCandidateV1 {
            source_selection_ref: builtin_ref(11),
            ready: true,
            explicitly_selected: false,
        };
        let forward =
            resolve_tool_definition_current_selection_v1(&[first.clone(), second.clone()]);
        let reverse =
            resolve_tool_definition_current_selection_v1(&[second.clone(), first.clone()]);
        assert_eq!(forward, reverse);
        assert!(matches!(
            forward,
            ToolDefinitionCurrentSelectionV1::Ambiguous { candidates }
                if candidates.len() == 2
        ));

        let explicit = resolve_tool_definition_current_selection_v1(&[
            first,
            ToolDefinitionSelectionCandidateV1 { explicitly_selected: true, ..second },
        ]);
        assert!(matches!(
            explicit,
            ToolDefinitionCurrentSelectionV1::Resolved {
                selection_basis: ToolDefinitionSelectionBasisV1::ExplicitInstanceSelection,
                ..
            }
        ));
    }
}
