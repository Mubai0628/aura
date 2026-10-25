use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{ContentHash, SecurityOperationId};
use aura_domain_common::{ToolConformanceRunId, ToolDraftId, ToolInstanceId};
use aura_security_contract::InvocationIdentity;
use aura_tool_contract::{
    PackageRuntimeClosureHashV1, ToolContractFingerprintV1, ToolDefinitionSnapshotRef, ToolKey,
    ToolPackageConformanceCaseNameV1, ToolSourceContractFingerprintV1, ToolSourceKindV1,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

/// Historical decoder contract. It is used only by the versioned P04 -> P07
/// migration and is never written by current code.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "source_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecuritySourceV1 {
    UserInterface,
    BuiltInTool { tool_id: String },
    CustomTool { package_id: String },
    Hook { hook_id: String },
    McpServer { server_id: String },
    Skill { skill_id: String },
    Plugin { plugin_id: String },
    SystemComponent { component_id: String },
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolSecuritySourceRefError {
    #[error("tool source kind and source contract fingerprint are inconsistent")]
    SourceFingerprintMismatch,
}

/// Exact authority identity for one uninstalled Draft/Quarantine Conformance
/// run. It cannot be interpreted as an installed Tool or Registry binding.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolConformanceSecuritySourceRefV1 {
    pub tool_conformance_run_id: ToolConformanceRunId,
    pub tool_draft_id: ToolDraftId,
    pub draft_revision: aura_domain_common::Revision,
    pub draft_snapshot_hash: CanonicalSha256HashV1,
    pub tool_key: ToolKey,
    pub conformance_case_name: ToolPackageConformanceCaseNameV1,
    pub canonical_manifest_hash: CanonicalSha256HashV1,
    pub runtime_closure_hash: PackageRuntimeClosureHashV1,
    pub tool_contract_fingerprint: ToolContractFingerprintV1,
    pub conformance_binding_hash: CanonicalSha256HashV1,
}

/// Exact Tool identity accepted by P04 for one concrete ToolCall. Production
/// construction is additionally gated by P07 Batch Admission against the
/// unique EffectiveToolBindingSnapshot in the same immutable `trs_`.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSecuritySourceRefV1 {
    pub definition_ref: ToolDefinitionSnapshotRef,
    pub tool_instance_id: ToolInstanceId,
    pub source_kind: ToolSourceKindV1,
    pub definition_fingerprint: aura_domain_common::jcs::CanonicalHashV1,
    pub source_contract_fingerprint: ToolSourceContractFingerprintV1,
}

impl ToolSecuritySourceRefV1 {
    pub fn new(
        definition_ref: ToolDefinitionSnapshotRef,
        tool_instance_id: ToolInstanceId,
        source_kind: ToolSourceKindV1,
        definition_fingerprint: aura_domain_common::jcs::CanonicalHashV1,
        source_contract_fingerprint: ToolSourceContractFingerprintV1,
    ) -> Result<Self, ToolSecuritySourceRefError> {
        let value = Self {
            definition_ref,
            tool_instance_id,
            source_kind,
            definition_fingerprint,
            source_contract_fingerprint,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ToolSecuritySourceRefError> {
        if self.source_contract_fingerprint.source_kind() != self.source_kind {
            return Err(ToolSecuritySourceRefError::SourceFingerprintMismatch);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ToolSecuritySourceRefV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireRef {
            definition_ref: ToolDefinitionSnapshotRef,
            tool_instance_id: ToolInstanceId,
            source_kind: ToolSourceKindV1,
            definition_fingerprint: aura_domain_common::jcs::CanonicalHashV1,
            source_contract_fingerprint: ToolSourceContractFingerprintV1,
        }

        let wire = WireRef::deserialize(deserializer)?;
        Self::new(
            wire.definition_ref,
            wire.tool_instance_id,
            wire.source_kind,
            wire.definition_fingerprint,
            wire.source_contract_fingerprint,
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "source_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecuritySourceV2 {
    UserInterface,
    Tool { tool: Box<ToolSecuritySourceRefV1> },
    ToolConformance { conformance: Box<ToolConformanceSecuritySourceRefV1> },
    Hook { hook_id: String },
    McpServer { server_id: String },
    Skill { skill_id: String },
    Plugin { plugin_id: String },
    SystemComponent { component_id: String },
}

/// Current security DTOs deliberately use the V2 wire. The V1 type remains a
/// separate migration-only decoder so old Tool strings are never accepted by
/// new command paths.
pub type SecuritySource = SecuritySourceV2;

impl SecuritySourceV2 {
    /// Core-owned projection, never a replacement for Tool Admission or the
    /// exact source revalidation immediately before dispatch.
    #[must_use]
    pub fn security_source_ref(&self) -> aura_security_contract::SecuritySourceRef {
        use aura_security_contract::SecuritySourceRef as Ref;
        match self {
            Self::UserInterface => Ref::UserInterface,
            Self::Tool { tool } => {
                let identity_hash = canonical_sha256(tool).expect("Tool source is canonical JSON");
                if matches!(tool.source_kind, ToolSourceKindV1::BuiltIn) {
                    Ref::BuiltInTool { identity_hash }
                } else {
                    Ref::Tool { identity_hash }
                }
            }
            Self::ToolConformance { conformance } => Ref::ToolConformance {
                identity_hash: canonical_sha256(conformance)
                    .expect("Conformance source is canonical JSON"),
            },
            Self::Hook { hook_id } => Ref::Hook { hook_id: hook_id.clone() },
            Self::McpServer { server_id } => Ref::McpServer { server_id: server_id.clone() },
            Self::Skill { skill_id } => Ref::Skill { skill_id: skill_id.clone() },
            Self::Plugin { plugin_id } => Ref::Plugin { plugin_id: plugin_id.clone() },
            Self::SystemComponent { component_id } => {
                Ref::SystemComponent { component_id: component_id.clone() }
            }
        }
    }

    #[must_use]
    pub fn stable_key(&self) -> String {
        self.security_source_ref().stable_key()
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperationSecurityPlan {
    pub operation_id: SecurityOperationId,
    pub identity: InvocationIdentity,
    pub source: SecuritySource,
    pub capabilities: Vec<aura_security_contract::ResolvedCapability>,
    pub scopes: Vec<crate::ResolvedScope>,
    pub risk_flags: Vec<aura_security_contract::OperationRiskFlag>,
    pub purpose: Option<String>,
    pub source_fingerprint: Option<ContentHash>,
    pub requested_sandbox: Option<crate::NativeSandboxMode>,
    pub network_access: crate::NetworkAccessRequirement,
    pub network_plan: Option<crate::NetworkRequestPlan>,
    pub operation_fingerprint: ContentHash,
}
