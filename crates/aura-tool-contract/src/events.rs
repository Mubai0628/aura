use aura_domain_common::{
    ArtifactId, CredentialId, CredentialRevision, ErrorEnvelope, Revision, SessionId, UtcTimestamp,
    WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    PackageSecurityFingerprintV1, ToolCancellationReasonV1, ToolIndeterminateReasonV1,
    ToolPackageTombstoneV1, ToolPackageToolIdentityV1, ToolPackageVersionRefV1, ToolPinV1,
    ToolRegistryScopeV1, ToolRegistrySnapshotRef, ToolResultV1, ToolTombstoneV1,
};
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ToolBatchId, ToolCallId, ToolExecutionAttemptId, ToolInstanceId, ToolPackageId,
};
use aura_execution_contract::SideEffectStateV1;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageInstalledPayloadV1 {
    pub package: ToolPackageVersionRefV1,
    pub tool_identities: Vec<ToolPackageToolIdentityV1>,
    pub manifest_hash: CanonicalHashV1,
    pub installed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageUpdatedPayloadV1 {
    pub previous_package: ToolPackageVersionRefV1,
    pub package: ToolPackageVersionRefV1,
    pub active_tool_identities: Vec<ToolPackageToolIdentityV1>,
    pub retired_tools: Vec<ToolTombstoneV1>,
    pub manifest_hash: CanonicalHashV1,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageDeletedPayloadV1 {
    pub package_tombstone: ToolPackageTombstoneV1,
    pub tool_tombstones: Vec<ToolTombstoneV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolTrustChangedPayloadV1 {
    pub tool_package_id: ToolPackageId,
    pub package_security_fingerprint: PackageSecurityFingerprintV1,
    pub trusted: bool,
    pub entity_revision: Revision,
    pub changed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolEnabledChangedPayloadV1 {
    pub tool_instance_id: ToolInstanceId,
    pub enabled: bool,
    pub entity_revision: Revision,
    pub changed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstanceConfigurationChangedPayloadV1 {
    pub tool_instance_id: ToolInstanceId,
    pub configuration_revision: Revision,
    pub schema_hash: CanonicalHashV1,
    pub configuration_hash: CanonicalHashV1,
    pub entity_revision: Revision,
    pub changed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstanceCredentialBindingChangedPayloadV1 {
    pub tool_instance_id: ToolInstanceId,
    pub credential_id: Option<CredentialId>,
    pub credential_revision: Option<CredentialRevision>,
    pub binding_revision: Option<Revision>,
    pub entity_revision: Revision,
    pub changed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceToolPinChangedPayloadV1 {
    pub workspace_id: WorkspaceId,
    pub previous_pin: Option<ToolPinV1>,
    pub current_pin: Option<ToolPinV1>,
    pub entity_revision: Revision,
    pub changed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionToolPinChangedPayloadV1 {
    pub session_id: SessionId,
    pub previous_pin: Option<ToolPinV1>,
    pub current_pin: Option<ToolPinV1>,
    pub entity_revision: Revision,
    pub changed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRegistrySnapshotCreatedPayloadV1 {
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub scope: ToolRegistryScopeV1,
    pub tool_set_hash: CanonicalHashV1,
    pub binding_set_hash: CanonicalHashV1,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchAdmittedPayloadV1 {
    pub tool_batch_id: ToolBatchId,
    pub batch_content_hash: CanonicalHashV1,
    pub execution_plan_hash: CanonicalHashV1,
    pub admitted_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchAdmissionRejectedPayloadV1 {
    pub tool_batch_id: ToolBatchId,
    pub batch_content_hash: CanonicalHashV1,
    pub error: ErrorEnvelope,
    pub internal_diagnostic_ref: Option<ArtifactId>,
    pub rejected_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionAttemptStartedPayloadV1 {
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub tool_call_id: ToolCallId,
    pub tool_batch_id: ToolBatchId,
    pub attempt_number: u8,
    pub execution_plan_hash: CanonicalHashV1,
    pub started_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionAttemptDispatchPreparedPayloadV1 {
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub dispatch_evidence_hash: CanonicalHashV1,
    pub prepared_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionAttemptCancellationRequestedPayloadV1 {
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub reason: ToolCancellationReasonV1,
    pub requested_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionAttemptSucceededPayloadV1 {
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub side_effect_state: SideEffectStateV1,
    pub evidence_ref: ArtifactId,
    pub completed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionAttemptFailedPayloadV1 {
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub side_effect_state: SideEffectStateV1,
    pub error: ErrorEnvelope,
    pub evidence_ref: ArtifactId,
    pub completed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionAttemptCancelledPayloadV1 {
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub reason: ToolCancellationReasonV1,
    pub side_effect_state: SideEffectStateV1,
    pub evidence_ref: ArtifactId,
    pub completed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionAttemptIndeterminatePayloadV1 {
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub reason: ToolIndeterminateReasonV1,
    pub side_effect_state: SideEffectStateV1,
    pub error: ErrorEnvelope,
    pub evidence_ref: ArtifactId,
    pub completed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolResultFinalizedPayloadV1 {
    pub tool_batch_id: ToolBatchId,
    pub result: ToolResultV1,
}

/// Closed catalog of P07 lifecycle events. Event Store metadata (EventId, actor,
/// causation and correlation) remains in the shared persistence envelope.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "event_type", content = "payload")]
pub enum ToolLifecycleEventV1 {
    #[serde(rename = "ToolPackageInstalled")]
    ToolPackageInstalled(ToolPackageInstalledPayloadV1),
    #[serde(rename = "ToolPackageUpdated")]
    ToolPackageUpdated(ToolPackageUpdatedPayloadV1),
    #[serde(rename = "ToolPackageDeleted")]
    ToolPackageDeleted(ToolPackageDeletedPayloadV1),
    #[serde(rename = "ToolTrustChanged")]
    ToolTrustChanged(ToolTrustChangedPayloadV1),
    #[serde(rename = "ToolEnabledChanged")]
    ToolEnabledChanged(ToolEnabledChangedPayloadV1),
    #[serde(rename = "ToolInstanceConfigurationChanged")]
    ToolInstanceConfigurationChanged(ToolInstanceConfigurationChangedPayloadV1),
    #[serde(rename = "ToolInstanceCredentialBindingChanged")]
    ToolInstanceCredentialBindingChanged(ToolInstanceCredentialBindingChangedPayloadV1),
    #[serde(rename = "WorkspaceToolPinChanged")]
    WorkspaceToolPinChanged(WorkspaceToolPinChangedPayloadV1),
    #[serde(rename = "SessionToolPinChanged")]
    SessionToolPinChanged(SessionToolPinChangedPayloadV1),
    #[serde(rename = "ToolRegistrySnapshotCreated")]
    ToolRegistrySnapshotCreated(ToolRegistrySnapshotCreatedPayloadV1),
    #[serde(rename = "ToolBatchAdmitted")]
    ToolBatchAdmitted(ToolBatchAdmittedPayloadV1),
    #[serde(rename = "ToolBatchAdmissionRejected")]
    ToolBatchAdmissionRejected(ToolBatchAdmissionRejectedPayloadV1),
    #[serde(rename = "ToolExecutionAttemptStarted")]
    ToolExecutionAttemptStarted(ToolExecutionAttemptStartedPayloadV1),
    #[serde(rename = "ToolExecutionAttemptDispatchPrepared")]
    ToolExecutionAttemptDispatchPrepared(ToolExecutionAttemptDispatchPreparedPayloadV1),
    #[serde(rename = "ToolExecutionAttemptCancellationRequested")]
    ToolExecutionAttemptCancellationRequested(ToolExecutionAttemptCancellationRequestedPayloadV1),
    #[serde(rename = "ToolExecutionAttemptSucceeded")]
    ToolExecutionAttemptSucceeded(ToolExecutionAttemptSucceededPayloadV1),
    #[serde(rename = "ToolExecutionAttemptFailed")]
    ToolExecutionAttemptFailed(ToolExecutionAttemptFailedPayloadV1),
    #[serde(rename = "ToolExecutionAttemptCancelled")]
    ToolExecutionAttemptCancelled(ToolExecutionAttemptCancelledPayloadV1),
    #[serde(rename = "ToolExecutionAttemptIndeterminate")]
    ToolExecutionAttemptIndeterminate(ToolExecutionAttemptIndeterminatePayloadV1),
    #[serde(rename = "ToolResultFinalized")]
    ToolResultFinalized(ToolResultFinalizedPayloadV1),
}

impl ToolLifecycleEventV1 {
    #[must_use]
    pub const fn event_type(&self) -> &'static str {
        match self {
            Self::ToolPackageInstalled(_) => "ToolPackageInstalled",
            Self::ToolPackageUpdated(_) => "ToolPackageUpdated",
            Self::ToolPackageDeleted(_) => "ToolPackageDeleted",
            Self::ToolTrustChanged(_) => "ToolTrustChanged",
            Self::ToolEnabledChanged(_) => "ToolEnabledChanged",
            Self::ToolInstanceConfigurationChanged(_) => "ToolInstanceConfigurationChanged",
            Self::ToolInstanceCredentialBindingChanged(_) => "ToolInstanceCredentialBindingChanged",
            Self::WorkspaceToolPinChanged(_) => "WorkspaceToolPinChanged",
            Self::SessionToolPinChanged(_) => "SessionToolPinChanged",
            Self::ToolRegistrySnapshotCreated(_) => "ToolRegistrySnapshotCreated",
            Self::ToolBatchAdmitted(_) => "ToolBatchAdmitted",
            Self::ToolBatchAdmissionRejected(_) => "ToolBatchAdmissionRejected",
            Self::ToolExecutionAttemptStarted(_) => "ToolExecutionAttemptStarted",
            Self::ToolExecutionAttemptDispatchPrepared(_) => "ToolExecutionAttemptDispatchPrepared",
            Self::ToolExecutionAttemptCancellationRequested(_) => {
                "ToolExecutionAttemptCancellationRequested"
            }
            Self::ToolExecutionAttemptSucceeded(_) => "ToolExecutionAttemptSucceeded",
            Self::ToolExecutionAttemptFailed(_) => "ToolExecutionAttemptFailed",
            Self::ToolExecutionAttemptCancelled(_) => "ToolExecutionAttemptCancelled",
            Self::ToolExecutionAttemptIndeterminate(_) => "ToolExecutionAttemptIndeterminate",
            Self::ToolResultFinalized(_) => "ToolResultFinalized",
        }
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{Revision, UtcTimestamp, WorkspaceId};
    use serde_json::json;

    use super::{ToolLifecycleEventV1, WorkspaceToolPinChangedPayloadV1};

    #[test]
    fn event_wire_uses_frozen_event_store_name() {
        let event =
            ToolLifecycleEventV1::WorkspaceToolPinChanged(WorkspaceToolPinChangedPayloadV1 {
                workspace_id: WorkspaceId::new(),
                previous_pin: None,
                current_pin: None,
                entity_revision: Revision::INITIAL,
                changed_at: UtcTimestamp::now(),
            });
        let wire = serde_json::to_value(&event).expect("event wire");
        assert_eq!(wire["event_type"], json!("WorkspaceToolPinChanged"));
        assert_eq!(event.event_type(), "WorkspaceToolPinChanged");
    }
}
