use std::collections::BTreeSet;

use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{Revision, UtcTimestamp};
use aura_domain_common::{ToolId, ToolPackageId};
use aura_security_contract::ResolvedCapability;
use aura_tool_contract::{
    PackageKey, ResourceClaimV1, ToolAudienceV1, ToolCanonicalName, ToolInputSchemaV1, ToolKey,
    ToolResultContractV1, ToolRuntimeDescriptorV1, ToolSourceKindV1, ToolVersion,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionSemanticsV1 {
    /// The definition explicitly permits an automatic retry after all Core
    /// safety gates have also passed.
    pub automatic_retry_allowed: bool,
    /// Repeating the same normalized invocation is declared idempotent.
    pub repeat_is_idempotent: bool,
    /// The runtime can create a persistent effect outside its attempt-local
    /// staging area. Actual SideEffectState remains Core-derived evidence.
    pub may_create_persistent_effects: bool,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolDefinitionError {
    #[error("tool definition model-visible text must not be blank")]
    BlankModelText,
    #[error("tool definition capability list must be canonically sorted and unique")]
    CapabilityOrder,
    #[error("tool definition contains duplicate resource claims")]
    DuplicateResourceClaim,
    #[error("tool definition package identity is incomplete or inconsistent")]
    PackageIdentity,
    #[error("tool definition fingerprint is inconsistent")]
    FingerprintMismatch,
    #[error("tool definition revision must start at one")]
    InvalidEntityRevision,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinitionRecordV1 {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub tool_package_id: Option<ToolPackageId>,
    pub package_key: Option<PackageKey>,
    pub tool_key: Option<ToolKey>,
    pub canonical_name: ToolCanonicalName,
    pub display_name: String,
    pub model_description: String,
    pub model_summary: String,
    pub input_schema: ToolInputSchemaV1,
    pub strict: bool,
    pub result_contract: ToolResultContractV1,
    pub runtime: ToolRuntimeDescriptorV1,
    pub capabilities: Vec<ResolvedCapability>,
    pub resource_claims: Vec<ResourceClaimV1>,
    pub audience: ToolAudienceV1,
    pub source_kind: ToolSourceKindV1,
    pub execution_semantics: ToolExecutionSemanticsV1,
    pub runtime_file_closure_hash: CanonicalHashV1,
    pub definition_fingerprint: CanonicalHashV1,
    pub entity_revision: Revision,
    pub created_at: UtcTimestamp,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct DefinitionFingerprintMaterial<'a> {
    tool_id: ToolId,
    tool_version: &'a ToolVersion,
    tool_package_id: Option<ToolPackageId>,
    package_key: &'a Option<PackageKey>,
    tool_key: &'a Option<ToolKey>,
    canonical_name: &'a ToolCanonicalName,
    model_description: &'a str,
    model_summary: &'a str,
    input_schema: &'a ToolInputSchemaV1,
    strict: bool,
    result_contract: &'a ToolResultContractV1,
    runtime: &'a ToolRuntimeDescriptorV1,
    capabilities: &'a [ResolvedCapability],
    resource_claims: &'a [ResourceClaimV1],
    audience: ToolAudienceV1,
    source_kind: ToolSourceKindV1,
    execution_semantics: ToolExecutionSemanticsV1,
    runtime_file_closure_hash: &'a CanonicalHashV1,
}

impl ToolDefinitionRecordV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tool_id: ToolId,
        tool_version: ToolVersion,
        tool_package_id: Option<ToolPackageId>,
        package_key: Option<PackageKey>,
        tool_key: Option<ToolKey>,
        canonical_name: ToolCanonicalName,
        display_name: String,
        model_description: String,
        model_summary: String,
        input_schema: ToolInputSchemaV1,
        strict: bool,
        result_contract: ToolResultContractV1,
        runtime: ToolRuntimeDescriptorV1,
        capabilities: Vec<ResolvedCapability>,
        resource_claims: Vec<ResourceClaimV1>,
        audience: ToolAudienceV1,
        source_kind: ToolSourceKindV1,
        execution_semantics: ToolExecutionSemanticsV1,
        runtime_file_closure_hash: CanonicalHashV1,
        entity_revision: Revision,
        created_at: UtcTimestamp,
    ) -> Result<Self, ToolDefinitionError> {
        let mut record = Self {
            tool_id,
            tool_version,
            tool_package_id,
            package_key,
            tool_key,
            canonical_name,
            display_name,
            model_description,
            model_summary,
            input_schema,
            strict,
            result_contract,
            runtime,
            capabilities,
            resource_claims,
            audience,
            source_kind,
            execution_semantics,
            runtime_file_closure_hash,
            definition_fingerprint: CanonicalHashV1::sha256(&[]),
            entity_revision,
            created_at,
        };
        record.validate_without_hash()?;
        record.definition_fingerprint = record.compute_fingerprint()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), ToolDefinitionError> {
        self.validate_without_hash()?;
        if self.compute_fingerprint()? != self.definition_fingerprint {
            return Err(ToolDefinitionError::FingerprintMismatch);
        }
        Ok(())
    }

    fn validate_without_hash(&self) -> Result<(), ToolDefinitionError> {
        if self.model_description.trim().is_empty() || self.model_summary.trim().is_empty() {
            return Err(ToolDefinitionError::BlankModelText);
        }
        if self.entity_revision.value() == 0 {
            return Err(ToolDefinitionError::InvalidEntityRevision);
        }
        let package_fields =
            [self.tool_package_id.is_some(), self.package_key.is_some(), self.tool_key.is_some()];
        if package_fields.iter().any(|present| *present)
            && !package_fields.iter().all(|present| *present)
        {
            return Err(ToolDefinitionError::PackageIdentity);
        }
        if !self.capabilities.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(ToolDefinitionError::CapabilityOrder);
        }
        let mut claim_hashes = BTreeSet::new();
        for claim in &self.resource_claims {
            let hash =
                canonical_sha256(claim).map_err(|_| ToolDefinitionError::DuplicateResourceClaim)?;
            if !claim_hashes.insert(hash.hex().to_owned()) {
                return Err(ToolDefinitionError::DuplicateResourceClaim);
            }
        }
        Ok(())
    }

    fn compute_fingerprint(&self) -> Result<CanonicalHashV1, ToolDefinitionError> {
        canonical_sha256(&DefinitionFingerprintMaterial {
            tool_id: self.tool_id,
            tool_version: &self.tool_version,
            tool_package_id: self.tool_package_id,
            package_key: &self.package_key,
            tool_key: &self.tool_key,
            canonical_name: &self.canonical_name,
            model_description: &self.model_description,
            model_summary: &self.model_summary,
            input_schema: &self.input_schema,
            strict: self.strict,
            result_contract: &self.result_contract,
            runtime: &self.runtime,
            capabilities: &self.capabilities,
            resource_claims: &self.resource_claims,
            audience: self.audience,
            source_kind: self.source_kind,
            execution_semantics: self.execution_semantics,
            runtime_file_closure_hash: &self.runtime_file_closure_hash,
        })
        .map_err(|_| ToolDefinitionError::FingerprintMismatch)
    }
}

impl<'de> Deserialize<'de> for ToolDefinitionRecordV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireRecord {
            tool_id: ToolId,
            tool_version: ToolVersion,
            tool_package_id: Option<ToolPackageId>,
            package_key: Option<PackageKey>,
            tool_key: Option<ToolKey>,
            canonical_name: ToolCanonicalName,
            display_name: String,
            model_description: String,
            model_summary: String,
            input_schema: ToolInputSchemaV1,
            strict: bool,
            result_contract: ToolResultContractV1,
            runtime: ToolRuntimeDescriptorV1,
            capabilities: Vec<ResolvedCapability>,
            resource_claims: Vec<ResourceClaimV1>,
            audience: ToolAudienceV1,
            source_kind: ToolSourceKindV1,
            execution_semantics: ToolExecutionSemanticsV1,
            runtime_file_closure_hash: CanonicalHashV1,
            definition_fingerprint: CanonicalHashV1,
            entity_revision: Revision,
            created_at: UtcTimestamp,
        }

        let wire = WireRecord::deserialize(deserializer)?;
        let record = Self {
            tool_id: wire.tool_id,
            tool_version: wire.tool_version,
            tool_package_id: wire.tool_package_id,
            package_key: wire.package_key,
            tool_key: wire.tool_key,
            canonical_name: wire.canonical_name,
            display_name: wire.display_name,
            model_description: wire.model_description,
            model_summary: wire.model_summary,
            input_schema: wire.input_schema,
            strict: wire.strict,
            result_contract: wire.result_contract,
            runtime: wire.runtime,
            capabilities: wire.capabilities,
            resource_claims: wire.resource_claims,
            audience: wire.audience,
            source_kind: wire.source_kind,
            execution_semantics: wire.execution_semantics,
            runtime_file_closure_hash: wire.runtime_file_closure_hash,
            definition_fingerprint: wire.definition_fingerprint,
            entity_revision: wire.entity_revision,
            created_at: wire.created_at,
        };
        record.validate().map_err(D::Error::custom)?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::ToolId;
    use aura_domain_common::jcs::CanonicalHashV1;
    use aura_domain_common::{Revision, UtcTimestamp};
    use aura_security_contract::ResolvedCapability;
    use aura_tool_contract::{
        ResourceClaimSelectorV1, ResourceClaimV1, ResourceInvocationContextV1, ToolAudienceV1,
        ToolCanonicalName, ToolInputSchemaV1, ToolResultContractV1, ToolRuntimeDescriptorV1,
        ToolSourceKindV1, ToolVersion,
    };
    use serde_json::json;

    use super::{ToolDefinitionError, ToolDefinitionRecordV1, ToolExecutionSemanticsV1};

    fn definition(
        display_name: &str,
        model_description: &str,
        runtime_file_closure_hash: CanonicalHashV1,
    ) -> ToolDefinitionRecordV1 {
        ToolDefinitionRecordV1::new(
            ToolId::from_bytes([1; 16]),
            ToolVersion::new(None, Revision::INITIAL).expect("tool version"),
            None,
            None,
            None,
            ToolCanonicalName::new("context.search").expect("canonical name"),
            display_name.to_owned(),
            model_description.to_owned(),
            "Search the visible context".to_owned(),
            ToolInputSchemaV1::new(json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"],
                "additionalProperties": false
            }))
            .expect("input schema"),
            true,
            ToolResultContractV1::Unit,
            ToolRuntimeDescriptorV1::ExternalSource {
                source_contract_key: "signed_builtin.context.search.v1".to_owned(),
            },
            vec![],
            vec![],
            ToolAudienceV1::SubagentConfigurable,
            ToolSourceKindV1::BuiltIn,
            ToolExecutionSemanticsV1 {
                automatic_retry_allowed: false,
                repeat_is_idempotent: true,
                may_create_persistent_effects: false,
            },
            runtime_file_closure_hash,
            Revision::INITIAL,
            UtcTimestamp::now(),
        )
        .expect("definition")
    }

    #[test]
    fn ui_only_display_name_does_not_change_definition_fingerprint() {
        let runtime_hash = CanonicalHashV1::sha256(b"runtime-v1");
        let first = definition("Context search", "Search session context", runtime_hash.clone());
        let renamed = definition("上下文搜索", "Search session context", runtime_hash);

        assert_eq!(first.definition_fingerprint, renamed.definition_fingerprint);
    }

    #[test]
    fn model_contract_or_runtime_closure_changes_definition_fingerprint() {
        let first = definition(
            "Context search",
            "Search session context",
            CanonicalHashV1::sha256(b"runtime-v1"),
        );
        let description_changed = definition(
            "Context search",
            "Search visible session context",
            CanonicalHashV1::sha256(b"runtime-v1"),
        );
        let runtime_changed = definition(
            "Context search",
            "Search session context",
            CanonicalHashV1::sha256(b"runtime-v2"),
        );

        assert_ne!(first.definition_fingerprint, description_changed.definition_fingerprint);
        assert_ne!(first.definition_fingerprint, runtime_changed.definition_fingerprint);
    }

    #[test]
    fn capabilities_must_be_canonical_and_claims_unique() {
        let mut unordered = definition(
            "Context search",
            "Search session context",
            CanonicalHashV1::sha256(b"runtime-v1"),
        );
        unordered.capabilities =
            vec![ResolvedCapability::WorkspaceFileWrite, ResolvedCapability::WorkspaceFileRead];
        assert_eq!(unordered.validate(), Err(ToolDefinitionError::CapabilityOrder));

        let claim = ResourceClaimV1::Artifact {
            selector: ResourceClaimSelectorV1::InvocationContext {
                context: ResourceInvocationContextV1::ToolCall,
            },
            access: aura_tool_contract::ArtifactResourceAccessV1::Read,
        };
        let mut duplicate_claim = definition(
            "Context search",
            "Search session context",
            CanonicalHashV1::sha256(b"runtime-v1"),
        );
        duplicate_claim.resource_claims = vec![claim.clone(), claim];
        assert_eq!(duplicate_claim.validate(), Err(ToolDefinitionError::DuplicateResourceClaim));
    }
}
