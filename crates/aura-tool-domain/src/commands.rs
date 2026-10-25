use aura_domain_common::jcs::{CanonicalHashV1, CanonicalSha256HashV1};
use aura_domain_common::{
    ArtifactId, CredentialId, CredentialRevision, MutationJournalId, Revision, SessionId,
    WorkspaceId,
};
use aura_domain_common::{
    ToolConformanceRunId, ToolDraftId, ToolId, ToolInstanceId, ToolPackageId,
};
use aura_tool_contract::{
    CredentialSlotKey, PackageFingerprintV1, PackageSecurityFingerprintV1, ToolKey,
    ToolPackageConformanceCaseNameV1, ToolVersion,
};
use aura_workspace_domain::FileTargetRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ToolDraftTransferArtifactRefV1;

pub const TOOL_SET_ENABLED_METHOD_V1: &str = "tools.tool.set_enabled";
pub const TOOL_INSTANCE_UPDATE_CONFIGURATION_METHOD_V1: &str =
    "tools.instance.update_configuration";
pub const TOOL_INSTANCE_BIND_CREDENTIAL_METHOD_V1: &str = "tools.instance.bind_credential";
pub const TOOL_INSTANCE_CLEAR_CREDENTIAL_METHOD_V1: &str = "tools.instance.clear_credential";
pub const TOOL_PIN_WORKSPACE_SET_METHOD_V1: &str = "tools.pin.workspace.set";
pub const TOOL_PIN_WORKSPACE_CLEAR_METHOD_V1: &str = "tools.pin.workspace.clear";
pub const TOOL_PIN_SESSION_SET_METHOD_V1: &str = "tools.pin.session.set";
pub const TOOL_PIN_SESSION_CLEAR_METHOD_V1: &str = "tools.pin.session.clear";
pub const TOOL_ACTIVITY_PAGE_METHOD_V1: &str = "tools.activity.page";
pub const TOOL_CREDENTIAL_CANDIDATES_METHOD_V1: &str = "tools.credential.candidates";

/// The closed tool settings mutations. The P07 Draft, package and
/// long-operation commands are retired; their request types stay only as historical contracts.
pub const TOOL_MUTATION_METHODS_V1: [&str; 8] = [
    TOOL_SET_ENABLED_METHOD_V1,
    TOOL_INSTANCE_UPDATE_CONFIGURATION_METHOD_V1,
    TOOL_INSTANCE_BIND_CREDENTIAL_METHOD_V1,
    TOOL_INSTANCE_CLEAR_CREDENTIAL_METHOD_V1,
    TOOL_PIN_WORKSPACE_SET_METHOD_V1,
    TOOL_PIN_WORKSPACE_CLEAR_METHOD_V1,
    TOOL_PIN_SESSION_SET_METHOD_V1,
    TOOL_PIN_SESSION_CLEAR_METHOD_V1,
];
pub const TOOL_READ_METHODS_V1: [&str; 2] =
    [TOOL_ACTIVITY_PAGE_METHOD_V1, TOOL_CREDENTIAL_CANDIDATES_METHOD_V1];

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftCreateRequestV1 {}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftImportDirectoryRequestV1 {
    pub directory: FileTargetRef,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "source_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolDraftArchiveSourceV1 {
    FileTarget { archive: FileTargetRef },
    ControlledArtifact { artifact_id: ArtifactId },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftImportArchiveRequestV1 {
    pub source: ToolDraftArchiveSourceV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftReplaceSnapshotRequestV1 {
    pub tool_draft_id: ToolDraftId,
    pub expected_draft_revision: Revision,
    pub snapshot_artifact: ToolDraftTransferArtifactRefV1,
    pub expected_snapshot_hash: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftValidateRequestV1 {
    pub tool_draft_id: ToolDraftId,
    pub expected_entity_revision: Revision,
    pub expected_snapshot_hash: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftStartConformanceRequestV1 {
    pub tool_draft_id: ToolDraftId,
    pub expected_entity_revision: Revision,
    pub expected_snapshot_hash: CanonicalSha256HashV1,
    pub tool_key: ToolKey,
    pub conformance_case: ToolPackageConformanceCaseNameV1,
    pub instance_configuration: Option<Value>,
}

pub type ToolDraftPreviewRequestV1 = ToolDraftValidateRequestV1;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftCancelConformanceRequestV1 {
    pub tool_conformance_run_id: ToolConformanceRunId,
    pub expected_operational_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftApproveRequestV1 {
    pub tool_draft_id: ToolDraftId,
    pub expected_entity_revision: Revision,
    pub expected_snapshot_hash: CanonicalSha256HashV1,
    pub expected_preview_hash: CanonicalSha256HashV1,
    pub expected_fingerprint: PackageFingerprintV1,
    pub expected_security_fingerprint: PackageSecurityFingerprintV1,
    pub trust_after_install: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftRejectRequestV1 {
    pub tool_draft_id: ToolDraftId,
    pub expected_entity_revision: Revision,
    pub expected_snapshot_hash: CanonicalSha256HashV1,
    pub expected_preview_hash: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftApplyRequestV1 {
    pub tool_draft_id: ToolDraftId,
    pub expected_entity_revision: Revision,
    pub expected_snapshot_hash: CanonicalSha256HashV1,
    pub expected_preview_hash: CanonicalSha256HashV1,
    pub expected_fingerprint: PackageFingerprintV1,
    pub expected_security_fingerprint: PackageSecurityFingerprintV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageRequestDeleteRequestV1 {
    pub tool_package_id: ToolPackageId,
    pub expected_entity_revision: Revision,
    pub expected_fingerprint: PackageFingerprintV1,
    pub expected_security_fingerprint: PackageSecurityFingerprintV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageApproveDeleteRequestV1 {
    pub tool_draft_id: ToolDraftId,
    pub expected_entity_revision: Revision,
    pub expected_snapshot_hash: CanonicalSha256HashV1,
    pub expected_preview_hash: CanonicalSha256HashV1,
    pub expected_persistent_impact_hash: CanonicalSha256HashV1,
}

pub type ToolPackageRejectDeleteRequestV1 = ToolPackageApproveDeleteRequestV1;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageSetTrustRequestV1 {
    pub tool_package_id: ToolPackageId,
    pub trusted: bool,
    pub expected_entity_revision: Revision,
    pub expected_fingerprint: PackageSecurityFingerprintV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageSetEnabledRequestV1 {
    pub tool_package_id: ToolPackageId,
    pub enabled: bool,
    pub expected_entity_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSetEnabledRequestV1 {
    pub tool_id: ToolId,
    pub enabled: bool,
    pub expected_entity_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstanceUpdateConfigurationRequestV1 {
    pub tool_instance_id: ToolInstanceId,
    pub configuration: Value,
    pub expected_entity_revision: Revision,
    pub expected_configuration_revision: Option<Revision>,
    pub expected_schema_hash: CanonicalHashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstanceBindCredentialRequestV1 {
    pub tool_instance_id: ToolInstanceId,
    pub credential_slot_key: CredentialSlotKey,
    pub placement: crate::ToolCredentialPlacementV1,
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub workspace_id: Option<WorkspaceId>,
    pub session_id: Option<SessionId>,
    pub expected_candidate_set_revision: Revision,
    pub expected_entity_revision: Revision,
    pub expected_binding_revision: Option<Revision>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialCandidatesRequestV1 {
    pub tool_instance_id: ToolInstanceId,
    pub credential_slot_key: CredentialSlotKey,
    pub placement: crate::ToolCredentialPlacementV1,
    pub workspace_id: Option<WorkspaceId>,
    pub session_id: Option<SessionId>,
    pub expected_instance_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstanceClearCredentialRequestV1 {
    pub tool_instance_id: ToolInstanceId,
    pub expected_entity_revision: Revision,
    pub expected_binding_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceToolPinSetRequestV1 {
    pub workspace_id: WorkspaceId,
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub expected_entity_revision: Option<Revision>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceToolPinClearRequestV1 {
    pub workspace_id: WorkspaceId,
    pub tool_id: ToolId,
    pub expected_entity_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionToolPinSetRequestV1 {
    pub session_id: SessionId,
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub expected_entity_revision: Option<Revision>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionToolPinClearRequestV1 {
    pub session_id: SessionId,
    pub tool_id: ToolId,
    pub expected_entity_revision: Revision,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolLongOperationStateV1 {
    Queued,
    Running,
    Cancelling,
    Cancelled,
    Succeeded,
    Failed,
    RecoveryRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "operation_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolLongOperationRefV1 {
    DraftApply { tool_draft_id: ToolDraftId, journal_id: MutationJournalId },
    PackageDelete { tool_package_id: ToolPackageId, journal_id: MutationJournalId },
}

impl ToolLongOperationRefV1 {
    #[must_use]
    pub const fn journal_id(&self) -> MutationJournalId {
        match self {
            Self::DraftApply { journal_id, .. } | Self::PackageDelete { journal_id, .. } => {
                *journal_id
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolLongOperationAcceptedV1 {
    pub operation: ToolLongOperationRefV1,
    pub state: ToolLongOperationStateV1,
    pub operational_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "operation_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolOperationRefV1 {
    Conformance { tool_conformance_run_id: ToolConformanceRunId },
    DraftApply { tool_draft_id: ToolDraftId, journal_id: MutationJournalId },
    PackageDelete { tool_package_id: ToolPackageId, journal_id: MutationJournalId },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolOperationCancelRequestV1 {
    pub operation: ToolOperationRefV1,
    pub expected_operational_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "result_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolMutationResultV1 {
    DraftCommitted {
        tool_draft_id: ToolDraftId,
        draft_revision: Revision,
        snapshot_hash: CanonicalSha256HashV1,
        entity_revision: Revision,
    },
    ConformanceAccepted {
        tool_conformance_run_id: ToolConformanceRunId,
        operational_revision: Revision,
    },
    LongOperationAccepted {
        accepted: ToolLongOperationAcceptedV1,
    },
    PackageCommitted {
        tool_package_id: ToolPackageId,
        entity_revision: Revision,
    },
    ToolCommitted {
        tool_id: ToolId,
        entity_revision: Revision,
    },
    InstanceCommitted {
        tool_instance_id: ToolInstanceId,
        entity_revision: Revision,
    },
    WorkspacePinCommitted {
        workspace_id: WorkspaceId,
        entity_revision: Revision,
    },
    SessionPinCommitted {
        session_id: SessionId,
        entity_revision: Revision,
    },
    OperationCancelled {
        operation: ToolOperationRefV1,
        operational_revision: Revision,
    },
}

/// Schema aggregation only. Runtime IPC uses the individual method-specific
/// DTOs so no all-nullable mutation request can enter the authenticated path.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolMutationContractSchemaRootV1 {
    pub draft_create: ToolDraftCreateRequestV1,
    pub draft_import_directory: ToolDraftImportDirectoryRequestV1,
    pub draft_import_archive: ToolDraftImportArchiveRequestV1,
    pub draft_replace_snapshot: ToolDraftReplaceSnapshotRequestV1,
    pub draft_validate: ToolDraftValidateRequestV1,
    pub draft_start_conformance: ToolDraftStartConformanceRequestV1,
    pub draft_cancel_conformance: ToolDraftCancelConformanceRequestV1,
    pub draft_preview: ToolDraftPreviewRequestV1,
    pub draft_approve: ToolDraftApproveRequestV1,
    pub draft_reject: ToolDraftRejectRequestV1,
    pub draft_apply: ToolDraftApplyRequestV1,
    pub package_request_delete: ToolPackageRequestDeleteRequestV1,
    pub package_approve_delete: ToolPackageApproveDeleteRequestV1,
    pub package_reject_delete: ToolPackageRejectDeleteRequestV1,
    pub package_set_trust: ToolPackageSetTrustRequestV1,
    pub package_set_enabled: ToolPackageSetEnabledRequestV1,
    pub tool_set_enabled: ToolSetEnabledRequestV1,
    pub instance_update_configuration: ToolInstanceUpdateConfigurationRequestV1,
    pub instance_bind_credential: ToolInstanceBindCredentialRequestV1,
    pub instance_clear_credential: ToolInstanceClearCredentialRequestV1,
    pub workspace_pin_set: WorkspaceToolPinSetRequestV1,
    pub workspace_pin_clear: WorkspaceToolPinClearRequestV1,
    pub session_pin_set: SessionToolPinSetRequestV1,
    pub session_pin_clear: SessionToolPinClearRequestV1,
    pub operation_cancel: ToolOperationCancelRequestV1,
    pub mutation_result: ToolMutationResultV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolReadContractSchemaRootV1 {
    pub credential_candidates: ToolCredentialCandidatesRequestV1,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        TOOL_ACTIVITY_PAGE_METHOD_V1, TOOL_CREDENTIAL_CANDIDATES_METHOD_V1,
        TOOL_MUTATION_METHODS_V1, TOOL_READ_METHODS_V1,
    };

    #[test]
    fn public_tool_methods_are_unique_and_closed() {
        let unique = TOOL_MUTATION_METHODS_V1.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), TOOL_MUTATION_METHODS_V1.len());
        assert!(!unique.contains(TOOL_ACTIVITY_PAGE_METHOD_V1));
        assert_eq!(
            TOOL_READ_METHODS_V1,
            [TOOL_ACTIVITY_PAGE_METHOD_V1, TOOL_CREDENTIAL_CANDIDATES_METHOD_V1]
        );
    }
}
