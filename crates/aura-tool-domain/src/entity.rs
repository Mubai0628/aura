use aura_domain_common::CanonicalSemVer;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{Revision, SessionId, TurnTaskId, UtcTimestamp, WorkspaceId};
use aura_domain_common::{
    ToolConformanceRunId, ToolDraftId, ToolId, ToolInstanceId, ToolPackageId,
};
use aura_tool_contract::{
    PackageFingerprintV1, PackageKey, PackageRelativePath, PackageRuntimeClosureHashV1,
    PackageSecurityFingerprintV1, ToolKey, ToolPinV1, ToolSourceContractFingerprintV1,
    ToolSourceKindV1, ToolVersion,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use thiserror::Error;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolPackageLifecycleV1 {
    Installed,
    Corrupted,
    Deleted,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageRecordV1 {
    pub tool_package_id: ToolPackageId,
    pub package_key: PackageKey,
    pub active_version: Option<CanonicalSemVer>,
    pub lifecycle: ToolPackageLifecycleV1,
    /// Set in the same transaction that consumes a Package deletion approval
    /// and creates the single corresponding Delete Journal.
    pub deletion_pending: bool,
    pub entity_revision: Revision,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageVersionRecordV1 {
    pub tool_package_id: ToolPackageId,
    pub package_version: CanonicalSemVer,
    pub package_fingerprint: PackageFingerprintV1,
    pub package_security_fingerprint: PackageSecurityFingerprintV1,
    pub manifest_hash: CanonicalHashV1,
    /// Canonical relative location beneath the immutable Package Store root.
    pub package_store_relative_path: PackageRelativePath,
    /// Every Tool/case Conformance result approved for this exact immutable
    /// Package version, sorted by ID and never borrowed by a later Draft.
    pub conformance_run_ids: Vec<ToolConformanceRunId>,
    pub installed_at: UtcTimestamp,
    pub payload_strong_reference_count: u64,
    pub payload_gc_eligible_at: Option<UtcTimestamp>,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolRuntimeHealthV1 {
    Healthy,
    Unavailable,
    Corrupted,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstanceOverridesV1 {
    pub timeout_ms: Option<u64>,
    pub max_retries: Option<u8>,
    pub max_concurrent_attempts: Option<u32>,
    // UNWIRED: every writer sets None and no subagent path reads it.
    pub subagent_enabled: Option<bool>,
}

impl ToolInstanceOverridesV1 {
    pub fn validate(&self) -> Result<(), ToolEntityError> {
        if self.timeout_ms.is_some_and(|value| {
            !(aura_tool_contract::MIN_TOOL_TIMEOUT_MS..=aura_tool_contract::MAX_TOOL_TIMEOUT_MS)
                .contains(&value)
        }) || self.max_retries.is_some_and(|value| value > aura_tool_contract::MAX_TOOL_RETRIES)
            || self.max_concurrent_attempts.is_some_and(|value| {
                !(aura_tool_contract::MIN_MAX_CONCURRENT_TOOL_CALLS
                    ..=aura_tool_contract::MAX_MAX_CONCURRENT_TOOL_CALLS)
                    .contains(&value)
            })
        {
            return Err(ToolEntityError::InvalidOverride);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstanceRecordV1 {
    pub tool_instance_id: ToolInstanceId,
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub source_kind: ToolSourceKindV1,
    pub source_contract_fingerprint: ToolSourceContractFingerprintV1,
    pub enabled: bool,
    pub runtime_health: ToolRuntimeHealthV1,
    pub overrides: ToolInstanceOverridesV1,
    pub entity_revision: Revision,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstanceConfigurationRecordV1 {
    pub tool_instance_id: ToolInstanceId,
    pub configuration_revision: Revision,
    pub instance_config_schema_hash: CanonicalHashV1,
    pub configuration: Value,
    pub configuration_hash: CanonicalHashV1,
    /// Evidence from the trusted Core scanner. No candidate value may be
    /// persisted through this boundary without a successful scan.
    pub secret_scan_evidence_hash: CanonicalHashV1,
    pub updated_at: UtcTimestamp,
}

impl ToolInstanceConfigurationRecordV1 {
    pub fn new(
        tool_instance_id: ToolInstanceId,
        configuration_revision: Revision,
        instance_config_schema_hash: CanonicalHashV1,
        configuration: Value,
        secret_scan_evidence_hash: CanonicalHashV1,
        updated_at: UtcTimestamp,
    ) -> Result<Self, ToolEntityError> {
        if !configuration.is_object() || configuration_revision.value() == 0 {
            return Err(ToolEntityError::InvalidInstanceConfiguration);
        }
        let configuration_hash = aura_domain_common::jcs::canonical_sha256(&configuration)
            .map_err(|_| ToolEntityError::InvalidInstanceConfiguration)?;
        Ok(Self {
            tool_instance_id,
            configuration_revision,
            instance_config_schema_hash,
            configuration,
            configuration_hash,
            secret_scan_evidence_hash,
            updated_at,
        })
    }

    pub fn validate(&self) -> Result<(), ToolEntityError> {
        if !self.configuration.is_object()
            || self.configuration_revision.value() == 0
            || aura_domain_common::jcs::canonical_sha256(&self.configuration)
                .map_err(|_| ToolEntityError::InvalidInstanceConfiguration)?
                != self.configuration_hash
        {
            return Err(ToolEntityError::InvalidInstanceConfiguration);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ToolInstanceConfigurationRecordV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireRecord {
            tool_instance_id: ToolInstanceId,
            configuration_revision: Revision,
            instance_config_schema_hash: CanonicalHashV1,
            configuration: Value,
            configuration_hash: CanonicalHashV1,
            secret_scan_evidence_hash: CanonicalHashV1,
            updated_at: UtcTimestamp,
        }
        let wire = WireRecord::deserialize(deserializer)?;
        let record = Self {
            tool_instance_id: wire.tool_instance_id,
            configuration_revision: wire.configuration_revision,
            instance_config_schema_hash: wire.instance_config_schema_hash,
            configuration: wire.configuration,
            configuration_hash: wire.configuration_hash,
            secret_scan_evidence_hash: wire.secret_scan_evidence_hash,
            updated_at: wire.updated_at,
        };
        record.validate().map_err(D::Error::custom)?;
        Ok(record)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageTrustRecordV1 {
    pub tool_package_id: ToolPackageId,
    pub package_security_fingerprint: PackageSecurityFingerprintV1,
    pub trusted: bool,
    pub entity_revision: Revision,
    pub reviewed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolPinRecordV1 {
    Workspace {
        workspace_id: WorkspaceId,
        pin: ToolPinV1,
        entity_revision: Revision,
        updated_at: UtcTimestamp,
    },
    Session {
        session_id: SessionId,
        workspace_id: WorkspaceId,
        pin: ToolPinV1,
        entity_revision: Revision,
        updated_at: UtcTimestamp,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolEphemeralLoadRecordV1 {
    pub turn_task_id: TurnTaskId,
    pub session_id: SessionId,
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub entity_revision: Revision,
    pub loaded_at: UtcTimestamp,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolDraftStatusV1 {
    Editing,
    Validating,
    ValidationFailed,
    ReadyForTest,
    Testing,
    TestFailed,
    ReadyToApply,
    Applying,
    Applied,
    Discarded,
    RecoveryRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftRecordV1 {
    pub tool_draft_id: ToolDraftId,
    pub kind: crate::ToolDraftKindV1,
    pub status: ToolDraftStatusV1,
    pub current_draft_revision: Revision,
    pub draft_hash: aura_domain_common::jcs::CanonicalSha256HashV1,
    pub entity_revision: Revision,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolConformanceStatusV1 {
    Queued,
    Running,
    Passed,
    Failed,
    Cancelled,
    RecoveryRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolConformanceRunRecordV1 {
    pub tool_conformance_run_id: ToolConformanceRunId,
    pub draft: crate::ConformanceDraftRevisionRefV1,
    pub tool_key: ToolKey,
    pub conformance_case: crate::ToolPackageConformanceCaseNameV1,
    pub canonical_manifest_hash: aura_domain_common::jcs::CanonicalSha256HashV1,
    pub runtime_closure_hash: PackageRuntimeClosureHashV1,
    pub package_fingerprint: PackageFingerprintV1,
    pub binding_hash: aura_domain_common::jcs::CanonicalSha256HashV1,
    pub status: ToolConformanceStatusV1,
    pub evidence: Option<crate::ConformanceCaseEvidenceV1>,
    pub entity_revision: Revision,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolEntityError {
    #[error("tool instance override is outside the Core safety bounds")]
    InvalidOverride,
    #[error("tool instance configuration evidence is invalid")]
    InvalidInstanceConfiguration,
}
