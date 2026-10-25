use std::{borrow::Cow, fmt, str::FromStr};

use aura_domain_common::Revision;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use thiserror::Error;

use crate::{
    CanonicalJsonSchemaDocumentV1, ToolContractFingerprintV1, ToolInputSchemaV1, ToolSchemaDialect,
    ToolStructuredResultSchemaV1, ToolVersion,
};
use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{ToolId, ToolInstanceId, ToolRegistrySnapshotId};

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolDefinitionContractError {
    #[error("canonical tool name must contain valid dot-separated ASCII segments")]
    InvalidCanonicalName,
    #[error("tool model-visible description and summary must not be blank")]
    InvalidModelText,
    #[error("tool schema snapshot hash is inconsistent")]
    SnapshotHashMismatch,
    #[error(
        "tool schema set must be strictly ordered by ToolId and contain one version per ToolId"
    )]
    InvalidSchemaSetOrder,
    #[error("tool schema set hash is inconsistent")]
    ToolSetHashMismatch,
    #[error("tool schema snapshot metadata is inconsistent")]
    InvalidSchemaMetadata,
    #[error("tool definition fingerprint is inconsistent")]
    InvalidDefinitionFingerprint,
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ToolCanonicalName(String);

impl ToolCanonicalName {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolDefinitionContractError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 128
            && value.is_ascii()
            && value.split('.').all(|segment| {
                !segment.is_empty()
                    && segment.len() <= 64
                    && segment.bytes().enumerate().all(|(index, byte)| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || (index > 0 && matches!(byte, b'_' | b'-'))
                    })
            });
        valid.then_some(Self(value)).ok_or(ToolDefinitionContractError::InvalidCanonicalName)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for ToolCanonicalName {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolCanonicalName")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 128,
            "pattern": "^[a-z0-9][a-z0-9_-]{0,63}(?:[.][a-z0-9][a-z0-9_-]{0,63})*$"
        })
    }
}

impl fmt::Debug for ToolCanonicalName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ToolCanonicalName").field(&self.0).finish()
    }
}

impl fmt::Display for ToolCanonicalName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ToolCanonicalName {
    type Err = ToolDefinitionContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for ToolCanonicalName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolSourceKindV1 {
    BuiltIn,
    UserCreated,
    ModelCreated,
    Imported,
    Mcp,
    Plugin,
}

macro_rules! source_contract_fingerprint_type {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(CanonicalHashV1);

        impl $name {
            #[must_use]
            pub const fn new(fingerprint: CanonicalHashV1) -> Self {
                Self(fingerprint)
            }

            #[must_use]
            pub const fn fingerprint(&self) -> &CanonicalHashV1 {
                &self.0
            }
        }
    };
}

source_contract_fingerprint_type!(SignedBuiltinToolContractFingerprintV1);
source_contract_fingerprint_type!(NormalizedMcpToolContractFingerprintV1);
source_contract_fingerprint_type!(NormalizedPluginToolContractFingerprintV1);

/// Exact, source-specific contract identity frozen into a Registry binding.
///
/// Package-authored tools retain the complete W2 fingerprint (including the
/// authoritative runtime-closure hash). Built-ins and external adapters use
/// their own normalized/signed contract fingerprints so a generic digest can
/// never be silently reinterpreted as a different source class.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "source_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolSourceContractFingerprintV1 {
    BuiltIn {
        signed_builtin_tool_contract_fingerprint: SignedBuiltinToolContractFingerprintV1,
    },
    UserCreated {
        tool_contract_fingerprint: ToolContractFingerprintV1,
    },
    ModelCreated {
        tool_contract_fingerprint: ToolContractFingerprintV1,
    },
    Imported {
        tool_contract_fingerprint: ToolContractFingerprintV1,
    },
    Mcp {
        normalized_mcp_tool_contract_fingerprint: NormalizedMcpToolContractFingerprintV1,
    },
    Plugin {
        normalized_plugin_tool_contract_fingerprint: NormalizedPluginToolContractFingerprintV1,
    },
}

impl ToolSourceContractFingerprintV1 {
    #[must_use]
    pub const fn source_kind(&self) -> ToolSourceKindV1 {
        match self {
            Self::BuiltIn { .. } => ToolSourceKindV1::BuiltIn,
            Self::UserCreated { .. } => ToolSourceKindV1::UserCreated,
            Self::ModelCreated { .. } => ToolSourceKindV1::ModelCreated,
            Self::Imported { .. } => ToolSourceKindV1::Imported,
            Self::Mcp { .. } => ToolSourceKindV1::Mcp,
            Self::Plugin { .. } => ToolSourceKindV1::Plugin,
        }
    }

    pub fn package(
        source_kind: ToolSourceKindV1,
        tool_contract_fingerprint: ToolContractFingerprintV1,
    ) -> Result<Self, ToolDefinitionContractError> {
        match source_kind {
            ToolSourceKindV1::UserCreated => Ok(Self::UserCreated { tool_contract_fingerprint }),
            ToolSourceKindV1::ModelCreated => Ok(Self::ModelCreated { tool_contract_fingerprint }),
            ToolSourceKindV1::Imported => Ok(Self::Imported { tool_contract_fingerprint }),
            ToolSourceKindV1::BuiltIn | ToolSourceKindV1::Mcp | ToolSourceKindV1::Plugin => {
                Err(ToolDefinitionContractError::InvalidDefinitionFingerprint)
            }
        }
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolAudienceV1 {
    ChildRuntimeRequired,
    SubagentConfigurable,
    ParentAgentOnly,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolExposureV1 {
    Core,
    SessionPin,
    WorkspacePin,
    EphemeralLoad,
    // Callable only through `tools.call`; its definition is not sent with the request.
    UnifiedEntry,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolResultContractV1 {
    Unit,
    Utf8Text,
    StructuredJson { result_schema: ToolStructuredResultSchemaV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRegistrySnapshotRef {
    pub tool_registry_snapshot_id: ToolRegistrySnapshotId,
    pub registry_revision: Revision,
    pub content_hash: CanonicalHashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinitionSnapshotRef {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSchemaSnapshotV1 {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub canonical_name: ToolCanonicalName,
    pub description: String,
    pub input_schema: Value,
    pub schema_hash: CanonicalHashV1,
    pub schema_dialect: ToolSchemaDialect,
    pub schema_policy_version: u32,
    pub strict: bool,
    pub tool_schema_snapshot_hash: CanonicalHashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolSchemaSnapshotHashMaterial<'a> {
    tool_id: ToolId,
    tool_version: &'a ToolVersion,
    canonical_name: &'a ToolCanonicalName,
    description: &'a str,
    input_schema: &'a Value,
    schema_hash: &'a CanonicalHashV1,
    schema_dialect: ToolSchemaDialect,
    schema_policy_version: u32,
    strict: bool,
}

impl ToolSchemaSnapshotV1 {
    pub fn new(
        tool_id: ToolId,
        tool_version: ToolVersion,
        canonical_name: ToolCanonicalName,
        description: String,
        input_schema: ToolInputSchemaV1,
        strict: bool,
    ) -> Result<Self, ToolDefinitionContractError> {
        if description.trim().is_empty() {
            return Err(ToolDefinitionContractError::InvalidModelText);
        }
        let schema = input_schema.into_document();
        let mut snapshot = Self {
            tool_id,
            tool_version,
            canonical_name,
            description,
            input_schema: schema.document,
            schema_hash: schema.schema_hash,
            schema_dialect: schema.schema_dialect,
            schema_policy_version: schema.schema_policy_version,
            strict,
            tool_schema_snapshot_hash: CanonicalHashV1::sha256(&[]),
        };
        snapshot.tool_schema_snapshot_hash = snapshot.compute_hash()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), ToolDefinitionContractError> {
        if self.description.trim().is_empty() {
            return Err(ToolDefinitionContractError::InvalidModelText);
        }
        let document = CanonicalJsonSchemaDocumentV1 {
            schema_dialect: self.schema_dialect,
            schema_policy_version: self.schema_policy_version,
            document: self.input_schema.clone(),
            schema_hash: self.schema_hash.clone(),
        };
        ToolInputSchemaV1::try_from(document)
            .map_err(|_| ToolDefinitionContractError::InvalidSchemaMetadata)?;
        let hash = self.compute_hash()?;
        if hash != self.tool_schema_snapshot_hash {
            return Err(ToolDefinitionContractError::SnapshotHashMismatch);
        }
        Ok(())
    }

    fn compute_hash(&self) -> Result<CanonicalHashV1, ToolDefinitionContractError> {
        canonical_sha256(&ToolSchemaSnapshotHashMaterial {
            tool_id: self.tool_id,
            tool_version: &self.tool_version,
            canonical_name: &self.canonical_name,
            description: &self.description,
            input_schema: &self.input_schema,
            schema_hash: &self.schema_hash,
            schema_dialect: self.schema_dialect,
            schema_policy_version: self.schema_policy_version,
            strict: self.strict,
        })
        .map_err(|_| ToolDefinitionContractError::SnapshotHashMismatch)
    }
}

impl<'de> Deserialize<'de> for ToolSchemaSnapshotV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSnapshot {
            tool_id: ToolId,
            tool_version: ToolVersion,
            canonical_name: ToolCanonicalName,
            description: String,
            input_schema: Value,
            schema_hash: CanonicalHashV1,
            schema_dialect: ToolSchemaDialect,
            schema_policy_version: u32,
            strict: bool,
            tool_schema_snapshot_hash: CanonicalHashV1,
        }

        let value = WireSnapshot::deserialize(deserializer)?;
        let snapshot = Self {
            tool_id: value.tool_id,
            tool_version: value.tool_version,
            canonical_name: value.canonical_name,
            description: value.description,
            input_schema: value.input_schema,
            schema_hash: value.schema_hash,
            schema_dialect: value.schema_dialect,
            schema_policy_version: value.schema_policy_version,
            strict: value.strict,
            tool_schema_snapshot_hash: value.tool_schema_snapshot_hash,
        };
        snapshot.validate().map_err(D::Error::custom)?;
        Ok(snapshot)
    }
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSchemaSetSnapshotV1 {
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub tools: Vec<ToolSchemaSnapshotV1>,
    pub tool_set_hash: CanonicalHashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolSchemaSetHashMaterial<'a> {
    tools: Vec<ToolSchemaSetEntryHashMaterial<'a>>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolSchemaSetEntryHashMaterial<'a> {
    tool_id: ToolId,
    tool_version: &'a ToolVersion,
    tool_schema_snapshot_hash: &'a CanonicalHashV1,
}

impl ToolSchemaSetSnapshotV1 {
    pub fn new(
        registry_snapshot_ref: ToolRegistrySnapshotRef,
        tools: Vec<ToolSchemaSnapshotV1>,
    ) -> Result<Self, ToolDefinitionContractError> {
        let mut snapshot =
            Self { registry_snapshot_ref, tools, tool_set_hash: CanonicalHashV1::sha256(&[]) };
        snapshot.validate_order()?;
        snapshot.tool_set_hash = snapshot.compute_hash()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), ToolDefinitionContractError> {
        self.validate_order()?;
        for tool in &self.tools {
            tool.validate()?;
        }
        if self.compute_hash()? != self.tool_set_hash {
            return Err(ToolDefinitionContractError::ToolSetHashMismatch);
        }
        Ok(())
    }

    fn validate_order(&self) -> Result<(), ToolDefinitionContractError> {
        if !self.tools.windows(2).all(|pair| pair[0].tool_id < pair[1].tool_id) {
            return Err(ToolDefinitionContractError::InvalidSchemaSetOrder);
        }
        Ok(())
    }

    fn compute_hash(&self) -> Result<CanonicalHashV1, ToolDefinitionContractError> {
        let tools = self
            .tools
            .iter()
            .map(|tool| ToolSchemaSetEntryHashMaterial {
                tool_id: tool.tool_id,
                tool_version: &tool.tool_version,
                tool_schema_snapshot_hash: &tool.tool_schema_snapshot_hash,
            })
            .collect();
        canonical_sha256(&ToolSchemaSetHashMaterial { tools })
            .map_err(|_| ToolDefinitionContractError::ToolSetHashMismatch)
    }
}

impl<'de> Deserialize<'de> for ToolSchemaSetSnapshotV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSet {
            registry_snapshot_ref: ToolRegistrySnapshotRef,
            tools: Vec<ToolSchemaSnapshotV1>,
            tool_set_hash: CanonicalHashV1,
        }

        let value = WireSet::deserialize(deserializer)?;
        let snapshot = Self {
            registry_snapshot_ref: value.registry_snapshot_ref,
            tools: value.tools,
            tool_set_hash: value.tool_set_hash,
        };
        snapshot.validate().map_err(D::Error::custom)?;
        Ok(snapshot)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveToolBindingSnapshot {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub tool_instance_id: ToolInstanceId,
    pub source_kind: ToolSourceKindV1,
    pub exposure: ToolExposureV1,
    pub definition_fingerprint: CanonicalHashV1,
    pub source_contract_fingerprint: ToolSourceContractFingerprintV1,
}

impl TryFrom<CanonicalJsonSchemaDocumentV1> for ToolInputSchemaV1 {
    type Error = crate::ToolSchemaError;

    fn try_from(value: CanonicalJsonSchemaDocumentV1) -> Result<Self, Self::Error> {
        ToolInputSchemaV1::new(value.document)
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::Revision;
    use serde_json::json;

    use super::{
        ToolCanonicalName, ToolDefinitionContractError, ToolRegistrySnapshotRef,
        ToolSchemaSetSnapshotV1, ToolSchemaSnapshotV1,
    };
    use crate::{ToolInputSchemaV1, ToolVersion};
    use aura_domain_common::{ToolId, ToolRegistrySnapshotId};

    #[test]
    fn canonical_names_are_closed_ascii_segments() {
        assert!("fs.read_text".parse::<ToolCanonicalName>().is_ok());
        assert!("3d.render-v2".parse::<ToolCanonicalName>().is_ok());
        for invalid in ["Fs.read", "a..b", "_a.b", "a/b", ""] {
            assert!(invalid.parse::<ToolCanonicalName>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn schema_set_requires_tool_id_order_and_hashes_the_snapshot_identity() {
        let registry = ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
            registry_revision: Revision::INITIAL,
            content_hash: aura_domain_common::jcs::canonical_sha256(&json!({"registry": 1}))
                .expect("hash"),
        };
        let make = |tool_id| {
            ToolSchemaSnapshotV1::new(
                tool_id,
                ToolVersion::new(None, Revision::INITIAL).expect("version"),
                "fs.read".parse().expect("name"),
                "Read a file".to_owned(),
                ToolInputSchemaV1::new(json!({"type": "object"})).expect("schema"),
                false,
            )
            .expect("snapshot")
        };
        let high = ToolId::from_bytes([2; 16]);
        let low = ToolId::from_bytes([1; 16]);
        assert_eq!(
            ToolSchemaSetSnapshotV1::new(registry.clone(), vec![make(high), make(low)]),
            Err(ToolDefinitionContractError::InvalidSchemaSetOrder)
        );
        ToolSchemaSetSnapshotV1::new(registry, vec![make(low), make(high)])
            .expect("ordered schema set");
    }
}
