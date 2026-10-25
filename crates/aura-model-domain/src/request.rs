use std::collections::{BTreeSet, HashSet};

use aura_domain_common::{
    ArtifactId, ContentHash, ContentHashAlgorithm, ContextProjectionSnapshotId, ContractVersion,
    ModelAvailabilityTestId, ModelDiscoveryRunId, ModelExecutionSnapshotId, ModelProfileId,
    ModelStepId, PromptBundleSnapshotId,
};
use aura_permission_domain::NormalizedOrigin;
use aura_tool_contract::ToolRegistrySnapshotRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ChatOutputTokenFieldAlias, LogicalModelRequestError, ModelApiFormat,
    ProviderToolCallWireBinding, ProviderToolCallWireIdentity, ProviderToolNameBinding,
    ProviderWireToolCallId,
};

pub const MODEL_ADAPTER_COMPILATION_CONTRACT_VERSION: ContractVersion = ContractVersion::new(1, 1);
const LEGACY_ADAPTER_COMPILATION_CONTRACT_VERSION: ContractVersion = ContractVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRequestHeaderClass {
    ProtocolFixed,
    SemanticRouting,
    CredentialInjected,
    ContentMetadata,
    OtherRedacted,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequestTargetMetadata {
    pub method: String,
    pub origin: NormalizedOrigin,
    pub path: String,
    pub query_parameter_names: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequestHeaderMetadata {
    pub name: String,
    pub class: ProviderRequestHeaderClass,
    pub non_secret_value: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAdapterIdentity {
    pub api_format: ModelApiFormat,
    pub contract_version: ContractVersion,
}

impl ModelAdapterIdentity {
    #[must_use]
    pub const fn v1(api_format: ModelApiFormat) -> Self {
        Self { api_format, contract_version: LEGACY_ADAPTER_COMPILATION_CONTRACT_VERSION }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransportOperationContext {
    AgentModelAttempt {
        model_step_id: ModelStepId,
        model_execution_snapshot_id: ModelExecutionSnapshotId,
        context_snapshot_id: ContextProjectionSnapshotId,
        prompt_bundle_snapshot_id: PromptBundleSnapshotId,
    },
    ModelDiscovery {
        discovery_run_id: ModelDiscoveryRunId,
    },
    ModelAvailabilityTest {
        model_availability_test_id: ModelAvailabilityTestId,
        model_profile_id: ModelProfileId,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequestEvidence {
    pub body_sha256: ContentHash,
    pub body_byte_length: u64,
    pub target: ProviderRequestTargetMetadata,
    pub headers: Vec<ProviderRequestHeaderMetadata>,
    pub compatibility_removals: BTreeSet<String>,
    pub applied_output_token_field_alias: Option<ChatOutputTokenFieldAlias>,
    pub adapter: ModelAdapterIdentity,
    pub operation_context: TransportOperationContext,
    pub tool_registry_snapshot_ref: Option<ToolRegistrySnapshotRef>,
    pub tool_name_bindings: Vec<ProviderToolNameBinding>,
    pub historical_tool_call_wire_bindings: Vec<ProviderToolCallWireBinding>,
    pub canonical_body_artifact_id: Option<ArtifactId>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProviderRequestMetadataError {
    #[error("provider request method must be non-blank canonical uppercase ASCII")]
    InvalidMethod,
    #[error("provider request path must be an absolute path without query or fragment")]
    InvalidPath,
    #[error("provider request query parameter names must be non-blank names without values")]
    InvalidQueryParameterName,
    #[error("provider request header name must be canonical lowercase without control characters")]
    InvalidHeaderName,
    #[error("provider request header value contains a prohibited control character")]
    InvalidHeaderValue,
    #[error("credential headers must be redacted and classified as credential_injected")]
    InvalidCredentialHeader,
    #[error("credential_injected header metadata must never retain a value")]
    CredentialValueRetained,
    #[error("adapter compilation contract version must be 1.0 or 1.1")]
    InvalidAdapterContractVersion,
    #[error("provider request body evidence must use SHA-256")]
    InvalidBodyHash,
    #[error("Adapter Compilation Contract v1 has an empty compatibility-removal whitelist")]
    CompatibilityRemovalNotAllowed,
    #[error(
        "provider tool name bindings must be ordered by ToolId and unique by ToolId and wire name"
    )]
    InvalidToolBindings,
    #[error(transparent)]
    InvalidToolBinding(#[from] LogicalModelRequestError),
    #[error("historical tool-call wire bindings are invalid")]
    InvalidHistoricalToolCallBindings,
    #[error("tool registry snapshot evidence is inconsistent with the operation context")]
    InvalidToolRegistrySnapshot,
}

impl ProviderRequestTargetMetadata {
    pub fn validate(&self) -> Result<(), ProviderRequestMetadataError> {
        if self.method.is_empty()
            || !self.method.bytes().all(|byte| byte.is_ascii_uppercase() || byte == b'-')
        {
            return Err(ProviderRequestMetadataError::InvalidMethod);
        }
        if !self.path.starts_with('/')
            || self.path.contains('?')
            || self.path.contains('#')
            || self.path.bytes().any(|byte| matches!(byte, b'\r' | b'\n'))
        {
            return Err(ProviderRequestMetadataError::InvalidPath);
        }
        if self.query_parameter_names.iter().any(|name| {
            name.trim().is_empty()
                || name.contains('=')
                || name.bytes().any(|byte| matches!(byte, b'\r' | b'\n'))
        }) {
            return Err(ProviderRequestMetadataError::InvalidQueryParameterName);
        }
        Ok(())
    }
}

impl ProviderRequestHeaderMetadata {
    pub fn validate(&self) -> Result<(), ProviderRequestMetadataError> {
        if self.name.is_empty()
            || !self.name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
        {
            return Err(ProviderRequestMetadataError::InvalidHeaderName);
        }
        if self
            .non_secret_value
            .as_ref()
            .is_some_and(|value| value.bytes().any(|byte| matches!(byte, b'\r' | b'\n')))
        {
            return Err(ProviderRequestMetadataError::InvalidHeaderValue);
        }
        let is_credential_header = matches!(self.name.as_str(), "authorization" | "x-api-key");
        if is_credential_header
            && (self.class != ProviderRequestHeaderClass::CredentialInjected
                || self.non_secret_value.is_some())
        {
            return Err(ProviderRequestMetadataError::InvalidCredentialHeader);
        }
        if self.class == ProviderRequestHeaderClass::CredentialInjected
            && self.non_secret_value.is_some()
        {
            return Err(ProviderRequestMetadataError::CredentialValueRetained);
        }
        Ok(())
    }
}

impl ModelAdapterIdentity {
    /// Compilation contract 1.1 allows only a call-local ToolNotFound for an unknown name.
    #[must_use]
    pub fn current(api_format: ModelApiFormat) -> Self {
        Self { api_format, contract_version: MODEL_ADAPTER_COMPILATION_CONTRACT_VERSION }
    }

    pub fn validate(&self) -> Result<(), ProviderRequestMetadataError> {
        if self.contract_version != MODEL_ADAPTER_COMPILATION_CONTRACT_VERSION
            && self.contract_version != LEGACY_ADAPTER_COMPILATION_CONTRACT_VERSION
        {
            return Err(ProviderRequestMetadataError::InvalidAdapterContractVersion);
        }
        Ok(())
    }
}

impl ProviderRequestEvidence {
    /// Historical 1.0 evidence keeps its fail-the-step policy. This decision is
    /// frozen in request evidence, never inferred from a running binary version.
    #[must_use]
    pub fn allows_unknown_tool_names(&self) -> bool {
        self.adapter.contract_version == MODEL_ADAPTER_COMPILATION_CONTRACT_VERSION
            && matches!(
                &self.operation_context,
                TransportOperationContext::AgentModelAttempt { .. }
            )
    }

    pub fn validate(&self) -> Result<(), ProviderRequestMetadataError> {
        if self.body_sha256.algorithm != ContentHashAlgorithm::Sha256 {
            return Err(ProviderRequestMetadataError::InvalidBodyHash);
        }
        self.target.validate()?;
        self.adapter.validate()?;
        for header in &self.headers {
            header.validate()?;
        }
        if !self.compatibility_removals.is_empty() {
            return Err(ProviderRequestMetadataError::CompatibilityRemovalNotAllowed);
        }
        if !self
            .tool_name_bindings
            .windows(2)
            .all(|pair| pair[0].tool_definition.tool_id < pair[1].tool_definition.tool_id)
        {
            return Err(ProviderRequestMetadataError::InvalidToolBindings);
        }
        match &self.operation_context {
            TransportOperationContext::AgentModelAttempt { .. } => {
                let Some(registry_ref) = &self.tool_registry_snapshot_ref else {
                    return Err(ProviderRequestMetadataError::InvalidToolRegistrySnapshot);
                };
                if self
                    .tool_name_bindings
                    .iter()
                    .any(|binding| &binding.tool_definition.registry_snapshot_ref != registry_ref)
                {
                    return Err(ProviderRequestMetadataError::InvalidToolRegistrySnapshot);
                }
            }
            TransportOperationContext::ModelDiscovery { .. }
            | TransportOperationContext::ModelAvailabilityTest { .. }
                if self.tool_registry_snapshot_ref.is_some()
                    || !self.tool_name_bindings.is_empty() =>
            {
                return Err(ProviderRequestMetadataError::InvalidToolRegistrySnapshot);
            }
            TransportOperationContext::ModelDiscovery { .. }
            | TransportOperationContext::ModelAvailabilityTest { .. } => {}
        }
        let mut wire_names = HashSet::new();
        for binding in &self.tool_name_bindings {
            binding.validate_contract_shape()?;
            if !wire_names.insert(binding.provider_wire_name.as_str()) {
                return Err(ProviderRequestMetadataError::InvalidToolBindings);
            }
        }
        self.validate_historical_bindings_without_domain()
    }

    fn validate_historical_bindings_without_domain(
        &self,
    ) -> Result<(), ProviderRequestMetadataError> {
        if !self
            .historical_tool_call_wire_bindings
            .windows(2)
            .all(|pair| pair[0].tool_call_id < pair[1].tool_call_id)
        {
            return Err(ProviderRequestMetadataError::InvalidHistoricalToolCallBindings);
        }
        let mut wire_ids = HashSet::new();
        for binding in &self.historical_tool_call_wire_bindings {
            if binding.provider_wire_tool_name.trim().is_empty()
                || !wire_ids.insert(binding.wire_identity.wire_id())
            {
                return Err(ProviderRequestMetadataError::InvalidHistoricalToolCallBindings);
            }
            if let ProviderToolCallWireIdentity::AuraGenerated { provider_wire_tool_call_id } =
                &binding.wire_identity
                && provider_wire_tool_call_id
                    != &ProviderWireToolCallId::from_tool_call_id(binding.tool_call_id)
            {
                return Err(ProviderRequestMetadataError::InvalidHistoricalToolCallBindings);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use aura_domain_common::{
        ContentHash, ContentHashAlgorithm, ContextProjectionSnapshotId, ModelExecutionSnapshotId,
        ModelStepId, PromptBundleSnapshotId,
    };
    use aura_permission_domain::NormalizedOrigin;

    use super::{
        ModelAdapterIdentity, ProviderRequestEvidence, ProviderRequestHeaderClass,
        ProviderRequestHeaderMetadata, ProviderRequestTargetMetadata, TransportOperationContext,
    };
    use crate::ModelApiFormat;

    #[test]
    fn request_metadata_is_redacted_and_canonical() {
        let target = ProviderRequestTargetMetadata {
            method: "POST".to_owned(),
            origin: NormalizedOrigin {
                scheme: "https".to_owned(),
                host: "api.example.test".to_owned(),
                port: 443,
            },
            path: "/v1/responses".to_owned(),
            query_parameter_names: BTreeSet::new(),
        };
        assert!(target.validate().is_ok());

        let credential = ProviderRequestHeaderMetadata {
            name: "authorization".to_owned(),
            class: ProviderRequestHeaderClass::CredentialInjected,
            non_secret_value: None,
        };
        assert!(credential.validate().is_ok());
        assert!(
            ProviderRequestHeaderMetadata {
                non_secret_value: Some("Bearer secret".to_owned()),
                ..credential.clone()
            }
            .validate()
            .is_err()
        );
        assert!(ModelAdapterIdentity::v1(ModelApiFormat::OpenAiResponses).validate().is_ok());

        let evidence = ProviderRequestEvidence {
            body_sha256: ContentHash::new(ContentHashAlgorithm::Sha256, "11".repeat(32))
                .expect("sha256"),
            body_byte_length: 2,
            target,
            headers: vec![credential],
            compatibility_removals: BTreeSet::new(),
            applied_output_token_field_alias: None,
            adapter: ModelAdapterIdentity::v1(ModelApiFormat::OpenAiResponses),
            operation_context: TransportOperationContext::AgentModelAttempt {
                model_step_id: ModelStepId::new(),
                model_execution_snapshot_id: ModelExecutionSnapshotId::new(),
                context_snapshot_id: ContextProjectionSnapshotId::new(),
                prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
            },
            tool_registry_snapshot_ref: Some(aura_tool_contract::ToolRegistrySnapshotRef {
                tool_registry_snapshot_id: aura_domain_common::ToolRegistrySnapshotId::new(),
                registry_revision: aura_domain_common::Revision::INITIAL,
                content_hash: aura_domain_common::jcs::CanonicalHashV1::sha256(b"registry"),
            }),
            tool_name_bindings: Vec::new(),
            historical_tool_call_wire_bindings: Vec::new(),
            canonical_body_artifact_id: None,
        };
        assert!(evidence.validate().is_ok());
        assert!(
            ProviderRequestEvidence { body_sha256: ContentHash::blake3(b"{}"), ..evidence }
                .validate()
                .is_err()
        );
    }
}
