use std::{
    borrow::Cow,
    collections::BTreeSet,
    sync::{Arc, Mutex, OnceLock},
};

use jsonschema::{Draft, PatternOptions, Validator};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use thiserror::Error;

use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256, canonicalize_json_value};

mod document_policy;
mod policy_cache;
mod validation;
mod validation_message;

use document_policy::validate_schema_document;

pub const CANONICAL_TOOL_SCHEMA_POLICY_VERSION: u32 = 1;
pub const MAX_CANONICAL_SCHEMA_BYTES: usize = 512 * 1024;
pub const MAX_SCHEMA_NODES: usize = 16_384;
pub const MAX_SCHEMA_DEPTH: usize = 64;
pub const MAX_SCHEMA_REFS: usize = 4_096;
pub const MAX_SCHEMA_REF_CHAIN: usize = 64;
pub const MAX_SCHEMA_PATTERN_BYTES: usize = 8 * 1024;
pub const MAX_SCHEMA_PATTERN_TOTAL_BYTES: usize = 64 * 1024;
pub const MAX_SCHEMA_APPLICATOR_BRANCHES: usize = 2_048;
pub const MAX_SCHEMA_ENUM_VALUES: usize = 4_096;
pub const MAX_PUBLIC_SCHEMA_ERRORS: usize = 32;
pub const MAX_PROVIDER_ARGUMENTS_BYTES: usize = 512 * 1024;
/// The field that carries a call's purpose beside the tool's own fields (Provider envelope 3).
pub const PROVIDER_PURPOSE_FIELD: &str = "aura_purpose";
/// Root fields no tool schema may declare: the purpose, and the metadata object of envelopes
/// 1 and 2, which stored history still contains.
const RESERVED_PROVIDER_FIELDS: [&str; 2] = [PROVIDER_PURPOSE_FIELD, "_aura"];

const DRAFT_2020_12_URI: &str = "https://json-schema.org/draft/2020-12/schema";
const ALLOWED_FORMATS: [&str; 15] = [
    "date-time",
    "date",
    "time",
    "duration",
    "email",
    "hostname",
    "ipv4",
    "ipv6",
    "uuid",
    "uri",
    "uri-reference",
    "uri-template",
    "json-pointer",
    "relative-json-pointer",
    "regex",
];
const ALLOWED_KEYWORDS: [&str; 45] = [
    "$schema",
    "$ref",
    "$defs",
    "title",
    "description",
    "deprecated",
    "examples",
    "type",
    "enum",
    "const",
    "multipleOf",
    "maximum",
    "exclusiveMaximum",
    "minimum",
    "exclusiveMinimum",
    "maxLength",
    "minLength",
    "pattern",
    "format",
    "prefixItems",
    "items",
    "contains",
    "maxContains",
    "minContains",
    "maxItems",
    "minItems",
    "uniqueItems",
    "maxProperties",
    "minProperties",
    "required",
    "properties",
    "patternProperties",
    "additionalProperties",
    "dependentRequired",
    "dependentSchemas",
    "propertyNames",
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "if",
    "then",
    "else",
    "unevaluatedItems",
    "unevaluatedProperties",
];
const RESULT_ROOT_TYPES: [&str; 7] =
    ["object", "array", "string", "number", "integer", "boolean", "null"];

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub enum ToolSchemaDialect {
    #[serde(rename = "json_schema_draft_2020_12")]
    #[schemars(rename = "json_schema_draft_2020_12")]
    JsonSchemaDraft202012,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolSchemaError {
    #[error("tool schema must be a JSON object")]
    BooleanOrNonObjectSchema,
    #[error("tool input schema root must explicitly declare type=object")]
    InputRootNotObject,
    #[error(
        "tool input schema root must not declare a reserved Aura Provider field ('aura_purpose' or '_aura')"
    )]
    ReservedProviderMetadataProperty,
    #[error("structured result schema root must explicitly declare a supported JSON type")]
    ResultRootTypeInvalid,
    #[error("tool schema declares an unsupported meta-schema")]
    UnsupportedMetaSchema,
    #[error("tool schema contains unsupported keyword '{keyword}'")]
    UnsupportedKeyword { keyword: String },
    #[error("tool schema contains unsupported format '{format}'")]
    UnsupportedFormat { format: String },
    #[error("tool schema contains an external or malformed reference")]
    ExternalReference,
    #[error("tool schema pattern is invalid or outside the linear-time subset")]
    InvalidPattern,
    #[error("tool schema exceeds the canonical byte limit")]
    CanonicalByteLimit,
    #[error("tool schema exceeds its structural complexity policy")]
    ComplexityLimit,
    #[error("tool schema reference chain is cyclic or exceeds its limit")]
    ReferenceChainLimit,
    #[error("tool schema does not satisfy the Draft 2020-12 meta-schema")]
    MetaSchemaInvalid,
    #[error("tool schema could not be compiled by the frozen validator")]
    ValidatorCompilation,
    #[error("tool schema hash does not match its canonical document")]
    HashMismatch,
    #[error("tool schema canonicalization failed")]
    Canonicalization,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalJsonSchemaDocumentV1 {
    pub schema_dialect: ToolSchemaDialect,
    pub schema_policy_version: u32,
    pub document: Value,
    pub schema_hash: CanonicalHashV1,
}

impl JsonSchema for CanonicalJsonSchemaDocumentV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("CanonicalJsonSchemaDocumentV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "schema_dialect": {
                    "type": "string",
                    "const": "json_schema_draft_2020_12"
                },
                "schema_policy_version": { "type": "integer", "const": 1 },
                "document": { "type": "object" },
                "schema_hash": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "algorithm": { "type": "string", "const": "sha256" },
                        "canonicalization_version": {
                            "type": "string",
                            "const": "rfc8785_jcs_v1"
                        },
                        "hex": {
                            "type": "string",
                            "minLength": 64,
                            "maxLength": 64,
                            "pattern": "^[0-9a-f]{64}$"
                        }
                    },
                    "required": ["algorithm", "canonicalization_version", "hex"]
                }
            },
            "required": ["schema_dialect", "schema_policy_version", "document", "schema_hash"]
        })
    }
}

impl CanonicalJsonSchemaDocumentV1 {
    pub fn new(document: Value) -> Result<Self, ToolSchemaError> {
        validate_schema_document(&document)?;
        let schema_hash =
            canonical_sha256(&document).map_err(|_| ToolSchemaError::Canonicalization)?;
        Ok(Self {
            schema_dialect: ToolSchemaDialect::JsonSchemaDraft202012,
            schema_policy_version: CANONICAL_TOOL_SCHEMA_POLICY_VERSION,
            document,
            schema_hash,
        })
    }

    pub fn validate(&self) -> Result<(), ToolSchemaError> {
        if self.schema_dialect != ToolSchemaDialect::JsonSchemaDraft202012
            || self.schema_policy_version != CANONICAL_TOOL_SCHEMA_POLICY_VERSION
        {
            return Err(ToolSchemaError::UnsupportedMetaSchema);
        }
        validate_schema_document(&self.document)?;
        let expected =
            canonical_sha256(&self.document).map_err(|_| ToolSchemaError::Canonicalization)?;
        if self.schema_hash != expected {
            return Err(ToolSchemaError::HashMismatch);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for CanonicalJsonSchemaDocumentV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireDocument {
            schema_dialect: ToolSchemaDialect,
            schema_policy_version: u32,
            document: Value,
            schema_hash: CanonicalHashV1,
        }

        let wire = WireDocument::deserialize(deserializer)?;
        let document = Self {
            schema_dialect: wire.schema_dialect,
            schema_policy_version: wire.schema_policy_version,
            document: wire.document,
            schema_hash: wire.schema_hash,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolInputSchemaV1(CanonicalJsonSchemaDocumentV1);

impl ToolInputSchemaV1 {
    pub fn new(document: Value) -> Result<Self, ToolSchemaError> {
        let schema = CanonicalJsonSchemaDocumentV1::new(document)?;
        ensure_input_root(&schema.document)?;
        Ok(Self(schema))
    }

    #[must_use]
    pub fn document(&self) -> &Value {
        &self.0.document
    }

    #[must_use]
    pub fn schema_hash(&self) -> &CanonicalHashV1 {
        &self.0.schema_hash
    }

    #[must_use]
    pub const fn canonical_document(&self) -> &CanonicalJsonSchemaDocumentV1 {
        &self.0
    }

    #[must_use]
    pub fn into_document(self) -> CanonicalJsonSchemaDocumentV1 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ToolInputSchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = CanonicalJsonSchemaDocumentV1::deserialize(deserializer)?;
        ensure_input_root(&document.document).map_err(D::Error::custom)?;
        Ok(Self(document))
    }
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolStructuredResultSchemaV1(CanonicalJsonSchemaDocumentV1);

impl ToolStructuredResultSchemaV1 {
    pub fn new(document: Value) -> Result<Self, ToolSchemaError> {
        let schema = CanonicalJsonSchemaDocumentV1::new(document)?;
        ensure_result_root(&schema.document)?;
        Ok(Self(schema))
    }

    #[must_use]
    pub fn document(&self) -> &Value {
        &self.0.document
    }

    #[must_use]
    pub fn schema_hash(&self) -> &CanonicalHashV1 {
        &self.0.schema_hash
    }

    #[must_use]
    pub const fn canonical_document(&self) -> &CanonicalJsonSchemaDocumentV1 {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ToolStructuredResultSchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = CanonicalJsonSchemaDocumentV1::deserialize(deserializer)?;
        ensure_result_root(&document.document).map_err(D::Error::custom)?;
        Ok(Self(document))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSchemaValidationIssueV1 {
    pub instance_path: String,
    pub schema_path: String,
    pub issue_kind: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSchemaValidationReportV1 {
    pub valid: bool,
    pub errors: Vec<ToolSchemaValidationIssueV1>,
    pub total_error_count: u64,
}

pub use validation::CompiledToolSchemaV1;

fn ensure_input_root(schema: &Value) -> Result<(), ToolSchemaError> {
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(ToolSchemaError::InputRootNotObject);
    }
    let reserved = |name: &str| RESERVED_PROVIDER_FIELDS.contains(&name);
    let declares_reserved_property = schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|properties| properties.keys().any(|name| reserved(name)))
        || schema.get("required").and_then(Value::as_array).is_some_and(|required| {
            required.iter().any(|value| value.as_str().is_some_and(reserved))
        })
        || schema
            .get("dependentRequired")
            .and_then(Value::as_object)
            .is_some_and(|dependencies| dependencies.keys().any(|name| reserved(name)))
        || schema
            .get("dependentSchemas")
            .and_then(Value::as_object)
            .is_some_and(|dependencies| dependencies.keys().any(|name| reserved(name)));
    if declares_reserved_property {
        return Err(ToolSchemaError::ReservedProviderMetadataProperty);
    }
    Ok(())
}

fn ensure_result_root(schema: &Value) -> Result<(), ToolSchemaError> {
    let Some(root_type) = schema.get("type") else {
        return Err(ToolSchemaError::ResultRootTypeInvalid);
    };
    let valid = match root_type {
        Value::String(value) => RESULT_ROOT_TYPES.contains(&value.as_str()),
        Value::Array(values) => {
            !values.is_empty()
                && values.iter().all(|value| {
                    value.as_str().is_some_and(|value| RESULT_ROOT_TYPES.contains(&value))
                })
                && values.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>().len()
                    == values.len()
        }
        _ => false,
    };
    valid.then_some(()).ok_or(ToolSchemaError::ResultRootTypeInvalid)
}

#[cfg(test)]
thread_local! {
    static POLICY_VALIDATIONS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[path = "schema/tests/policy_reuse.rs"]
mod policy_reuse_tests;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CompiledToolSchemaV1, ToolInputSchemaV1, ToolSchemaError, ToolStructuredResultSchemaV1,
    };

    #[test]
    fn input_is_object_only_and_runtime_validation_never_coerces() {
        let schema = ToolInputSchemaV1::new(json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema#",
            "type": "object",
            "properties": {"count": {"type": "integer"}},
            "required": ["count"],
            "additionalProperties": false
        }))
        .expect("input schema");
        let compiled = CompiledToolSchemaV1::compile(&schema.clone().into_document())
            .expect("compiled schema");
        assert!(compiled.validate(&json!({"count": 2})).valid);
        let report = compiled.validate(&json!({"count": "2", "extra": true}));
        assert!(!report.valid);
        assert_eq!(report.total_error_count, 2);
    }

    #[test]
    fn compiled_validators_are_reused_without_trusting_forged_hashes() {
        let schema = ToolInputSchemaV1::new(json!({"type":"object","properties":{"n":{"type":"integer","minimum":1,"maximum":100}}})).expect("schema").into_document();
        let first = CompiledToolSchemaV1::compile(&schema).expect("first");
        let second = CompiledToolSchemaV1::compile(&schema).expect("cached");
        assert!(std::sync::Arc::ptr_eq(&first.validator, &second.validator));
        assert!(!second.validate(&json!({"n":101})).valid);
        let mut forged = schema;
        forged.document["properties"]["n"]["maximum"] = json!(1000);
        assert!(matches!(
            CompiledToolSchemaV1::compile(&forged),
            Err(ToolSchemaError::HashMismatch)
        ));
    }

    #[test]
    fn result_allows_explicit_scalar_or_union_roots() {
        ToolStructuredResultSchemaV1::new(json!({"type": "string"})).expect("string result");
        ToolStructuredResultSchemaV1::new(
            json!({"type": ["array", "null"], "items": {"type": "integer"}}),
        )
        .expect("union result");
        assert_eq!(
            ToolStructuredResultSchemaV1::new(json!({"oneOf": [{"type": "string"}]})),
            Err(ToolSchemaError::ResultRootTypeInvalid)
        );
    }

    #[test]
    fn policy_rejects_external_refs_unknown_formats_and_backtracking_patterns() {
        assert_eq!(
            ToolInputSchemaV1::new(
                json!({"type": "object", "$ref": "https://example.test/schema"})
            ),
            Err(ToolSchemaError::ExternalReference)
        );
        assert!(matches!(
            ToolInputSchemaV1::new(json!({
                "type": "object",
                "properties": {"value": {"type": "string", "format": "custom"}}
            })),
            Err(ToolSchemaError::UnsupportedFormat { .. })
        ));
        assert_eq!(
            ToolInputSchemaV1::new(json!({
                "type": "object",
                "properties": {"value": {"type": "string", "pattern": "(?=a)"}}
            })),
            Err(ToolSchemaError::InvalidPattern)
        );
    }

    #[test]
    fn input_root_reserves_provider_metadata_property() {
        assert_eq!(
            ToolInputSchemaV1::new(json!({
                "type": "object",
                "properties": {"_aura": {"type": "string"}}
            })),
            Err(ToolSchemaError::ReservedProviderMetadataProperty)
        );
        assert_eq!(
            ToolInputSchemaV1::new(json!({
                "type": "object",
                "required": ["_aura"]
            })),
            Err(ToolSchemaError::ReservedProviderMetadataProperty)
        );
        // Envelope 3 puts the purpose beside the tool's own fields, so a tool that declares
        // the same name would have two meanings for one field. It is refused when loaded.
        for schema in [
            json!({"type": "object", "properties": {"aura_purpose": {"type": "string"}}}),
            json!({"type": "object", "required": ["aura_purpose"]}),
            json!({"type": "object", "dependentRequired": {"aura_purpose": ["x"]}}),
        ] {
            assert_eq!(
                ToolInputSchemaV1::new(schema),
                Err(ToolSchemaError::ReservedProviderMetadataProperty)
            );
        }
        assert!(
            ToolInputSchemaV1::new(json!({
                "type": "object",
                "properties": {"purpose": {"type": "string"}, "arguments": {"type": "object"}}
            }))
            .is_ok(),
            "only the reserved names are refused; ordinary 'purpose' or 'arguments' fields stay"
        );
    }
}
