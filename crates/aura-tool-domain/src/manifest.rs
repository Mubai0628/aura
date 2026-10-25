use std::{borrow::Cow, cmp::Ordering, collections::BTreeSet, ffi::OsStr};

use aura_domain_common::CanonicalSemVer;
use aura_domain_common::jcs::canonical_json_jcs;
pub use aura_tool_contract::ToolPackageConformanceCaseNameV1;
use aura_tool_contract::{
    ArgumentJsonPointer, CommandArgumentV1, CommandCredentialEnvironmentSlotV1,
    CommandEnvironmentEntryV1, CommandFlagV1, CommandLiteralV1, CommandOptionNameV1,
    CommandWorkingDirectoryV1, DeclarativeCommandV1, DeclarativeHttpBodyV1,
    DeclarativeHttpMethodV1, DeclarativeHttpTargetV1, DeclarativeHttpV1, HttpCredentialSlotV1,
    HttpHeaderEntryV1, HttpJsonPropertiesV1, HttpJsonPropertyNameV1, HttpJsonPropertyV1,
    HttpJsonTemplateV1, HttpPathSegmentV1, HttpQueryEntryV1, HttpStatusRangeV1,
    MAX_TOOL_PACKAGE_DECLARED_FILES_V1, PackageDeclaredResourceClaimV1, PackageKey,
    PackageRelativePath, TOOL_PACKAGE_IMPORT_POLICY_V1, TOOL_PACKAGE_MANIFEST_NAME,
    ToolCanonicalName, ToolInputSchemaV1, ToolKey, ToolPackageFileEntryV1, ToolResultContractV1,
    ToolRuntimeContractError, ToolRuntimeDescriptorV1, ToolStructuredResultSchemaV1,
    validate_command_environment_v1, validate_http_header_bindings_v1,
};
use aura_windows_platform::windows_ordinal_compare_ignore_case;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;

use crate::ToolExecutionSemanticsV1;

pub const TOOL_PACKAGE_MANIFEST_SCHEMA_VERSION_V1: u32 = 1;
pub const TOOL_PACKAGE_STDIO_PROTOCOL_VERSION_V1: u32 = 1;
pub const DEFAULT_PACKAGE_TOOL_AUDIENCE_V1: PackageToolAudienceV1 =
    PackageToolAudienceV1::SubagentConfigurable;
pub const DEFAULT_COMMAND_SUCCESS_EXIT_CODES_V1: &[i32] = &[0];
pub const DEFAULT_HTTP_SUCCESS_STATUS_RANGES_V1: &[HttpStatusRangeV1] =
    &[HttpStatusRangeV1 { start: 200, end: 299 }];

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolPackageManifestContractError {
    InvalidText,
    InvalidCanonicalSuffix,
    InvalidCollectionOrder,
    DuplicateCollectionItem,
    DuplicateToolCanonicalName,
    InvalidInstanceConfigurationReference,
    InvalidRuntimeContract,
    DuplicateEnvironmentName,
    CredentialEnvironmentConflict,
    DuplicateHttpHeader,
    CredentialHttpHeaderConflict,
    InvalidFilePolicy,
    ManifestListedAsFile,
    WindowsPathCollision,
    InvalidSchemaContract,
    CanonicalManifestBytesMismatch,
    RuntimeClosureMismatch,
}

impl std::fmt::Display for ToolPackageManifestContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidText => "tool package text violates its frozen byte or control policy",
            Self::InvalidCanonicalSuffix => "tool package canonical suffix is invalid",
            Self::InvalidCollectionOrder => {
                "canonical tool package collection is not in frozen canonical order"
            }
            Self::DuplicateCollectionItem => "tool package collection contains a duplicate item",
            Self::DuplicateToolCanonicalName => {
                "tool package contains duplicate canonical model names"
            }
            Self::InvalidInstanceConfigurationReference => {
                "tool package environment instance_configuration reference is invalid"
            }
            Self::InvalidRuntimeContract => "tool package runtime contract is invalid",
            Self::DuplicateEnvironmentName => {
                "tool package environment names must be unique under Windows case folding"
            }
            Self::CredentialEnvironmentConflict => {
                "tool package credential environment conflicts with ordinary environment"
            }
            Self::DuplicateHttpHeader => {
                "tool package HTTP header names must be unique without regard to ASCII case"
            }
            Self::CredentialHttpHeaderConflict => {
                "tool package credential HTTP header conflicts with an ordinary header"
            }
            Self::InvalidFilePolicy => "tool package files violate the fixed V1 import policy",
            Self::ManifestListedAsFile => {
                "tool package files must not contain aura-tool-package.json"
            }
            Self::WindowsPathCollision => {
                "tool package file paths collide under Windows ordinal case-insensitive comparison"
            }
            Self::InvalidSchemaContract => {
                "tool package JSON Schema violates the canonical schema contract"
            }
            Self::CanonicalManifestBytesMismatch => {
                "canonical manifest bytes are not the RFC 8785 encoding of the supplied manifest"
            }
            Self::RuntimeClosureMismatch => {
                "stored package runtime closure does not match the exact canonical manifest"
            }
        })
    }
}

impl std::error::Error for ToolPackageManifestContractError {}

fn valid_manifest_text(value: &str, maximum_bytes: usize, multiline: bool) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && !value.trim().is_empty()
        && !value.chars().any(|character| {
            character == '\u{007f}'
                || if multiline {
                    character <= '\u{001f}' && !matches!(character, '\n' | '\t')
                } else {
                    character <= '\u{001f}'
                }
        })
}

macro_rules! manifest_text_type {
    ($name:ident, $schema_name:literal, $maximum:literal, $multiline:literal, $pattern:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ToolPackageManifestContractError> {
                let value = value.into();
                valid_manifest_text(&value, $maximum, $multiline)
                    .then_some(Self(value))
                    .ok_or(ToolPackageManifestContractError::InvalidText)
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed($schema_name)
            }

            fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "minLength": 1,
                    "maxLength": $maximum,
                    "pattern": $pattern
                })
            }
        }
    };
}

manifest_text_type!(
    ToolPackageUiDisplayNameV1,
    "ToolPackageUiDisplayNameV1",
    256,
    false,
    "^[^\\u0000-\\u001f\\u007f]+$"
);

fn normalize_raw_multiline(value: String) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

macro_rules! raw_multiline_text_type {
    ($name:ident, $inner:ty, $schema_name:literal, $raw_maximum:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name($inner);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ToolPackageManifestContractError> {
                let normalized = normalize_raw_multiline(value.into());
                <$inner>::new(normalized).map(Self)
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed($schema_name)
            }

            fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "minLength": 1,
                    "maxLength": $raw_maximum,
                    "pattern": "^[^\\u0000-\\u0008\\u000b-\\u000c\\u000e-\\u001f\\u007f]+$"
                })
            }
        }
    };
}

raw_multiline_text_type!(
    RawToolPackageUiDescriptionV1,
    ToolPackageUiDescriptionV1,
    "RawToolPackageUiDescriptionV1",
    8192
);
raw_multiline_text_type!(
    RawToolPackageModelDescriptionV1,
    ToolPackageModelDescriptionV1,
    "RawToolPackageModelDescriptionV1",
    32768
);
manifest_text_type!(
    ToolPackageUiDescriptionV1,
    "ToolPackageUiDescriptionV1",
    4096,
    true,
    "^[^\\u0000-\\u0008\\u000b-\\u001f\\u007f]+$"
);
manifest_text_type!(
    ToolPackageToolDisplayNameV1,
    "ToolPackageToolDisplayNameV1",
    256,
    false,
    "^[^\\u0000-\\u001f\\u007f]+$"
);
manifest_text_type!(
    ToolPackageModelSummaryV1,
    "ToolPackageModelSummaryV1",
    512,
    false,
    "^[^\\u0000-\\u001f\\u007f]+$"
);
manifest_text_type!(
    ToolPackageModelDescriptionV1,
    "ToolPackageModelDescriptionV1",
    16384,
    true,
    "^[^\\u0000-\\u0008\\u000b-\\u001f\\u007f]+$"
);
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PackageToolCanonicalSuffixV1(String);

impl PackageToolCanonicalSuffixV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolPackageManifestContractError> {
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
        valid.then_some(Self(value)).ok_or(ToolPackageManifestContractError::InvalidCanonicalSuffix)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PackageToolCanonicalSuffixV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for PackageToolCanonicalSuffixV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PackageToolCanonicalSuffixV1")
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

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PackageToolAudienceV1 {
    SubagentConfigurable,
    ParentAgentOnly,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PackageCapabilityRequirementV1 {
    WorkspaceFileRead,
    WorkspaceFileWrite,
    WorkspaceFileCreate,
    WorkspaceFileDelete,
    ExternalFileRead,
    ExternalFileWrite,
    ExternalFileCreate,
    ExternalFileDelete,
    ProcessExecute,
    GitRead,
    GitWrite,
    GitRemoteRead,
    GitRemoteWrite,
    GitDestructive,
    PublicNetworkRead,
    PrivateNetworkAccess,
    DataEgress,
    DownloadArtifact,
    DownloadWindowsExecutable,
    ExternalServiceRead,
    ExternalServiceMutation,
    ExternalServiceDestructiveMutation,
}

/// A numeric literal version so the generated JSON Schema freezes
/// `schema_version` to exactly `1`, rather than merely documenting it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToolPackageManifestSchemaVersionV1;

impl Serialize for ToolPackageManifestSchemaVersionV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(TOOL_PACKAGE_MANIFEST_SCHEMA_VERSION_V1)
    }
}

impl<'de> Deserialize<'de> for ToolPackageManifestSchemaVersionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        if value != TOOL_PACKAGE_MANIFEST_SCHEMA_VERSION_V1 {
            return Err(D::Error::custom("tool package manifest schema_version must be 1"));
        }
        Ok(Self)
    }
}

impl JsonSchema for ToolPackageManifestSchemaVersionV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolPackageManifestSchemaVersionV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "const": 1
        })
    }
}

/// A required numeric literal that makes the Package's Stdio protocol choice
/// explicit. V1 does not use an omitted-field default or a free-form protocol
/// identifier.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToolPackageStdioProtocolVersionV1;

impl Serialize for ToolPackageStdioProtocolVersionV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(TOOL_PACKAGE_STDIO_PROTOCOL_VERSION_V1)
    }
}

impl<'de> Deserialize<'de> for ToolPackageStdioProtocolVersionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        if value != TOOL_PACKAGE_STDIO_PROTOCOL_VERSION_V1 {
            return Err(D::Error::custom("tool package stdio protocol_version must be 1"));
        }
        Ok(Self)
    }
}

impl JsonSchema for ToolPackageStdioProtocolVersionV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolPackageStdioProtocolVersionV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "const": 1
        })
    }
}

fn deserialize_non_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let values = Vec::<T>::deserialize(deserializer)?;
    if values.is_empty() {
        return Err(D::Error::custom("array must contain at least one item"));
    }
    Ok(values)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolPackageCommandSuccessExitCodesV1(Vec<i32>);

impl ToolPackageCommandSuccessExitCodesV1 {
    pub fn new(values: Vec<i32>) -> Result<Self, &'static str> {
        if values.is_empty()
            || values.len() > 32
            || values.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(
                "tool package command success exit codes must be non-empty, strictly ordered, unique, and contain at most 32 values",
            );
        }
        Ok(Self(values))
    }

    #[must_use]
    pub fn as_slice(&self) -> &[i32] {
        &self.0
    }

    #[must_use]
    pub fn default_v1() -> Self {
        Self(DEFAULT_COMMAND_SUCCESS_EXIT_CODES_V1.to_vec())
    }
}

impl<'de> Deserialize<'de> for ToolPackageCommandSuccessExitCodesV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<i32>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for ToolPackageCommandSuccessExitCodesV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolPackageCommandSuccessExitCodesV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "array",
            "items": { "type": "integer", "format": "int32" },
            "minItems": 1,
            "maxItems": 32,
            "uniqueItems": true
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolPackageHttpSuccessStatusRangesV1(Vec<HttpStatusRangeV1>);

impl ToolPackageHttpSuccessStatusRangesV1 {
    pub fn new(values: Vec<HttpStatusRangeV1>) -> Result<Self, &'static str> {
        if values.is_empty()
            || values.len() > 16
            || values
                .iter()
                .any(|range| range.start < 100 || range.end > 599 || range.start > range.end)
            || values.windows(2).any(|pair| pair[0].end >= pair[1].start)
        {
            return Err(
                "tool package HTTP success status ranges must contain 1..=16 ordered, disjoint ranges within 100..=599",
            );
        }
        Ok(Self(values))
    }

    #[must_use]
    pub fn as_slice(&self) -> &[HttpStatusRangeV1] {
        &self.0
    }

    #[must_use]
    pub fn default_v1() -> Self {
        Self(DEFAULT_HTTP_SUCCESS_STATUS_RANGES_V1.to_vec())
    }
}

impl<'de> Deserialize<'de> for ToolPackageHttpSuccessStatusRangesV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<HttpStatusRangeV1>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for ToolPackageHttpSuccessStatusRangesV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolPackageHttpSuccessStatusRangesV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "array",
            "items": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "start": { "type": "integer", "minimum": 100, "maximum": 599 },
                    "end": { "type": "integer", "minimum": 100, "maximum": 599 }
                },
                "required": ["start", "end"]
            },
            "minItems": 1,
            "maxItems": 16
        })
    }
}

/// Shared wire contract for Package-authored JSON objects. Semantic consumers
/// still validate schema documents and conformance arguments in their owning
/// phase; this type only prevents either field from accepting a non-object.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolPackageJsonObjectV1(Value);

impl ToolPackageJsonObjectV1 {
    pub fn new(value: Value) -> Result<Self, &'static str> {
        value.is_object().then_some(Self(value)).ok_or("tool package JSON value must be an object")
    }

    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ToolPackageJsonObjectV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Value::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for ToolPackageJsonObjectV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolPackageJsonObjectV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "additionalProperties": true
        })
    }
}

pub type RawToolJsonSchemaV1 = ToolPackageJsonObjectV1;
pub type ToolPackageConformanceArgumentsV1 = ToolPackageJsonObjectV1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RawToolPackageInstanceConfigurationSchemaV1(pub Option<RawToolJsonSchemaV1>);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CanonicalToolPackageInstanceConfigurationSchemaV1(pub Option<ToolInputSchemaV1>);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CanonicalToolPackageUiDescriptionV1(pub Option<ToolPackageUiDescriptionV1>);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CanonicalCommandCredentialSlotV1(pub Option<CommandCredentialEnvironmentSlotV1>);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CanonicalHttpCredentialSlotV1(pub Option<HttpCredentialSlotV1>);

macro_rules! nullable_wrapper_schema {
    ($wrapper:ty, $inner:ty, $name:literal) => {
        impl JsonSchema for $wrapper {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed($name)
            }

            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                let inner = generator.subschema_for::<$inner>();
                json_schema!({
                    "anyOf": [inner, { "type": "null" }]
                })
            }
        }
    };
}

nullable_wrapper_schema!(
    RawToolPackageInstanceConfigurationSchemaV1,
    RawToolJsonSchemaV1,
    "RawToolPackageInstanceConfigurationSchemaV1"
);
nullable_wrapper_schema!(
    CanonicalToolPackageInstanceConfigurationSchemaV1,
    ToolInputSchemaV1,
    "CanonicalToolPackageInstanceConfigurationSchemaV1"
);
nullable_wrapper_schema!(
    CanonicalToolPackageUiDescriptionV1,
    ToolPackageUiDescriptionV1,
    "CanonicalToolPackageUiDescriptionV1"
);
nullable_wrapper_schema!(
    CanonicalCommandCredentialSlotV1,
    CommandCredentialEnvironmentSlotV1,
    "CanonicalCommandCredentialSlotV1"
);
nullable_wrapper_schema!(
    CanonicalHttpCredentialSlotV1,
    HttpCredentialSlotV1,
    "CanonicalHttpCredentialSlotV1"
);

fn deserialize_raw_instance_configuration<'de, D>(
    deserializer: D,
) -> Result<RawToolPackageInstanceConfigurationSchemaV1, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<RawToolJsonSchemaV1>::deserialize(deserializer)
        .map(RawToolPackageInstanceConfigurationSchemaV1)
}

fn deserialize_canonical_instance_configuration<'de, D>(
    deserializer: D,
) -> Result<CanonicalToolPackageInstanceConfigurationSchemaV1, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<ToolInputSchemaV1>::deserialize(deserializer)
        .map(CanonicalToolPackageInstanceConfigurationSchemaV1)
}

fn deserialize_canonical_ui_description<'de, D>(
    deserializer: D,
) -> Result<CanonicalToolPackageUiDescriptionV1, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<ToolPackageUiDescriptionV1>::deserialize(deserializer)
        .map(CanonicalToolPackageUiDescriptionV1)
}

fn deserialize_canonical_command_credential_slot<'de, D>(
    deserializer: D,
) -> Result<CanonicalCommandCredentialSlotV1, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<CommandCredentialEnvironmentSlotV1>::deserialize(deserializer)
        .map(CanonicalCommandCredentialSlotV1)
}

fn deserialize_canonical_http_credential_slot<'de, D>(
    deserializer: D,
) -> Result<CanonicalHttpCredentialSlotV1, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<HttpCredentialSlotV1>::deserialize(deserializer).map(CanonicalHttpCredentialSlotV1)
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawToolPackageUiV1 {
    pub display_name: ToolPackageUiDisplayNameV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<RawToolPackageUiDescriptionV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalToolPackageUiV1 {
    pub display_name: ToolPackageUiDisplayNameV1,
    #[serde(deserialize_with = "deserialize_canonical_ui_description")]
    #[schemars(required)]
    pub description: CanonicalToolPackageUiDescriptionV1,
}

/// UI-only metadata. It is deliberately separate from model-visible text and
/// therefore is not reused as a Provider description.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageToolUiV1 {
    pub display_name: ToolPackageToolDisplayNameV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RawToolPackageResultContractV1 {
    Unit,
    Utf8Text,
    StructuredJson { result_schema: RawToolJsonSchemaV1 },
}

/// Package-author supplied model contract before schema normalization and
/// hashing. The JSON Schema documents are the only intentionally open JSON
/// objects in this structure.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawToolPackageModelContractV1 {
    pub canonical_suffix: PackageToolCanonicalSuffixV1,
    pub description: RawToolPackageModelDescriptionV1,
    pub summary: ToolPackageModelSummaryV1,
    pub input_schema: RawToolJsonSchemaV1,
    pub strict: bool,
    pub result_contract: RawToolPackageResultContractV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalToolPackageModelContractV1 {
    pub canonical_name: ToolCanonicalName,
    pub description: ToolPackageModelDescriptionV1,
    pub summary: ToolPackageModelSummaryV1,
    pub input_schema: ToolInputSchemaV1,
    pub strict: bool,
    pub result_contract: ToolResultContractV1,
}

/// Declarative argv mapping authored by a Package. `NamedOption` carries the
/// frozen omission rule explicitly instead of relying on missing-value
/// coercion in a Runtime implementation.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolPackageCommandArgumentV1 {
    Literal {
        value: CommandLiteralV1,
    },
    Scalar {
        source: ArgumentJsonPointer,
    },
    Repeated {
        source: ArgumentJsonPointer,
    },
    ConditionalFlag {
        source: ArgumentJsonPointer,
        flag: CommandFlagV1,
    },
    NamedOption {
        name: CommandOptionNameV1,
        source: ArgumentJsonPointer,
        omit_when_missing_or_null: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RawToolPackageDeclarativeCommandV1 {
    Argv {
        executable: PackageRelativePath,
        arguments: Vec<ToolPackageCommandArgumentV1>,
        working_directory: CommandWorkingDirectoryV1,
        environment: Vec<CommandEnvironmentEntryV1>,
        credential_slot: Option<CommandCredentialEnvironmentSlotV1>,
        #[serde(skip_serializing_if = "Option::is_none")]
        success_exit_codes: Option<ToolPackageCommandSuccessExitCodesV1>,
    },
    PowershellScript {
        script: PackageRelativePath,
        arguments: Vec<ToolPackageCommandArgumentV1>,
        working_directory: CommandWorkingDirectoryV1,
        environment: Vec<CommandEnvironmentEntryV1>,
        credential_slot: Option<CommandCredentialEnvironmentSlotV1>,
        #[serde(skip_serializing_if = "Option::is_none")]
        success_exit_codes: Option<ToolPackageCommandSuccessExitCodesV1>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CanonicalToolPackageDeclarativeCommandV1 {
    Argv {
        executable: PackageRelativePath,
        arguments: Vec<ToolPackageCommandArgumentV1>,
        working_directory: CommandWorkingDirectoryV1,
        environment: Vec<CommandEnvironmentEntryV1>,
        #[serde(deserialize_with = "deserialize_canonical_command_credential_slot")]
        #[schemars(required)]
        credential_slot: CanonicalCommandCredentialSlotV1,
        success_exit_codes: ToolPackageCommandSuccessExitCodesV1,
    },
    PowershellScript {
        script: PackageRelativePath,
        arguments: Vec<ToolPackageCommandArgumentV1>,
        working_directory: CommandWorkingDirectoryV1,
        environment: Vec<CommandEnvironmentEntryV1>,
        #[serde(deserialize_with = "deserialize_canonical_command_credential_slot")]
        #[schemars(required)]
        credential_slot: CanonicalCommandCredentialSlotV1,
        success_exit_codes: ToolPackageCommandSuccessExitCodesV1,
    },
}

/// Raw HTTP JSON templates preserve author order for arrays and object
/// property lists. Object properties reject duplicate names immediately, but
/// ordering is applied only by the explicit Raw-to-Canonical contract
/// conversion below.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RawToolPackageHttpJsonTemplateV1 {
    Literal { value: Value },
    Argument { source: ArgumentJsonPointer },
    Array { items: Vec<RawToolPackageHttpJsonTemplateV1> },
    Object { properties: RawToolPackageHttpJsonPropertiesV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawToolPackageHttpJsonPropertyV1 {
    pub name: HttpJsonPropertyNameV1,
    pub value: RawToolPackageHttpJsonTemplateV1,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RawToolPackageHttpJsonPropertiesV1(Vec<RawToolPackageHttpJsonPropertyV1>);

impl RawToolPackageHttpJsonPropertiesV1 {
    pub fn new(
        values: Vec<RawToolPackageHttpJsonPropertyV1>,
    ) -> Result<Self, ToolPackageManifestContractError> {
        let mut names = BTreeSet::new();
        if values.iter().any(|property| !names.insert(property.name.as_str())) {
            return Err(ToolPackageManifestContractError::DuplicateCollectionItem);
        }
        Ok(Self(values))
    }

    #[must_use]
    pub fn as_slice(&self) -> &[RawToolPackageHttpJsonPropertyV1] {
        &self.0
    }

    fn into_canonical(self) -> Result<HttpJsonPropertiesV1, ToolPackageManifestContractError> {
        let mut values = self
            .0
            .into_iter()
            .map(|property| {
                Ok(HttpJsonPropertyV1 {
                    name: property.name,
                    value: property.value.into_canonical()?,
                })
            })
            .collect::<Result<Vec<_>, ToolPackageManifestContractError>>()?;
        values
            .sort_by(|left, right| jcs_property_name_cmp(left.name.as_str(), right.name.as_str()));
        HttpJsonPropertiesV1::new(values)
            .map_err(|_| ToolPackageManifestContractError::DuplicateCollectionItem)
    }
}

impl<'de> Deserialize<'de> for RawToolPackageHttpJsonPropertiesV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<RawToolPackageHttpJsonPropertyV1>::deserialize(deserializer)?)
            .map_err(D::Error::custom)
    }
}

impl RawToolPackageHttpJsonTemplateV1 {
    fn into_canonical(self) -> Result<HttpJsonTemplateV1, ToolPackageManifestContractError> {
        match self {
            Self::Literal { value } => Ok(HttpJsonTemplateV1::Literal { value }),
            Self::Argument { source } => Ok(HttpJsonTemplateV1::Argument { source }),
            Self::Array { items } => Ok(HttpJsonTemplateV1::Array {
                items: items
                    .into_iter()
                    .map(Self::into_canonical)
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            Self::Object { properties } => {
                Ok(HttpJsonTemplateV1::Object { properties: properties.into_canonical()? })
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RawToolPackageDeclarativeHttpBodyV1 {
    None,
    ApplicationJson { template: RawToolPackageHttpJsonTemplateV1 },
}

impl RawToolPackageDeclarativeHttpBodyV1 {
    fn into_canonical(self) -> Result<DeclarativeHttpBodyV1, ToolPackageManifestContractError> {
        match self {
            Self::None => Ok(DeclarativeHttpBodyV1::None),
            Self::ApplicationJson { template } => {
                Ok(DeclarativeHttpBodyV1::ApplicationJson { template: template.into_canonical()? })
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawToolPackageDeclarativeHttpV1 {
    pub target: DeclarativeHttpTargetV1,
    pub method: DeclarativeHttpMethodV1,
    pub path: Vec<HttpPathSegmentV1>,
    pub query: Vec<HttpQueryEntryV1>,
    pub headers: Vec<HttpHeaderEntryV1>,
    pub body: RawToolPackageDeclarativeHttpBodyV1,
    pub credential_slot: Option<HttpCredentialSlotV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success_status_ranges: Option<ToolPackageHttpSuccessStatusRangesV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalToolPackageDeclarativeHttpV1 {
    pub target: DeclarativeHttpTargetV1,
    pub method: DeclarativeHttpMethodV1,
    pub path: Vec<HttpPathSegmentV1>,
    pub query: Vec<HttpQueryEntryV1>,
    pub headers: Vec<HttpHeaderEntryV1>,
    pub body: DeclarativeHttpBodyV1,
    #[serde(deserialize_with = "deserialize_canonical_http_credential_slot")]
    #[schemars(required)]
    pub credential_slot: CanonicalHttpCredentialSlotV1,
    pub success_status_ranges: ToolPackageHttpSuccessStatusRangesV1,
}

/// Raw Package Runtime union. External MCP/Plugin sources cannot be declared
/// by an imported Package and therefore have no variant here.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RawToolPackageRuntimeV1 {
    DeclarativeCommand {
        command: RawToolPackageDeclarativeCommandV1,
    },
    AuraStdio {
        protocol_version: ToolPackageStdioProtocolVersionV1,
        executable: PackageRelativePath,
        arguments: Vec<ToolPackageCommandArgumentV1>,
        working_directory: CommandWorkingDirectoryV1,
        environment: Vec<CommandEnvironmentEntryV1>,
        credential_slot: Option<CommandCredentialEnvironmentSlotV1>,
    },
    DeclarativeHttp {
        request: RawToolPackageDeclarativeHttpV1,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CanonicalToolPackageRuntimeV1 {
    DeclarativeCommand {
        command: CanonicalToolPackageDeclarativeCommandV1,
    },
    AuraStdio {
        protocol_version: ToolPackageStdioProtocolVersionV1,
        executable: PackageRelativePath,
        arguments: Vec<ToolPackageCommandArgumentV1>,
        working_directory: CommandWorkingDirectoryV1,
        environment: Vec<CommandEnvironmentEntryV1>,
        #[serde(deserialize_with = "deserialize_canonical_command_credential_slot")]
        #[schemars(required)]
        credential_slot: CanonicalCommandCredentialSlotV1,
    },
    DeclarativeHttp {
        request: CanonicalToolPackageDeclarativeHttpV1,
    },
}

impl CanonicalToolPackageRuntimeV1 {
    /// Lossless bridge from the canonical Package wire to the shared runtime
    /// descriptor consumed by both installed execution and W3 Conformance.
    /// Keeping this conversion beside the canonical type prevents the two
    /// execution paths from drifting in argument or Credential semantics.
    #[must_use]
    pub fn to_runtime_descriptor_v1(&self) -> ToolRuntimeDescriptorV1 {
        match self {
            Self::DeclarativeCommand { command } => {
                let command = match command {
                    CanonicalToolPackageDeclarativeCommandV1::Argv {
                        executable,
                        arguments,
                        working_directory,
                        environment,
                        credential_slot,
                        success_exit_codes,
                    } => DeclarativeCommandV1::Argv {
                        executable: executable.clone(),
                        arguments: arguments.iter().map(command_argument_v1).collect(),
                        working_directory: working_directory.clone(),
                        environment: environment.clone(),
                        credential_slot: credential_slot.0.clone(),
                        success_exit_codes: success_exit_codes.as_slice().to_vec(),
                    },
                    CanonicalToolPackageDeclarativeCommandV1::PowershellScript {
                        script,
                        arguments,
                        working_directory,
                        environment,
                        credential_slot,
                        success_exit_codes,
                    } => DeclarativeCommandV1::PowershellScript {
                        script: script.clone(),
                        arguments: arguments.iter().map(command_argument_v1).collect(),
                        working_directory: working_directory.clone(),
                        environment: environment.clone(),
                        credential_slot: credential_slot.0.clone(),
                        success_exit_codes: success_exit_codes.as_slice().to_vec(),
                    },
                };
                ToolRuntimeDescriptorV1::DeclarativeCommand { command }
            }
            Self::AuraStdio {
                executable,
                arguments,
                working_directory,
                environment,
                credential_slot,
                ..
            } => ToolRuntimeDescriptorV1::AuraStdio {
                executable: executable.clone(),
                arguments: arguments.iter().map(command_argument_v1).collect(),
                working_directory: working_directory.clone(),
                environment: environment.clone(),
                credential_slot: credential_slot.0.clone(),
            },
            Self::DeclarativeHttp { request } => ToolRuntimeDescriptorV1::DeclarativeHttp {
                request: DeclarativeHttpV1 {
                    target: request.target.clone(),
                    method: request.method,
                    path: request.path.clone(),
                    query: request.query.clone(),
                    headers: request.headers.clone(),
                    body: request.body.clone(),
                    credential_slot: request.credential_slot.0.clone(),
                    success_status_ranges: request.success_status_ranges.as_slice().to_vec(),
                },
            },
        }
    }
}

fn command_argument_v1(value: &ToolPackageCommandArgumentV1) -> CommandArgumentV1 {
    match value {
        ToolPackageCommandArgumentV1::Literal { value } => {
            CommandArgumentV1::Literal { value: value.clone() }
        }
        ToolPackageCommandArgumentV1::Scalar { source } => {
            CommandArgumentV1::Scalar { source: source.clone() }
        }
        ToolPackageCommandArgumentV1::Repeated { source } => {
            CommandArgumentV1::Repeated { source: source.clone() }
        }
        ToolPackageCommandArgumentV1::ConditionalFlag { source, flag } => {
            CommandArgumentV1::ConditionalFlag { source: source.clone(), flag: flag.clone() }
        }
        ToolPackageCommandArgumentV1::NamedOption { name, source, omit_when_missing_or_null } => {
            CommandArgumentV1::NamedOption {
                name: name.clone(),
                source: source.clone(),
                omit_when_missing_or_null: *omit_when_missing_or_null,
            }
        }
    }
}

/// A V1 Package conformance case is deliberately narrow: it supplies one
/// Package-argument object to the owning Tool. Result interpretation remains
/// governed by that Tool's declared Result Contract; this draft does not add a
/// second assertion language.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageConformanceCaseV1 {
    pub name: ToolPackageConformanceCaseNameV1,
    pub arguments: ToolPackageConformanceArgumentsV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawToolPackageToolV1 {
    pub tool_key: ToolKey,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_version: Option<CanonicalSemVer>,
    pub ui: ToolPackageToolUiV1,
    pub model: RawToolPackageModelContractV1,
    #[serde(deserialize_with = "deserialize_raw_instance_configuration")]
    #[schemars(required)]
    pub instance_configuration: RawToolPackageInstanceConfigurationSchemaV1,
    pub runtime: RawToolPackageRuntimeV1,
    pub capability_requirements: Vec<PackageCapabilityRequirementV1>,
    pub resource_claims: Vec<PackageDeclaredResourceClaimV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<PackageToolAudienceV1>,
    pub execution_semantics: ToolExecutionSemanticsV1,
    #[schemars(length(min = 1))]
    #[serde(deserialize_with = "deserialize_non_empty_vec")]
    pub conformance_cases: Vec<ToolPackageConformanceCaseV1>,
}

/// A required nullable field used by the Canonical contract. Unlike an
/// `Option` struct field, omission is rejected while explicit JSON `null`
/// represents the frozen `semantic_version=None` value.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CanonicalToolSemanticVersionV1(pub Option<CanonicalSemVer>);

nullable_wrapper_schema!(
    CanonicalToolSemanticVersionV1,
    CanonicalSemVer,
    "CanonicalToolSemanticVersionV1"
);

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalToolPackageToolV1 {
    pub tool_key: ToolKey,
    #[serde(deserialize_with = "deserialize_canonical_semantic_version")]
    pub semantic_version: CanonicalToolSemanticVersionV1,
    pub ui: ToolPackageToolUiV1,
    pub model: CanonicalToolPackageModelContractV1,
    #[serde(deserialize_with = "deserialize_canonical_instance_configuration")]
    #[schemars(required)]
    pub instance_configuration: CanonicalToolPackageInstanceConfigurationSchemaV1,
    pub runtime: CanonicalToolPackageRuntimeV1,
    pub capability_requirements: Vec<PackageCapabilityRequirementV1>,
    pub resource_claims: Vec<PackageDeclaredResourceClaimV1>,
    pub audience: PackageToolAudienceV1,
    pub execution_semantics: ToolExecutionSemanticsV1,
    #[schemars(length(min = 1))]
    #[serde(deserialize_with = "deserialize_non_empty_vec")]
    pub conformance_cases: Vec<ToolPackageConformanceCaseV1>,
}

fn deserialize_canonical_semantic_version<'de, D>(
    deserializer: D,
) -> Result<CanonicalToolSemanticVersionV1, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<CanonicalSemVer>::deserialize(deserializer).map(CanonicalToolSemanticVersionV1)
}

/// The file representation authored by a Package producer. This type is a
/// contract only; P07-DG-001 forbids wiring it to file parsing or installation
/// before the project owner reviews the generated draft.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawToolPackageManifestV1 {
    pub schema_version: ToolPackageManifestSchemaVersionV1,
    pub package_key: PackageKey,
    pub package_version: CanonicalSemVer,
    pub minimum_aura_version: CanonicalSemVer,
    pub ui: RawToolPackageUiV1,
    #[schemars(length(max = 4_095))]
    pub files: Vec<ToolPackageFileEntryV1>,
    #[schemars(length(min = 1))]
    #[serde(deserialize_with = "deserialize_non_empty_vec")]
    pub tools: Vec<RawToolPackageToolV1>,
}

/// Normalized contract used as the future fingerprint input. Defaults are
/// explicit and schemas are already represented by their canonical P07 forms.
/// The pure conversion method is Contract-only and is not wired to a file
/// parser, Package Store, Trust, Conformance, Journal, or Eligible Registry.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalToolPackageManifestV1 {
    pub schema_version: ToolPackageManifestSchemaVersionV1,
    pub package_key: PackageKey,
    pub package_version: CanonicalSemVer,
    pub minimum_aura_version: CanonicalSemVer,
    pub ui: CanonicalToolPackageUiV1,
    #[schemars(length(max = 4_095))]
    pub files: Vec<ToolPackageFileEntryV1>,
    #[schemars(length(min = 1))]
    #[serde(deserialize_with = "deserialize_non_empty_vec")]
    pub tools: Vec<CanonicalToolPackageToolV1>,
}

impl RawToolPackageManifestV1 {
    pub fn validate(&self) -> Result<(), ToolPackageManifestContractError> {
        validate_raw_manifest_collections(self)?;
        for tool in &self.tools {
            validate_raw_instance_configuration(tool)?;
            validate_raw_runtime_contract(&tool.runtime)?;
        }
        Ok(())
    }

    /// Revalidates the complete Raw contract while including the exact source
    /// Manifest length in the fixed expanded-package byte limit.
    pub fn validate_with_source_byte_length(
        &self,
        manifest_source_byte_length: usize,
    ) -> Result<(), ToolPackageManifestContractError> {
        self.validate()?;
        validate_manifest_files(&self.files, false, Some(manifest_source_byte_length))
    }

    /// Performs the frozen, deterministic Contract transformation without
    /// reading or mutating Package storage. Production import wiring remains
    /// outside this Owner Review gate.
    pub fn into_canonical_contract(
        self,
    ) -> Result<CanonicalToolPackageManifestV1, ToolPackageManifestContractError> {
        self.validate()?;
        let Self {
            schema_version,
            package_key,
            package_version,
            minimum_aura_version,
            ui,
            mut files,
            tools,
        } = self;

        files.sort_by(|left, right| {
            left.path.as_str().as_bytes().cmp(right.path.as_str().as_bytes())
        });
        let mut canonical_tools = tools
            .into_iter()
            .map(|tool| canonicalize_tool(&package_key, tool))
            .collect::<Result<Vec<_>, _>>()?;
        canonical_tools.sort_by(|left, right| {
            left.tool_key.as_str().as_bytes().cmp(right.tool_key.as_str().as_bytes())
        });

        let description = ui
            .description
            .map(|value| ToolPackageUiDescriptionV1::new(value.as_str().to_owned()))
            .transpose()?;
        let canonical = CanonicalToolPackageManifestV1 {
            schema_version,
            package_key,
            package_version,
            minimum_aura_version,
            ui: CanonicalToolPackageUiV1 {
                display_name: ui.display_name,
                description: CanonicalToolPackageUiDescriptionV1(description),
            },
            files,
            tools: canonical_tools,
        };
        canonical.validate()?;
        Ok(canonical)
    }
}

fn canonicalize_tool(
    package_key: &PackageKey,
    tool: RawToolPackageToolV1,
) -> Result<CanonicalToolPackageToolV1, ToolPackageManifestContractError> {
    let mut capabilities = tool.capability_requirements;
    capabilities.sort_by_key(|capability| package_capability_wire(*capability));
    let mut resource_claims = tool.resource_claims;
    resource_claims.sort_by(|left, right| {
        left.claim_key().as_str().as_bytes().cmp(right.claim_key().as_str().as_bytes())
    });
    let mut conformance_cases = tool.conformance_cases;
    conformance_cases
        .sort_by(|left, right| left.name.as_str().as_bytes().cmp(right.name.as_str().as_bytes()));

    let canonical_name = ToolCanonicalName::new(format!(
        "user.{}.{}",
        package_key.as_str(),
        tool.model.canonical_suffix.as_str()
    ))
    .map_err(|_| ToolPackageManifestContractError::InvalidCanonicalSuffix)?;
    let input_schema = ToolInputSchemaV1::new(tool.model.input_schema.value().clone())
        .map_err(|_| ToolPackageManifestContractError::InvalidSchemaContract)?;
    let result_contract = match tool.model.result_contract {
        RawToolPackageResultContractV1::Unit => ToolResultContractV1::Unit,
        RawToolPackageResultContractV1::Utf8Text => ToolResultContractV1::Utf8Text,
        RawToolPackageResultContractV1::StructuredJson { result_schema } => {
            ToolResultContractV1::StructuredJson {
                result_schema: ToolStructuredResultSchemaV1::new(result_schema.value().clone())
                    .map_err(|_| ToolPackageManifestContractError::InvalidSchemaContract)?,
            }
        }
    };
    let instance_configuration = tool
        .instance_configuration
        .0
        .map(|schema| {
            ToolInputSchemaV1::new(schema.value().clone())
                .map_err(|_| ToolPackageManifestContractError::InvalidSchemaContract)
        })
        .transpose()?;

    Ok(CanonicalToolPackageToolV1 {
        tool_key: tool.tool_key,
        semantic_version: CanonicalToolSemanticVersionV1(tool.semantic_version),
        ui: tool.ui,
        model: CanonicalToolPackageModelContractV1 {
            canonical_name,
            description: ToolPackageModelDescriptionV1::new(
                tool.model.description.as_str().to_owned(),
            )?,
            summary: tool.model.summary,
            input_schema,
            strict: tool.model.strict,
            result_contract,
        },
        instance_configuration: CanonicalToolPackageInstanceConfigurationSchemaV1(
            instance_configuration,
        ),
        runtime: canonicalize_runtime(tool.runtime)?,
        capability_requirements: capabilities,
        resource_claims,
        audience: tool.audience.unwrap_or(DEFAULT_PACKAGE_TOOL_AUDIENCE_V1),
        execution_semantics: tool.execution_semantics,
        conformance_cases,
    })
}

fn canonicalize_runtime(
    runtime: RawToolPackageRuntimeV1,
) -> Result<CanonicalToolPackageRuntimeV1, ToolPackageManifestContractError> {
    match runtime {
        RawToolPackageRuntimeV1::DeclarativeCommand { command } => {
            let command = match command {
                RawToolPackageDeclarativeCommandV1::Argv {
                    executable,
                    arguments,
                    working_directory,
                    environment,
                    credential_slot,
                    success_exit_codes,
                } => CanonicalToolPackageDeclarativeCommandV1::Argv {
                    executable,
                    arguments,
                    working_directory,
                    environment,
                    credential_slot: CanonicalCommandCredentialSlotV1(credential_slot),
                    success_exit_codes: success_exit_codes
                        .unwrap_or_else(ToolPackageCommandSuccessExitCodesV1::default_v1),
                },
                RawToolPackageDeclarativeCommandV1::PowershellScript {
                    script,
                    arguments,
                    working_directory,
                    environment,
                    credential_slot,
                    success_exit_codes,
                } => CanonicalToolPackageDeclarativeCommandV1::PowershellScript {
                    script,
                    arguments,
                    working_directory,
                    environment,
                    credential_slot: CanonicalCommandCredentialSlotV1(credential_slot),
                    success_exit_codes: success_exit_codes
                        .unwrap_or_else(ToolPackageCommandSuccessExitCodesV1::default_v1),
                },
            };
            Ok(CanonicalToolPackageRuntimeV1::DeclarativeCommand { command })
        }
        RawToolPackageRuntimeV1::AuraStdio {
            protocol_version,
            executable,
            arguments,
            working_directory,
            environment,
            credential_slot,
        } => Ok(CanonicalToolPackageRuntimeV1::AuraStdio {
            protocol_version,
            executable,
            arguments,
            working_directory,
            environment,
            credential_slot: CanonicalCommandCredentialSlotV1(credential_slot),
        }),
        RawToolPackageRuntimeV1::DeclarativeHttp { request } => {
            Ok(CanonicalToolPackageRuntimeV1::DeclarativeHttp {
                request: CanonicalToolPackageDeclarativeHttpV1 {
                    target: request.target,
                    method: request.method,
                    path: request.path,
                    query: request.query,
                    headers: request.headers,
                    body: request.body.into_canonical()?,
                    credential_slot: CanonicalHttpCredentialSlotV1(request.credential_slot),
                    success_status_ranges: request
                        .success_status_ranges
                        .unwrap_or_else(ToolPackageHttpSuccessStatusRangesV1::default_v1),
                },
            })
        }
    }
}

impl<'de> Deserialize<'de> for RawToolPackageManifestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireManifest {
            schema_version: ToolPackageManifestSchemaVersionV1,
            package_key: PackageKey,
            package_version: CanonicalSemVer,
            minimum_aura_version: CanonicalSemVer,
            ui: RawToolPackageUiV1,
            files: Vec<ToolPackageFileEntryV1>,
            #[serde(deserialize_with = "deserialize_non_empty_vec")]
            tools: Vec<RawToolPackageToolV1>,
        }

        let wire = WireManifest::deserialize(deserializer)?;
        let manifest = Self {
            schema_version: wire.schema_version,
            package_key: wire.package_key,
            package_version: wire.package_version,
            minimum_aura_version: wire.minimum_aura_version,
            ui: wire.ui,
            files: wire.files,
            tools: wire.tools,
        };
        manifest.validate().map_err(D::Error::custom)?;
        Ok(manifest)
    }
}

impl CanonicalToolPackageManifestV1 {
    pub fn validate(&self) -> Result<(), ToolPackageManifestContractError> {
        validate_canonical_manifest_collections(self)?;
        for tool in &self.tools {
            validate_canonical_instance_configuration(tool)?;
            validate_canonical_runtime_contract(&tool.runtime)?;
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for CanonicalToolPackageManifestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireManifest {
            schema_version: ToolPackageManifestSchemaVersionV1,
            package_key: PackageKey,
            package_version: CanonicalSemVer,
            minimum_aura_version: CanonicalSemVer,
            ui: CanonicalToolPackageUiV1,
            files: Vec<ToolPackageFileEntryV1>,
            #[serde(deserialize_with = "deserialize_non_empty_vec")]
            tools: Vec<CanonicalToolPackageToolV1>,
        }

        let wire = WireManifest::deserialize(deserializer)?;
        let manifest = Self {
            schema_version: wire.schema_version,
            package_key: wire.package_key,
            package_version: wire.package_version,
            minimum_aura_version: wire.minimum_aura_version,
            ui: wire.ui,
            files: wire.files,
            tools: wire.tools,
        };
        manifest.validate().map_err(D::Error::custom)?;
        Ok(manifest)
    }
}

fn validate_raw_manifest_collections(
    manifest: &RawToolPackageManifestV1,
) -> Result<(), ToolPackageManifestContractError> {
    validate_manifest_files(&manifest.files, false, None)?;

    let mut tool_keys = BTreeSet::new();
    let mut canonical_names = BTreeSet::new();
    for tool in &manifest.tools {
        if !tool_keys.insert(tool.tool_key.as_str()) {
            return Err(ToolPackageManifestContractError::DuplicateCollectionItem);
        }
        let full_name = format!(
            "user.{}.{}",
            manifest.package_key.as_str(),
            tool.model.canonical_suffix.as_str()
        );
        let canonical_name = ToolCanonicalName::new(full_name)
            .map_err(|_| ToolPackageManifestContractError::InvalidCanonicalSuffix)?;
        if !canonical_names.insert(canonical_name) {
            return Err(ToolPackageManifestContractError::DuplicateToolCanonicalName);
        }
        validate_unordered_tool_collections(
            &tool.capability_requirements,
            &tool.resource_claims,
            &tool.conformance_cases,
        )?;
    }
    Ok(())
}

fn validate_canonical_manifest_collections(
    manifest: &CanonicalToolPackageManifestV1,
) -> Result<(), ToolPackageManifestContractError> {
    validate_manifest_files(&manifest.files, true, None)?;
    if !strictly_ordered_by(&manifest.tools, |tool| tool.tool_key.as_str().as_bytes()) {
        return Err(ToolPackageManifestContractError::InvalidCollectionOrder);
    }

    let required_prefix = format!("user.{}.", manifest.package_key.as_str());
    let mut canonical_names = BTreeSet::new();
    for tool in &manifest.tools {
        if !tool.model.canonical_name.as_str().starts_with(&required_prefix) {
            return Err(ToolPackageManifestContractError::InvalidCanonicalSuffix);
        }
        if !canonical_names.insert(tool.model.canonical_name.as_str()) {
            return Err(ToolPackageManifestContractError::DuplicateToolCanonicalName);
        }
        if !tool.capability_requirements.windows(2).all(|pair| {
            package_capability_wire(pair[0]).as_bytes()
                < package_capability_wire(pair[1]).as_bytes()
        }) || !strictly_ordered_by(&tool.resource_claims, |claim| {
            claim.claim_key().as_str().as_bytes()
        }) || !strictly_ordered_by(&tool.conformance_cases, |case| case.name.as_str().as_bytes())
        {
            return Err(ToolPackageManifestContractError::InvalidCollectionOrder);
        }
        validate_duplicate_claim_content(&tool.resource_claims)?;
    }
    Ok(())
}

fn validate_manifest_files(
    files: &[ToolPackageFileEntryV1],
    require_canonical_order: bool,
    manifest_source_byte_length: Option<usize>,
) -> Result<(), ToolPackageManifestContractError> {
    if files.len() > MAX_TOOL_PACKAGE_DECLARED_FILES_V1
        || (require_canonical_order
            && !strictly_ordered_by(files, |file| file.path.as_str().as_bytes()))
    {
        return Err(if require_canonical_order {
            ToolPackageManifestContractError::InvalidCollectionOrder
        } else {
            ToolPackageManifestContractError::InvalidFilePolicy
        });
    }

    let mut exact_paths = BTreeSet::new();
    let mut total = 0_u64;
    for (index, file) in files.iter().enumerate() {
        if !exact_paths.insert(file.path.as_str()) {
            return Err(ToolPackageManifestContractError::DuplicateCollectionItem);
        }
        if windows_paths_equal(file.path.as_str(), TOOL_PACKAGE_MANIFEST_NAME)? {
            return Err(ToolPackageManifestContractError::ManifestListedAsFile);
        }
        for previous in &files[..index] {
            if windows_paths_equal(file.path.as_str(), previous.path.as_str())? {
                return Err(ToolPackageManifestContractError::WindowsPathCollision);
            }
        }
        if file.byte_length > TOOL_PACKAGE_IMPORT_POLICY_V1.max_file_bytes {
            return Err(ToolPackageManifestContractError::InvalidFilePolicy);
        }
        total = total
            .checked_add(file.byte_length)
            .ok_or(ToolPackageManifestContractError::InvalidFilePolicy)?;
    }

    if let Some(manifest_length) = manifest_source_byte_length {
        let manifest_length = u64::try_from(manifest_length)
            .map_err(|_| ToolPackageManifestContractError::InvalidFilePolicy)?;
        if manifest_length > TOOL_PACKAGE_IMPORT_POLICY_V1.max_manifest_bytes {
            return Err(ToolPackageManifestContractError::InvalidFilePolicy);
        }
        total = total
            .checked_add(manifest_length)
            .ok_or(ToolPackageManifestContractError::InvalidFilePolicy)?;
    }
    if total > TOOL_PACKAGE_IMPORT_POLICY_V1.max_expanded_bytes {
        return Err(ToolPackageManifestContractError::InvalidFilePolicy);
    }
    Ok(())
}

fn windows_paths_equal(left: &str, right: &str) -> Result<bool, ToolPackageManifestContractError> {
    windows_ordinal_compare_ignore_case(OsStr::new(left), OsStr::new(right))
        .map(Ordering::is_eq)
        .map_err(|_| ToolPackageManifestContractError::WindowsPathCollision)
}

const fn package_capability_wire(capability: PackageCapabilityRequirementV1) -> &'static str {
    match capability {
        PackageCapabilityRequirementV1::WorkspaceFileRead => "workspace_file_read",
        PackageCapabilityRequirementV1::WorkspaceFileWrite => "workspace_file_write",
        PackageCapabilityRequirementV1::WorkspaceFileCreate => "workspace_file_create",
        PackageCapabilityRequirementV1::WorkspaceFileDelete => "workspace_file_delete",
        PackageCapabilityRequirementV1::ExternalFileRead => "external_file_read",
        PackageCapabilityRequirementV1::ExternalFileWrite => "external_file_write",
        PackageCapabilityRequirementV1::ExternalFileCreate => "external_file_create",
        PackageCapabilityRequirementV1::ExternalFileDelete => "external_file_delete",
        PackageCapabilityRequirementV1::ProcessExecute => "process_execute",
        PackageCapabilityRequirementV1::GitRead => "git_read",
        PackageCapabilityRequirementV1::GitWrite => "git_write",
        PackageCapabilityRequirementV1::GitRemoteRead => "git_remote_read",
        PackageCapabilityRequirementV1::GitRemoteWrite => "git_remote_write",
        PackageCapabilityRequirementV1::GitDestructive => "git_destructive",
        PackageCapabilityRequirementV1::PublicNetworkRead => "public_network_read",
        PackageCapabilityRequirementV1::PrivateNetworkAccess => "private_network_access",
        PackageCapabilityRequirementV1::DataEgress => "data_egress",
        PackageCapabilityRequirementV1::DownloadArtifact => "download_artifact",
        PackageCapabilityRequirementV1::DownloadWindowsExecutable => "download_windows_executable",
        PackageCapabilityRequirementV1::ExternalServiceRead => "external_service_read",
        PackageCapabilityRequirementV1::ExternalServiceMutation => "external_service_mutation",
        PackageCapabilityRequirementV1::ExternalServiceDestructiveMutation => {
            "external_service_destructive_mutation"
        }
    }
}

fn strictly_ordered_by<'a, T, F>(values: &'a [T], key: F) -> bool
where
    F: Fn(&'a T) -> &'a [u8],
{
    values.windows(2).all(|pair| key(&pair[0]) < key(&pair[1]))
}

fn jcs_property_name_cmp(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn validate_unordered_tool_collections(
    capabilities: &[PackageCapabilityRequirementV1],
    claims: &[PackageDeclaredResourceClaimV1],
    cases: &[ToolPackageConformanceCaseV1],
) -> Result<(), ToolPackageManifestContractError> {
    if capabilities.iter().collect::<BTreeSet<_>>().len() != capabilities.len() {
        return Err(ToolPackageManifestContractError::DuplicateCollectionItem);
    }
    if claims.iter().map(|claim| claim.claim_key()).collect::<BTreeSet<_>>().len() != claims.len()
        || cases.iter().map(|case| &case.name).collect::<BTreeSet<_>>().len() != cases.len()
    {
        return Err(ToolPackageManifestContractError::DuplicateCollectionItem);
    }
    validate_duplicate_claim_content(claims)
}

fn validate_duplicate_claim_content(
    claims: &[PackageDeclaredResourceClaimV1],
) -> Result<(), ToolPackageManifestContractError> {
    let mut contents = BTreeSet::new();
    for claim in claims {
        let mut value = serde_json::to_value(claim)
            .map_err(|_| ToolPackageManifestContractError::DuplicateCollectionItem)?;
        if let Some(object) = value.as_object_mut() {
            object.remove("claim_key");
        }
        let bytes = canonical_json_jcs(&value)
            .map_err(|_| ToolPackageManifestContractError::DuplicateCollectionItem)?;
        if !contents.insert(bytes) {
            return Err(ToolPackageManifestContractError::DuplicateCollectionItem);
        }
    }
    Ok(())
}

fn validate_raw_instance_configuration(
    tool: &RawToolPackageToolV1,
) -> Result<(), ToolPackageManifestContractError> {
    validate_instance_configuration_references(
        tool.instance_configuration.0.as_ref().map(RawToolJsonSchemaV1::value),
        raw_runtime_environment(&tool.runtime),
    )
}

fn validate_canonical_instance_configuration(
    tool: &CanonicalToolPackageToolV1,
) -> Result<(), ToolPackageManifestContractError> {
    validate_instance_configuration_references(
        tool.instance_configuration.0.as_ref().map(ToolInputSchemaV1::document),
        canonical_runtime_environment(&tool.runtime),
    )
}

fn validate_raw_runtime_contract(
    runtime: &RawToolPackageRuntimeV1,
) -> Result<(), ToolPackageManifestContractError> {
    match runtime {
        RawToolPackageRuntimeV1::DeclarativeCommand { command } => match command {
            RawToolPackageDeclarativeCommandV1::Argv { environment, credential_slot, .. }
            | RawToolPackageDeclarativeCommandV1::PowershellScript {
                environment,
                credential_slot,
                ..
            } => validate_command_environment_v1(environment, credential_slot.as_ref()),
        },
        RawToolPackageRuntimeV1::AuraStdio { environment, credential_slot, .. } => {
            validate_command_environment_v1(environment, credential_slot.as_ref())
        }
        RawToolPackageRuntimeV1::DeclarativeHttp { request } => {
            validate_http_header_bindings_v1(&request.headers, request.credential_slot.as_ref())
        }
    }
    .map_err(map_runtime_contract_error)
}

fn validate_canonical_runtime_contract(
    runtime: &CanonicalToolPackageRuntimeV1,
) -> Result<(), ToolPackageManifestContractError> {
    match runtime {
        CanonicalToolPackageRuntimeV1::DeclarativeCommand { command } => match command {
            CanonicalToolPackageDeclarativeCommandV1::Argv {
                environment, credential_slot, ..
            }
            | CanonicalToolPackageDeclarativeCommandV1::PowershellScript {
                environment,
                credential_slot,
                ..
            } => validate_command_environment_v1(environment, credential_slot.0.as_ref()),
        },
        CanonicalToolPackageRuntimeV1::AuraStdio { environment, credential_slot, .. } => {
            validate_command_environment_v1(environment, credential_slot.0.as_ref())
        }
        CanonicalToolPackageRuntimeV1::DeclarativeHttp { request } => {
            validate_http_header_bindings_v1(&request.headers, request.credential_slot.0.as_ref())
        }
    }
    .map_err(map_runtime_contract_error)
}

fn map_runtime_contract_error(error: ToolRuntimeContractError) -> ToolPackageManifestContractError {
    match error {
        ToolRuntimeContractError::DuplicateEnvironmentName => {
            ToolPackageManifestContractError::DuplicateEnvironmentName
        }
        ToolRuntimeContractError::CredentialEnvironmentConflict => {
            ToolPackageManifestContractError::CredentialEnvironmentConflict
        }
        ToolRuntimeContractError::DuplicateHttpHeader => {
            ToolPackageManifestContractError::DuplicateHttpHeader
        }
        ToolRuntimeContractError::CredentialHttpHeaderConflict => {
            ToolPackageManifestContractError::CredentialHttpHeaderConflict
        }
        _ => ToolPackageManifestContractError::InvalidRuntimeContract,
    }
}

fn raw_runtime_environment(runtime: &RawToolPackageRuntimeV1) -> &[CommandEnvironmentEntryV1] {
    match runtime {
        RawToolPackageRuntimeV1::DeclarativeCommand { command } => match command {
            RawToolPackageDeclarativeCommandV1::Argv { environment, .. }
            | RawToolPackageDeclarativeCommandV1::PowershellScript { environment, .. } => {
                environment
            }
        },
        RawToolPackageRuntimeV1::AuraStdio { environment, .. } => environment,
        RawToolPackageRuntimeV1::DeclarativeHttp { .. } => &[],
    }
}

fn canonical_runtime_environment(
    runtime: &CanonicalToolPackageRuntimeV1,
) -> &[CommandEnvironmentEntryV1] {
    match runtime {
        CanonicalToolPackageRuntimeV1::DeclarativeCommand { command } => match command {
            CanonicalToolPackageDeclarativeCommandV1::Argv { environment, .. }
            | CanonicalToolPackageDeclarativeCommandV1::PowershellScript { environment, .. } => {
                environment
            }
        },
        CanonicalToolPackageRuntimeV1::AuraStdio { environment, .. } => environment,
        CanonicalToolPackageRuntimeV1::DeclarativeHttp { .. } => &[],
    }
}

fn validate_instance_configuration_references(
    schema: Option<&Value>,
    environment: &[CommandEnvironmentEntryV1],
) -> Result<(), ToolPackageManifestContractError> {
    for entry in environment {
        let aura_tool_contract::CommandEnvironmentValueV1::InstanceConfiguration { key } =
            &entry.value
        else {
            continue;
        };
        let Some(schema) = schema else {
            return Err(ToolPackageManifestContractError::InvalidInstanceConfigurationReference);
        };
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .ok_or(ToolPackageManifestContractError::InvalidInstanceConfigurationReference)?;
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .ok_or(ToolPackageManifestContractError::InvalidInstanceConfigurationReference)?;
        let property = properties
            .get(key.as_str())
            .and_then(Value::as_object)
            .ok_or(ToolPackageManifestContractError::InvalidInstanceConfigurationReference)?;
        let required = required.iter().any(|value| value.as_str() == Some(key.as_str()));
        let scalar = property
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| matches!(value, "string" | "integer" | "number" | "boolean"));
        let indirect_keywords =
            ["$ref", "$dynamicRef", "allOf", "anyOf", "oneOf", "not", "if", "then", "else"];
        let direct = indirect_keywords.iter().all(|keyword| !property.contains_key(*keyword));
        if !required || !scalar || !direct {
            return Err(ToolPackageManifestContractError::InvalidInstanceConfigurationReference);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CanonicalToolPackageManifestV1, PackageToolAudienceV1, RawToolPackageManifestV1,
        ToolPackageCommandSuccessExitCodesV1, ToolPackageHttpSuccessStatusRangesV1,
        ToolPackageManifestSchemaVersionV1, ToolPackageModelDescriptionV1,
        ToolPackageModelSummaryV1,
    };

    #[test]
    fn schema_version_is_the_numeric_literal_one() {
        assert_eq!(
            serde_json::to_value(ToolPackageManifestSchemaVersionV1)
                .expect("serialize schema version"),
            json!(1)
        );
        serde_json::from_value::<ToolPackageManifestSchemaVersionV1>(json!(1))
            .expect("version one");
        assert!(serde_json::from_value::<ToolPackageManifestSchemaVersionV1>(json!(2)).is_err());
    }

    #[test]
    fn raw_manifest_recursively_rejects_unknown_fields_and_internal_ids() {
        let base = json!({
            "schema_version": 1,
            "package_key": "org.example.fixture",
            "package_version": "1.0.0",
            "minimum_aura_version": "0.1.0",
            "ui": { "display_name": "Fixture Package", "description": null },
            "files": [],
            "tools": [{
                "tool_key": "echo",
                "ui": { "display_name": "Echo" },
                "model": {
                    "canonical_suffix": "echo",
                    "description": "Echo a value.",
                    "summary": "Echo a value.",
                    "input_schema": { "type": "object" },
                    "strict": true,
                    "result_contract": { "type": "unit" }
                },
                "instance_configuration": null,
                "runtime": {
                    "type": "aura_stdio",
                    "protocol_version": 1,
                    "executable": "bin/echo.exe",
                    "arguments": [],
                    "working_directory": { "type": "package_root" },
                    "environment": [],
                    "credential_slot": null
                },
                "capability_requirements": ["process_execute"],
                "resource_claims": [],
                "execution_semantics": {
                    "automatic_retry_allowed": false,
                    "repeat_is_idempotent": true,
                    "may_create_persistent_effects": false
                },
                "conformance_cases": [{ "name": "empty", "arguments": {} }]
            }]
        });
        serde_json::from_value::<RawToolPackageManifestV1>(base.clone()).expect("valid raw draft");

        let mut root_unknown = base.clone();
        root_unknown["tool_package_id"] = json!("tpk_01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert!(serde_json::from_value::<RawToolPackageManifestV1>(root_unknown).is_err());

        let mut nested_unknown = base;
        nested_unknown["tools"][0]["ui"]["model_description"] = json!("wrong layer");
        assert!(serde_json::from_value::<RawToolPackageManifestV1>(nested_unknown).is_err());
    }

    #[test]
    fn canonical_manifest_requires_explicit_defaulted_fields() {
        let input_schema = aura_tool_contract::ToolInputSchemaV1::new(json!({
            "type": "object",
            "additionalProperties": false
        }))
        .expect("canonical input schema");
        let mut missing_semantic_version = json!({
            "schema_version": 1,
            "package_key": "org.example.fixture",
            "package_version": "1.0.0",
            "minimum_aura_version": "0.1.0",
            "ui": { "display_name": "Fixture Package", "description": null },
            "files": [],
            "tools": [{
                "tool_key": "echo",
                "ui": { "display_name": "Echo" },
                "model": {
                    "canonical_name": "user.org.example.fixture.echo",
                    "description": "Echo a value.",
                    "summary": "Echo a value.",
                    "input_schema": input_schema,
                    "strict": true,
                    "result_contract": { "type": "unit" }
                },
                "instance_configuration": null,
                "runtime": {
                    "type": "aura_stdio",
                    "protocol_version": 1,
                    "executable": "bin/echo.exe",
                    "arguments": [],
                    "working_directory": { "type": "package_root" },
                    "environment": [],
                    "credential_slot": null
                },
                "capability_requirements": ["process_execute"],
                "resource_claims": [],
                "audience": "subagent_configurable",
                "execution_semantics": {
                    "automatic_retry_allowed": false,
                    "repeat_is_idempotent": true,
                    "may_create_persistent_effects": false
                },
                "conformance_cases": [{ "name": "empty", "arguments": {} }]
            }]
        });
        missing_semantic_version["tools"][0]["semantic_version"] = json!(null);
        serde_json::from_value::<CanonicalToolPackageManifestV1>(missing_semantic_version.clone())
            .expect("complete canonical manifest");
        missing_semantic_version["tools"][0]
            .as_object_mut()
            .expect("tool object")
            .remove("semantic_version");
        assert!(
            serde_json::from_value::<CanonicalToolPackageManifestV1>(missing_semantic_version)
                .is_err()
        );
    }

    #[test]
    fn raw_manifest_requires_tools_cases_and_exact_stdio_protocol() {
        let empty_tools = json!({
            "schema_version": 1,
            "package_key": "org.example.fixture",
            "package_version": "1.0.0",
            "minimum_aura_version": "0.1.0",
            "ui": { "display_name": "Fixture Package", "description": null },
            "files": [],
            "tools": []
        });
        assert!(serde_json::from_value::<RawToolPackageManifestV1>(empty_tools).is_err());

        let wrong_protocol = json!({
            "schema_version": 1,
            "package_key": "org.example.fixture",
            "package_version": "1.0.0",
            "minimum_aura_version": "0.1.0",
            "ui": { "display_name": "Fixture Package", "description": null },
            "files": [],
            "tools": [{
                "tool_key": "echo",
                "ui": { "display_name": "Echo" },
                "model": {
                    "canonical_suffix": "echo",
                    "description": "Echo a value.",
                    "summary": "Echo a value.",
                    "input_schema": { "type": "object" },
                    "strict": true,
                    "result_contract": { "type": "unit" }
                },
                "instance_configuration": null,
                "runtime": {
                    "type": "aura_stdio",
                    "protocol_version": 2,
                    "executable": "bin/echo.exe",
                    "arguments": [],
                    "working_directory": { "type": "package_root" },
                    "environment": [],
                    "credential_slot": null
                },
                "capability_requirements": ["process_execute"],
                "resource_claims": [],
                "execution_semantics": {
                    "automatic_retry_allowed": false,
                    "repeat_is_idempotent": true,
                    "may_create_persistent_effects": false
                },
                "conformance_cases": [{ "name": "empty", "arguments": {} }]
            }]
        });
        assert!(serde_json::from_value::<RawToolPackageManifestV1>(wrong_protocol).is_err());
    }

    #[test]
    fn success_predicates_enforce_the_frozen_closed_ranges() {
        serde_json::from_value::<ToolPackageCommandSuccessExitCodesV1>(json!([0, 2]))
            .expect("ordered command codes");
        for invalid in [json!([]), json!([0, 0]), json!([2, 0])] {
            assert!(
                serde_json::from_value::<ToolPackageCommandSuccessExitCodesV1>(invalid).is_err()
            );
        }

        serde_json::from_value::<ToolPackageHttpSuccessStatusRangesV1>(json!([
            { "start": 200, "end": 204 },
            { "start": 206, "end": 206 }
        ]))
        .expect("ordered HTTP status ranges");
        for invalid in [
            json!([]),
            json!([{ "start": 99, "end": 200 }]),
            json!([
                { "start": 200, "end": 204 },
                { "start": 204, "end": 206 }
            ]),
        ] {
            assert!(
                serde_json::from_value::<ToolPackageHttpSuccessStatusRangesV1>(invalid).is_err()
            );
        }
    }

    #[test]
    fn model_text_and_external_audience_are_closed() {
        ToolPackageModelSummaryV1::new("A concise model-visible summary.").expect("valid summary");
        assert!(ToolPackageModelSummaryV1::new("line one\nline two").is_err());
        ToolPackageModelDescriptionV1::new("line one\nline two").expect("multiline description");
        assert!(ToolPackageModelDescriptionV1::new(" \t ").is_err());
        assert!(
            serde_json::from_value::<PackageToolAudienceV1>(json!("child_runtime_required"))
                .is_err()
        );
    }
}
