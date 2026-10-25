use std::{borrow::Cow, fmt, str::FromStr};

use aura_domain_common::ContentHash;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;
use thiserror::Error;

use url::Url;

use crate::{ArgumentJsonPointer, PackageResourceClaimKeyV1};
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{ToolCallId, ToolExecutionAttemptId, ToolWireVersionV1};

pub const STDIO_PROTOCOL_VERSION_V1: u32 = 1;
pub const AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1: &str = "AURA_TOOL_CALL_ID";
pub const AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1: &str =
    "AURA_TOOL_EXECUTION_ATTEMPT_ID";
pub const AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1: &str = "AURA_TOOL_OUTPUT_ROOT";
pub const MAX_STDIO_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_STDIO_STDOUT_WIRE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_COMMAND_STDOUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_COMMAND_STDERR_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_COMMAND_COMBINED_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_HTTP_REQUEST_BODY_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_HTTP_RESPONSE_WIRE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TOOL_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ATTEMPT_LOGICAL_IO_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_TOOL_OUTPUT_ARTIFACTS_V1: u32 = 256;
pub const MAX_PACKAGE_RELATIVE_PATH_BYTES: usize = 1_024;
pub const MAX_PACKAGE_RELATIVE_PATH_SEGMENT_BYTES: usize = 255;
pub const MAX_PACKAGE_RELATIVE_PATH_DEPTH: usize = 32;
pub const MAX_CREDENTIAL_SLOT_KEY_BYTES: usize = 64;
pub const MAX_INSTANCE_CONFIGURATION_KEY_BYTES: usize = 64;
pub const MAX_COMMAND_LITERAL_BYTES: usize = 8_192;
pub const MAX_COMMAND_SWITCH_BYTES: usize = 256;
pub const MAX_ENVIRONMENT_VARIABLE_NAME_BYTES: usize = 64;
pub const MAX_ENVIRONMENT_LITERAL_BYTES: usize = 8_192;
pub const MAX_FIXED_HTTPS_ORIGIN_BYTES: usize = 2_048;
pub const MAX_HTTP_BASE_PATH_BYTES: usize = 2_048;
pub const MAX_HTTP_PATH_LITERAL_SEGMENT_BYTES: usize = 512;
pub const MAX_HTTP_QUERY_NAME_BYTES: usize = 128;
pub const MAX_HTTP_QUERY_LITERAL_VALUE_BYTES: usize = 8_192;
pub const MAX_HTTP_HEADER_NAME_BYTES: usize = 128;
pub const MAX_HTTP_HEADER_LITERAL_VALUE_BYTES: usize = 8_192;
pub const MAX_HTTP_JSON_PROPERTY_NAME_BYTES: usize = 256;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolRuntimeContractError {
    #[error("package path is not a canonical safe relative path")]
    InvalidPackagePath,
    #[error("credential slot key is invalid")]
    InvalidCredentialSlot,
    #[error("environment variable name is invalid or controlled by Core")]
    InvalidEnvironmentName,
    #[error("HTTP header is invalid or controlled by Core")]
    InvalidHttpHeader,
    #[error("fixed HTTPS target must be a canonical complete HTTPS origin")]
    InvalidHttpsOrigin,
    #[error("HTTP base path is invalid")]
    InvalidHttpBasePath,
    #[error("HTTP endpoint slot key is invalid")]
    InvalidHttpEndpointSlot,
    #[error("environment literal value contains a prohibited control character")]
    InvalidEnvironmentValue,
    #[error("instance configuration key is invalid")]
    InvalidInstanceConfigurationKey,
    #[error("command literal is invalid")]
    InvalidCommandLiteral,
    #[error("command flag or option is invalid")]
    InvalidCommandSwitch,
    #[error("command environment names must be unique under Windows case folding")]
    DuplicateEnvironmentName,
    #[error("credential environment name conflicts with an ordinary environment entry")]
    CredentialEnvironmentConflict,
    #[error("HTTP path literal segment is invalid")]
    InvalidHttpPathLiteralSegment,
    #[error("HTTP path argument segment is invalid")]
    InvalidHttpPathArgumentSegment,
    #[error("HTTP query name is invalid")]
    InvalidHttpQueryName,
    #[error("HTTP query literal value is invalid")]
    InvalidHttpQueryValue,
    #[error("HTTP literal header value contains a prohibited control character")]
    InvalidHttpHeaderValue,
    #[error("HTTP JSON property name is invalid")]
    InvalidHttpJsonPropertyName,
    #[error("HTTP JSON object properties must be in JCS key order and unique")]
    InvalidHttpJsonProperties,
    #[error("HTTP header names must be unique without regard to ASCII case")]
    DuplicateHttpHeader,
    #[error("credential HTTP header conflicts with an ordinary HTTP header")]
    CredentialHttpHeaderConflict,
    #[error("HTTP success status ranges must be ordered, disjoint, and valid")]
    InvalidStatusRange,
    #[error("command success exit codes must be non-empty, unique, and contain at most 32 entries")]
    InvalidSuccessExitCodes,
    #[error("stdio protocol version must be exactly 1")]
    InvalidStdioProtocolVersion,
    #[error("stdio protocol sequence must start at 1")]
    InvalidStdioSequence,
    #[error("stdio progress percentage must be in the inclusive range 0..=100")]
    InvalidStdioProgress,
    #[error("tool output slot key is invalid")]
    InvalidOutputSlot,
    #[error("tool output path is not a canonical safe relative path")]
    InvalidOutputPath,
    #[error("tool Runtime output authorization is invalid")]
    InvalidOutputAuthorization,
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PackageRelativePath(String);

impl PackageRelativePath {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        let valid = valid_windows_relative_path(&value);
        valid.then_some(Self(value)).ok_or(ToolRuntimeContractError::InvalidPackagePath)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn valid_windows_relative_path(value: &str) -> bool {
    let segments = value.split('/').collect::<Vec<_>>();
    !value.is_empty()
        && value.len() <= MAX_PACKAGE_RELATIVE_PATH_BYTES
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains(['\\', '\0', ':', '<', '>', '"', '|', '?', '*'])
        && segments.len() <= MAX_PACKAGE_RELATIVE_PATH_DEPTH
        && segments.iter().all(valid_package_segment)
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ToolOutputRelativePathV1(String);

impl ToolOutputRelativePathV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        valid_windows_relative_path(&value)
            .then_some(Self(value))
            .ok_or(ToolRuntimeContractError::InvalidOutputPath)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for ToolOutputRelativePathV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolOutputRelativePathV1")
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        PackageRelativePath::json_schema(generator)
    }
}

impl fmt::Debug for ToolOutputRelativePathV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ToolOutputRelativePathV1").field(&self.0).finish()
    }
}

impl fmt::Display for ToolOutputRelativePathV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ToolOutputRelativePathV1 {
    type Err = ToolRuntimeContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for ToolOutputRelativePathV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ToolOutputSlotKeyV1(String);

impl ToolOutputSlotKeyV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        let valid =
            !value.is_empty() && value.len() <= 64 && value.is_ascii() && valid_runtime_key(&value);
        valid.then_some(Self(value)).ok_or(ToolRuntimeContractError::InvalidOutputSlot)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for ToolOutputSlotKeyV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolOutputSlotKeyV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 64,
            "pattern": "^[a-z0-9]+(?:[-_][a-z0-9]+)*$"
        })
    }
}

impl fmt::Debug for ToolOutputSlotKeyV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ToolOutputSlotKeyV1").field(&self.0).finish()
    }
}

impl<'de> Deserialize<'de> for ToolOutputSlotKeyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRuntimeOutputAuthorizationV1 {
    pub authorization_version: ToolWireVersionV1,
    pub tool_call_id: ToolCallId,
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    #[schemars(schema_with = "output_artifact_count_schema")]
    pub max_artifact_count: u32,
    #[schemars(schema_with = "output_single_artifact_bytes_schema")]
    pub max_single_artifact_bytes: u64,
    #[schemars(schema_with = "output_total_artifact_bytes_schema")]
    pub max_total_artifact_bytes: u64,
    pub path_policy_version: ToolWireVersionV1,
    pub media_policy_version: ToolWireVersionV1,
    pub authorization_hash: CanonicalSha256HashV1,
}

fn output_artifact_count_schema(_generator: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "const": 256 })
}

fn output_single_artifact_bytes_schema(_generator: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "const": 67108864 })
}

fn output_total_artifact_bytes_schema(_generator: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "const": 268435456 })
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolRuntimeOutputAuthorizationHashMaterialV1 {
    authorization_version: ToolWireVersionV1,
    tool_call_id: ToolCallId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    max_artifact_count: u32,
    max_single_artifact_bytes: u64,
    max_total_artifact_bytes: u64,
    path_policy_version: ToolWireVersionV1,
    media_policy_version: ToolWireVersionV1,
}

impl ToolRuntimeOutputAuthorizationV1 {
    pub fn new(
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
    ) -> Self {
        let material = ToolRuntimeOutputAuthorizationHashMaterialV1 {
            authorization_version: ToolWireVersionV1,
            tool_call_id,
            tool_execution_attempt_id,
            max_artifact_count: MAX_TOOL_OUTPUT_ARTIFACTS_V1,
            max_single_artifact_bytes: MAX_TOOL_ARTIFACT_BYTES as u64,
            max_total_artifact_bytes: MAX_ATTEMPT_LOGICAL_IO_BYTES as u64,
            path_policy_version: ToolWireVersionV1,
            media_policy_version: ToolWireVersionV1,
        };
        Self {
            authorization_version: material.authorization_version,
            tool_call_id,
            tool_execution_attempt_id,
            max_artifact_count: material.max_artifact_count,
            max_single_artifact_bytes: material.max_single_artifact_bytes,
            max_total_artifact_bytes: material.max_total_artifact_bytes,
            path_policy_version: material.path_policy_version,
            media_policy_version: material.media_policy_version,
            authorization_hash: canonical_sha256(&material)
                .expect("frozen output authorization material is canonical"),
        }
    }

    pub fn validate(&self) -> Result<(), ToolRuntimeContractError> {
        if self.max_artifact_count != MAX_TOOL_OUTPUT_ARTIFACTS_V1
            || self.max_single_artifact_bytes != MAX_TOOL_ARTIFACT_BYTES as u64
            || self.max_total_artifact_bytes != MAX_ATTEMPT_LOGICAL_IO_BYTES as u64
        {
            return Err(ToolRuntimeContractError::InvalidOutputAuthorization);
        }
        let expected = canonical_sha256(&ToolRuntimeOutputAuthorizationHashMaterialV1 {
            authorization_version: self.authorization_version,
            tool_call_id: self.tool_call_id,
            tool_execution_attempt_id: self.tool_execution_attempt_id,
            max_artifact_count: self.max_artifact_count,
            max_single_artifact_bytes: self.max_single_artifact_bytes,
            max_total_artifact_bytes: self.max_total_artifact_bytes,
            path_policy_version: self.path_policy_version,
            media_policy_version: self.media_policy_version,
        })
        .map_err(|_| ToolRuntimeContractError::InvalidOutputAuthorization)?;
        (expected == self.authorization_hash)
            .then_some(())
            .ok_or(ToolRuntimeContractError::InvalidOutputAuthorization)
    }
}

impl<'de> Deserialize<'de> for ToolRuntimeOutputAuthorizationV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            authorization_version: ToolWireVersionV1,
            tool_call_id: ToolCallId,
            tool_execution_attempt_id: ToolExecutionAttemptId,
            max_artifact_count: u32,
            max_single_artifact_bytes: u64,
            max_total_artifact_bytes: u64,
            path_policy_version: ToolWireVersionV1,
            media_policy_version: ToolWireVersionV1,
            authorization_hash: CanonicalSha256HashV1,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            authorization_version: wire.authorization_version,
            tool_call_id: wire.tool_call_id,
            tool_execution_attempt_id: wire.tool_execution_attempt_id,
            max_artifact_count: wire.max_artifact_count,
            max_single_artifact_bytes: wire.max_single_artifact_bytes,
            max_total_artifact_bytes: wire.max_total_artifact_bytes,
            path_policy_version: wire.path_policy_version,
            media_policy_version: wire.media_policy_version,
            authorization_hash: wire.authorization_hash,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

fn valid_package_segment(segment: &&str) -> bool {
    if segment.is_empty()
        || segment.len() > MAX_PACKAGE_RELATIVE_PATH_SEGMENT_BYTES
        || matches!(*segment, "." | "..")
    {
        return false;
    }
    let stem = segment.split('.').next().unwrap_or(segment).to_ascii_lowercase();
    !is_windows_device_stem(&stem)
        && !segment.ends_with([' ', '.'])
        && !segment.chars().any(|character| character.is_control())
}

fn is_windows_device_stem(stem: &str) -> bool {
    matches!(stem, "con" | "prn" | "aux" | "nul")
        || stem.strip_prefix("com").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
        })
        || stem.strip_prefix("lpt").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
        })
}

impl JsonSchema for PackageRelativePath {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PackageRelativePath")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 1024,
            "pattern": "^(?![A-Za-z]:)(?!/)(?!.*\\\\)(?!.*:)(?!.*(?:^|/)[.]{1,2}(?:/|$))(?!.*[\\u0000-\\u001f\\u007f])[^/]{1,255}(?:/[^/]{1,255}){0,31}$",
            "allOf": [
                { "not": { "pattern": "[<>\"|?*]" } },
                {
                    "not": {
                        "pattern": "(?:^|/)(?:[Cc][Oo][Nn]|[Pp][Rr][Nn]|[Aa][Uu][Xx]|[Nn][Uu][Ll]|[Cc][Oo][Mm](?:[1-9¹²³])|[Ll][Pp][Tt](?:[1-9¹²³]))(?:[.]|/|$)"
                    }
                },
                { "not": { "pattern": "(?:^|/)[^/]*[ .](?:/|$)" } }
            ]
        })
    }
}

impl fmt::Debug for PackageRelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("PackageRelativePath").field(&self.0).finish()
    }
}

impl fmt::Display for PackageRelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for PackageRelativePath {
    type Err = ToolRuntimeContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for PackageRelativePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CredentialSlotKey(String);

impl CredentialSlotKey {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_CREDENTIAL_SLOT_KEY_BYTES
            && value.is_ascii()
            && valid_runtime_key(&value);
        valid.then_some(Self(value)).ok_or(ToolRuntimeContractError::InvalidCredentialSlot)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for CredentialSlotKey {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("CredentialSlotKey")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 64,
            "pattern": "^[a-z0-9]+(?:[-_][a-z0-9]+)*$"
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EnvironmentVariableNameV1(String);

/// Environment names a Tool package may not declare.
///
/// `validate_environment_name` and `EnvironmentVariableNameV1::json_schema` both
/// read this list, so the published schema and the Rust check cannot drift apart.
/// `LOCALAPPDATA` is here because Core binds it to the attempt's own isolated
/// directory: Windows reads it from the launch block while creating an
/// AppContainer child, so a package that could set it would choose where its
/// container storage is rooted.
const CORE_CONTROLLED_ENVIRONMENT_NAMES: [&str; 12] = [
    "PATH",
    "PATHEXT",
    "PSMODULEPATH",
    "COMSPEC",
    "SYSTEMROOT",
    "WINDIR",
    "TEMP",
    "TMP",
    "LOCALAPPDATA",
    AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1,
];

impl EnvironmentVariableNameV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        validate_environment_name(&value)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for EnvironmentVariableNameV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for EnvironmentVariableNameV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("EnvironmentVariableNameV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": MAX_ENVIRONMENT_VARIABLE_NAME_BYTES,
            "pattern": "^[A-Z_][A-Z0-9_]{0,63}$",
            "not": { "enum": CORE_CONTROLLED_ENVIRONMENT_NAMES },
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EnvironmentLiteralValueV1(String);

impl EnvironmentLiteralValueV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        if value.len() > MAX_ENVIRONMENT_LITERAL_BYTES
            || contains_prohibited_single_line_control(&value)
        {
            return Err(ToolRuntimeContractError::InvalidEnvironmentValue);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for EnvironmentLiteralValueV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for EnvironmentLiteralValueV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("EnvironmentLiteralValueV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "maxLength": MAX_ENVIRONMENT_LITERAL_BYTES,
            "pattern": "^[^\\u0000-\\u001f\\u007f]*$"
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct InstanceConfigurationKeyV1(String);

impl InstanceConfigurationKeyV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_INSTANCE_CONFIGURATION_KEY_BYTES
            && value.is_ascii()
            && valid_runtime_key(&value);
        valid
            .then_some(Self(value))
            .ok_or(ToolRuntimeContractError::InvalidInstanceConfigurationKey)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for InstanceConfigurationKeyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for InstanceConfigurationKeyV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("InstanceConfigurationKeyV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": MAX_INSTANCE_CONFIGURATION_KEY_BYTES,
            "pattern": "^[a-z0-9]+(?:[-_][a-z0-9]+)*$"
        })
    }
}

impl fmt::Debug for CredentialSlotKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("CredentialSlotKey").field(&self.0).finish()
    }
}

impl<'de> Deserialize<'de> for CredentialSlotKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

macro_rules! runtime_text_type {
    ($name:ident, $schema_name:literal, $minimum:literal, $maximum:expr, $pattern:literal, $error:ident, $validator:expr) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
                let value = value.into();
                let valid = byte_length_in_range(&value, $minimum, $maximum) && ($validator)(&value);
                valid.then_some(Self(value)).ok_or(ToolRuntimeContractError::$error)
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
                    "minLength": $minimum,
                    "maxLength": $maximum,
                    "pattern": $pattern
                })
            }
        }
    };
}

runtime_text_type!(
    CommandLiteralV1,
    "CommandLiteralV1",
    0,
    MAX_COMMAND_LITERAL_BYTES,
    "^[^\\u0000]*$",
    InvalidCommandLiteral,
    |value: &str| !value.contains('\0')
);
runtime_text_type!(
    CommandFlagV1,
    "CommandFlagV1",
    1,
    MAX_COMMAND_SWITCH_BYTES,
    "^[^\\u0000-\\u001f\\u007f]+$",
    InvalidCommandSwitch,
    |value: &str| !contains_prohibited_single_line_control(value)
);
runtime_text_type!(
    CommandOptionNameV1,
    "CommandOptionNameV1",
    1,
    MAX_COMMAND_SWITCH_BYTES,
    "^[^\\u0000-\\u001f\\u007f]+$",
    InvalidCommandSwitch,
    |value: &str| !contains_prohibited_single_line_control(value)
);

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandArgumentV1 {
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
pub enum CommandWorkingDirectoryV1 {
    PackageRoot,
    WorkspaceRoot,
    ResolvedDirectoryClaim { claim_key: PackageResourceClaimKeyV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandEnvironmentValueV1 {
    Literal { value: EnvironmentLiteralValueV1 },
    Scalar { source: ArgumentJsonPointer },
    InstanceConfiguration { key: InstanceConfigurationKeyV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEnvironmentEntryV1 {
    pub name: EnvironmentVariableNameV1,
    pub value: CommandEnvironmentValueV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandCredentialEnvironmentSlotV1 {
    pub slot_key: CredentialSlotKey,
    pub environment_variable: EnvironmentVariableNameV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeclarativeCommandV1 {
    Argv {
        executable: PackageRelativePath,
        arguments: Vec<CommandArgumentV1>,
        working_directory: CommandWorkingDirectoryV1,
        environment: Vec<CommandEnvironmentEntryV1>,
        credential_slot: Option<CommandCredentialEnvironmentSlotV1>,
        success_exit_codes: Vec<i32>,
    },
    PowershellScript {
        script: PackageRelativePath,
        arguments: Vec<CommandArgumentV1>,
        working_directory: CommandWorkingDirectoryV1,
        environment: Vec<CommandEnvironmentEntryV1>,
        credential_slot: Option<CommandCredentialEnvironmentSlotV1>,
        success_exit_codes: Vec<i32>,
    },
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "UPPERCASE")]
pub enum DeclarativeHttpMethodV1 {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FixedHttpsOriginV1(String);

impl FixedHttpsOriginV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        if value.len() > MAX_FIXED_HTTPS_ORIGIN_BYTES {
            return Err(ToolRuntimeContractError::InvalidHttpsOrigin);
        }
        let parsed =
            Url::parse(&value).map_err(|_| ToolRuntimeContractError::InvalidHttpsOrigin)?;
        let canonical_origin = parsed.origin().ascii_serialization();
        let valid = parsed.scheme() == "https"
            && parsed.host_str().is_some()
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.path() == "/"
            && parsed.query().is_none()
            && parsed.fragment().is_none()
            && canonical_origin == value;
        valid.then_some(Self(value)).ok_or(ToolRuntimeContractError::InvalidHttpsOrigin)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for FixedHttpsOriginV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for FixedHttpsOriginV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("FixedHttpsOriginV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "maxLength": MAX_FIXED_HTTPS_ORIGIN_BYTES,
            "format": "uri",
            "pattern": "^https://[^/?#]+$"
        })
    }
}

/// A fully encoded canonical absolute path for a P07 Declarative HTTP target.
///
/// The public wire remains a string, while the Rust value retains the parsed
/// segment boundary and explicit trailing-slash bit so later request builders
/// cannot accidentally collapse two distinct grants or request hashes.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HttpBasePathV1 {
    canonical_wire: String,
    segments: Vec<String>,
    explicit_trailing_slash: bool,
}

impl HttpBasePathV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        let (segments, explicit_trailing_slash) = parse_canonical_http_base_path(&value)
            .ok_or(ToolRuntimeContractError::InvalidHttpBasePath)?;
        Ok(Self { canonical_wire: value, segments, explicit_trailing_slash })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.canonical_wire
    }

    #[must_use]
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Root `/` has no optional trailing segment. For non-root paths this bit
    /// distinguishes `/v1/items` from `/v1/items/`.
    #[must_use]
    pub const fn has_explicit_trailing_slash(&self) -> bool {
        self.explicit_trailing_slash
    }
}

impl Serialize for HttpBasePathV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.canonical_wire)
    }
}

impl<'de> Deserialize<'de> for HttpBasePathV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for HttpBasePathV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("HttpBasePathV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
                    "type": "string",
                    "minLength": 1,
                    "maxLength": MAX_HTTP_BASE_PATH_BYTES,
                    "allOf": [
                        {
                            "pattern": "^/(?:(?:[A-Za-z0-9._~!$&'()*+,;=:@-]|%[0-9A-F]{2})+(?:/(?:[A-Za-z0-9._~!$&'()*+,;=:@-]|%[0-9A-F]{2})+)*/?)?$"
                },
                {
                    "not": {
                        "pattern": "/(?:\\.|%2E)(?:(?:\\.|%2E))?(?:/|$)"
                    }
                },
                {
                    "not": {
                        "pattern": "%(?:2F|5C|0[0-9A-F]|1[0-9A-F]|7F)"
                    }
                }
            ]
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct HttpEndpointSlotKeyV1(String);

impl HttpEndpointSlotKeyV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        let valid =
            !value.is_empty() && value.len() <= 64 && value.is_ascii() && valid_runtime_key(&value);
        valid.then_some(Self(value)).ok_or(ToolRuntimeContractError::InvalidHttpEndpointSlot)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for HttpEndpointSlotKeyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for HttpEndpointSlotKeyV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("HttpEndpointSlotKeyV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 64,
            "pattern": "^[a-z0-9]+(?:[-_][a-z0-9]+)*$"
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeclarativeHttpTargetV1 {
    FixedHttps { origin: FixedHttpsOriginV1, base_path: HttpBasePathV1 },
    UserBoundEndpoint { endpoint_slot_key: HttpEndpointSlotKeyV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HttpPathSegmentV1 {
    Literal { value: HttpPathLiteralSegmentV1 },
    Argument { source: ArgumentJsonPointer },
}

macro_rules! http_logical_path_segment_type {
    ($name:ident, $schema_name:literal, $error:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
                let value = value.into();
                valid_http_logical_path_segment(&value)
                    .then_some(Self(value))
                    .ok_or(ToolRuntimeContractError::$error)
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Encodes this validated logical segment exactly once. A literal
            /// `%2F` therefore becomes `%252F`; it is never interpreted as an
            /// already encoded slash.
            #[must_use]
            pub fn percent_encoded(&self) -> String {
                percent_encode_http_path_segment(&self.0)
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
                    "maxLength": MAX_HTTP_PATH_LITERAL_SEGMENT_BYTES,
                    "pattern": "^(?!(?:\\.|\\.\\.)$)[^/\\\\\\u0000-\\u001f\\u007f]+$"
                })
            }
        }
    };
}

http_logical_path_segment_type!(
    HttpPathLiteralSegmentV1,
    "HttpPathLiteralSegmentV1",
    InvalidHttpPathLiteralSegment
);
http_logical_path_segment_type!(
    HttpPathArgumentSegmentV1,
    "HttpPathArgumentSegmentV1",
    InvalidHttpPathArgumentSegment
);
runtime_text_type!(
    HttpQueryNameV1,
    "HttpQueryNameV1",
    1,
    MAX_HTTP_QUERY_NAME_BYTES,
    "^[^\\u0000-\\u001f\\u007f]+$",
    InvalidHttpQueryName,
    |value: &str| !contains_prohibited_single_line_control(value)
);
runtime_text_type!(
    HttpQueryLiteralValueV1,
    "HttpQueryLiteralValueV1",
    0,
    MAX_HTTP_QUERY_LITERAL_VALUE_BYTES,
    "^[^\\u0000-\\u001f\\u007f]*$",
    InvalidHttpQueryValue,
    |value: &str| !contains_prohibited_single_line_control(value)
);

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HttpValueSourceV1 {
    Literal { value: HttpQueryLiteralValueV1 },
    Scalar { source: ArgumentJsonPointer },
    Repeated { source: ArgumentJsonPointer },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HttpHeaderValueSourceV1 {
    Literal { value: HttpHeaderLiteralValueV1 },
    Scalar { source: ArgumentJsonPointer },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct HttpHeaderNameV1(String);

impl HttpHeaderNameV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        validate_http_header(&value)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for HttpHeaderNameV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for HttpHeaderNameV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("HttpHeaderNameV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": MAX_HTTP_HEADER_NAME_BYTES,
            "pattern": "^[!#$%&'*+.^_`|~0-9A-Za-z-]+$",
            "not": {
                "pattern": "^(?:[Hh][Oo][Ss][Tt]|[Cc][Oo][Nn][Tt][Ee][Nn][Tt]-[Ll][Ee][Nn][Gg][Tt][Hh]|[Tt][Rr][Aa][Nn][Ss][Ff][Ee][Rr]-[Ee][Nn][Cc][Oo][Dd][Ii][Nn][Gg]|[Cc][Oo][Nn][Nn][Ee][Cc][Tt][Ii][Oo][Nn]|[Aa][Uu][Tt][Hh][Oo][Rr][Ii][Zz][Aa][Tt][Ii][Oo][Nn]|[Cc][Oo][Oo][Kk][Ii][Ee]|[Ss][Ee][Tt]-[Cc][Oo][Oo][Kk][Ii][Ee]|[Uu][Pp][Gg][Rr][Aa][Dd][Ee]|[Tt][Rr][Aa][Ii][Ll][Ee][Rr]|[Ee][Xx][Pp][Ee][Cc][Tt]|[Tt][Ee]|[Pp][Rr][Oo][Xx][Yy]-.*)$"
            }
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HttpHeaderLiteralValueV1(String);

impl HttpHeaderLiteralValueV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRuntimeContractError> {
        let value = value.into();
        let valid = value.len() <= MAX_HTTP_HEADER_LITERAL_VALUE_BYTES
            && !contains_prohibited_single_line_control(&value);
        valid.then_some(Self(value)).ok_or(ToolRuntimeContractError::InvalidHttpHeaderValue)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for HttpHeaderLiteralValueV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for HttpHeaderLiteralValueV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("HttpHeaderLiteralValueV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "maxLength": MAX_HTTP_HEADER_LITERAL_VALUE_BYTES,
            "pattern": "^[^\\u0000-\\u001f\\u007f]*$"
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpQueryEntryV1 {
    pub name: HttpQueryNameV1,
    pub value: HttpValueSourceV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpHeaderEntryV1 {
    pub name: HttpHeaderNameV1,
    pub value: HttpHeaderValueSourceV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HttpJsonTemplateV1 {
    Literal { value: Value },
    Argument { source: ArgumentJsonPointer },
    Array { items: Vec<HttpJsonTemplateV1> },
    Object { properties: HttpJsonPropertiesV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpJsonPropertyV1 {
    pub name: HttpJsonPropertyNameV1,
    pub value: HttpJsonTemplateV1,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HttpJsonPropertiesV1(Vec<HttpJsonPropertyV1>);

impl HttpJsonPropertiesV1 {
    pub fn new(values: Vec<HttpJsonPropertyV1>) -> Result<Self, ToolRuntimeContractError> {
        let valid = values.windows(2).all(|pair| {
            pair[0].name.as_str().encode_utf16().cmp(pair[1].name.as_str().encode_utf16()).is_lt()
        });
        valid.then_some(Self(values)).ok_or(ToolRuntimeContractError::InvalidHttpJsonProperties)
    }

    #[must_use]
    pub fn as_slice(&self) -> &[HttpJsonPropertyV1] {
        &self.0
    }
}

runtime_text_type!(
    HttpJsonPropertyNameV1,
    "HttpJsonPropertyNameV1",
    1,
    MAX_HTTP_JSON_PROPERTY_NAME_BYTES,
    "^[^\\u0000-\\u001f\\u007f]+$",
    InvalidHttpJsonPropertyName,
    |value: &str| !contains_prohibited_single_line_control(value)
);

impl<'de> Deserialize<'de> for HttpJsonPropertiesV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<HttpJsonPropertyV1>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeclarativeHttpBodyV1 {
    None,
    ApplicationJson { template: HttpJsonTemplateV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HttpCredentialSlotV1 {
    BearerAuthorization { slot_key: CredentialSlotKey },
    FixedHeader { slot_key: CredentialSlotKey, header_name: HttpHeaderNameV1 },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpStatusRangeV1 {
    pub start: u16,
    pub end: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeclarativeHttpV1 {
    pub target: DeclarativeHttpTargetV1,
    pub method: DeclarativeHttpMethodV1,
    pub path: Vec<HttpPathSegmentV1>,
    pub query: Vec<HttpQueryEntryV1>,
    pub headers: Vec<HttpHeaderEntryV1>,
    pub body: DeclarativeHttpBodyV1,
    pub credential_slot: Option<HttpCredentialSlotV1>,
    pub success_status_ranges: Vec<HttpStatusRangeV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolRuntimeDescriptorV1 {
    DeclarativeCommand {
        command: DeclarativeCommandV1,
    },
    AuraStdio {
        executable: PackageRelativePath,
        arguments: Vec<CommandArgumentV1>,
        working_directory: CommandWorkingDirectoryV1,
        environment: Vec<CommandEnvironmentEntryV1>,
        credential_slot: Option<CommandCredentialEnvironmentSlotV1>,
    },
    DeclarativeHttp {
        request: DeclarativeHttpV1,
    },
    ExternalSource {
        source_contract_key: String,
    },
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum StdioProtocolDirectionV1 {
    CoreToRuntime,
    RuntimeToCore,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StdioHelloPayloadV1 {
    pub runtime_name: String,
    pub runtime_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StdioExecutePayloadV1 {
    pub arguments: Value,
    pub arguments_hash: ContentHash,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StdioEmptyPayloadV1 {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StdioProgressPayloadV1 {
    pub percentage: Option<u8>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StdioArtifactReadyPayloadV1 {
    pub slot: ToolOutputSlotKeyV1,
    pub relative_path: ToolOutputRelativePathV1,
    pub media_type: String,
    pub byte_length: u64,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StdioFinalResultPayloadV1 {
    pub result: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StdioFinalFailurePayloadV1 {
    pub code: String,
    pub safe_message: String,
}

/// The V1 wire keeps `message_type` and its typed `payload` at the frame root.
/// Common identity fields are deliberately repeated in every tagged variant so
/// Serde and JSON Schema cannot admit an invalid message/payload combination.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "message_type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StdioProtocolFrameV1 {
    Hello {
        protocol_version: u32,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        sequence: u64,
        payload: StdioHelloPayloadV1,
    },
    Execute {
        protocol_version: u32,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        sequence: u64,
        payload: StdioExecutePayloadV1,
    },
    Cancel {
        protocol_version: u32,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        sequence: u64,
        payload: StdioEmptyPayloadV1,
    },
    Progress {
        protocol_version: u32,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        sequence: u64,
        payload: StdioProgressPayloadV1,
    },
    ArtifactReady {
        protocol_version: u32,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        sequence: u64,
        payload: StdioArtifactReadyPayloadV1,
    },
    CancelAcknowledged {
        protocol_version: u32,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        sequence: u64,
        payload: StdioEmptyPayloadV1,
    },
    FinalResult {
        protocol_version: u32,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        sequence: u64,
        payload: StdioFinalResultPayloadV1,
    },
    FinalFailure {
        protocol_version: u32,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        sequence: u64,
        payload: StdioFinalFailurePayloadV1,
    },
}

impl StdioProtocolFrameV1 {
    pub fn validate(&self) -> Result<(), ToolRuntimeContractError> {
        if self.protocol_version() != STDIO_PROTOCOL_VERSION_V1 {
            return Err(ToolRuntimeContractError::InvalidStdioProtocolVersion);
        }
        if self.sequence() == 0 {
            return Err(ToolRuntimeContractError::InvalidStdioSequence);
        }
        if let Self::Progress { payload, .. } = self
            && payload.percentage.is_some_and(|value| value > 100)
        {
            return Err(ToolRuntimeContractError::InvalidStdioProgress);
        }
        Ok(())
    }

    #[must_use]
    pub const fn direction(&self) -> StdioProtocolDirectionV1 {
        match self {
            Self::Execute { .. } | Self::Cancel { .. } => StdioProtocolDirectionV1::CoreToRuntime,
            Self::Hello { .. }
            | Self::Progress { .. }
            | Self::ArtifactReady { .. }
            | Self::CancelAcknowledged { .. }
            | Self::FinalResult { .. }
            | Self::FinalFailure { .. } => StdioProtocolDirectionV1::RuntimeToCore,
        }
    }

    #[must_use]
    pub const fn protocol_version(&self) -> u32 {
        match self {
            Self::Hello { protocol_version, .. }
            | Self::Execute { protocol_version, .. }
            | Self::Cancel { protocol_version, .. }
            | Self::Progress { protocol_version, .. }
            | Self::ArtifactReady { protocol_version, .. }
            | Self::CancelAcknowledged { protocol_version, .. }
            | Self::FinalResult { protocol_version, .. }
            | Self::FinalFailure { protocol_version, .. } => *protocol_version,
        }
    }

    #[must_use]
    pub const fn tool_call_id(&self) -> ToolCallId {
        match self {
            Self::Hello { tool_call_id, .. }
            | Self::Execute { tool_call_id, .. }
            | Self::Cancel { tool_call_id, .. }
            | Self::Progress { tool_call_id, .. }
            | Self::ArtifactReady { tool_call_id, .. }
            | Self::CancelAcknowledged { tool_call_id, .. }
            | Self::FinalResult { tool_call_id, .. }
            | Self::FinalFailure { tool_call_id, .. } => *tool_call_id,
        }
    }

    #[must_use]
    pub const fn tool_execution_attempt_id(&self) -> ToolExecutionAttemptId {
        match self {
            Self::Hello { tool_execution_attempt_id, .. }
            | Self::Execute { tool_execution_attempt_id, .. }
            | Self::Cancel { tool_execution_attempt_id, .. }
            | Self::Progress { tool_execution_attempt_id, .. }
            | Self::ArtifactReady { tool_execution_attempt_id, .. }
            | Self::CancelAcknowledged { tool_execution_attempt_id, .. }
            | Self::FinalResult { tool_execution_attempt_id, .. }
            | Self::FinalFailure { tool_execution_attempt_id, .. } => *tool_execution_attempt_id,
        }
    }

    #[must_use]
    pub const fn sequence(&self) -> u64 {
        match self {
            Self::Hello { sequence, .. }
            | Self::Execute { sequence, .. }
            | Self::Cancel { sequence, .. }
            | Self::Progress { sequence, .. }
            | Self::ArtifactReady { sequence, .. }
            | Self::CancelAcknowledged { sequence, .. }
            | Self::FinalResult { sequence, .. }
            | Self::FinalFailure { sequence, .. } => *sequence,
        }
    }

    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::FinalResult { .. } | Self::FinalFailure { .. })
    }
}

impl DeclarativeCommandV1 {
    pub fn validate(&self) -> Result<(), ToolRuntimeContractError> {
        let (success_exit_codes, environment, credential_slot, script) = match self {
            Self::Argv { success_exit_codes, environment, credential_slot, .. } => {
                (success_exit_codes, environment, credential_slot, None)
            }
            Self::PowershellScript {
                script,
                success_exit_codes,
                environment,
                credential_slot,
                ..
            } => (success_exit_codes, environment, credential_slot, Some(script)),
        };
        if success_exit_codes.is_empty()
            || success_exit_codes.len() > 32
            || success_exit_codes.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(ToolRuntimeContractError::InvalidSuccessExitCodes);
        }
        if script.is_some_and(|path| !path.as_str().to_ascii_lowercase().ends_with(".ps1")) {
            return Err(ToolRuntimeContractError::InvalidPackagePath);
        }
        validate_command_environment_v1(environment, credential_slot.as_ref())
    }
}

impl DeclarativeHttpV1 {
    pub fn validate(&self) -> Result<(), ToolRuntimeContractError> {
        if self.success_status_ranges.is_empty()
            || self.success_status_ranges.len() > 16
            || self
                .success_status_ranges
                .iter()
                .any(|range| range.start < 100 || range.end > 599 || range.start > range.end)
            || self.success_status_ranges.windows(2).any(|pair| pair[0].end >= pair[1].start)
        {
            return Err(ToolRuntimeContractError::InvalidStatusRange);
        }
        validate_http_header_bindings_v1(&self.headers, self.credential_slot.as_ref())
    }
}

pub fn validate_command_environment_v1(
    environment: &[CommandEnvironmentEntryV1],
    credential_slot: Option<&CommandCredentialEnvironmentSlotV1>,
) -> Result<(), ToolRuntimeContractError> {
    let mut names = std::collections::BTreeSet::new();
    for entry in environment {
        validate_environment_name(entry.name.as_str())?;
        if !names.insert(entry.name.as_str().to_ascii_uppercase()) {
            return Err(ToolRuntimeContractError::DuplicateEnvironmentName);
        }
    }
    if let Some(slot) = credential_slot {
        validate_environment_name(slot.environment_variable.as_str())?;
        if names.contains(&slot.environment_variable.as_str().to_ascii_uppercase()) {
            return Err(ToolRuntimeContractError::CredentialEnvironmentConflict);
        }
    }
    Ok(())
}

pub fn validate_http_header_bindings_v1(
    headers: &[HttpHeaderEntryV1],
    credential_slot: Option<&HttpCredentialSlotV1>,
) -> Result<(), ToolRuntimeContractError> {
    let mut names = std::collections::BTreeSet::new();
    for header in headers {
        validate_http_header(header.name.as_str())?;
        if !names.insert(header.name.as_str().to_ascii_lowercase()) {
            return Err(ToolRuntimeContractError::DuplicateHttpHeader);
        }
    }
    if let Some(HttpCredentialSlotV1::FixedHeader { header_name, .. }) = credential_slot {
        validate_http_header(header_name.as_str())?;
        if names.contains(&header_name.as_str().to_ascii_lowercase()) {
            return Err(ToolRuntimeContractError::CredentialHttpHeaderConflict);
        }
    }
    Ok(())
}

fn validate_environment_name(name: &str) -> Result<(), ToolRuntimeContractError> {
    let upper = name.to_ascii_uppercase();
    let valid = !name.is_empty()
        && name.len() <= MAX_ENVIRONMENT_VARIABLE_NAME_BYTES
        && name.is_ascii()
        && name.as_bytes().first().is_some_and(|byte| byte.is_ascii_uppercase() || *byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && !CORE_CONTROLLED_ENVIRONMENT_NAMES.contains(&upper.as_str());
    valid.then_some(()).ok_or(ToolRuntimeContractError::InvalidEnvironmentName)
}

fn validate_http_header(name: &str) -> Result<(), ToolRuntimeContractError> {
    const BLOCKED: [&str; 13] = [
        "host",
        "content-length",
        "transfer-encoding",
        "connection",
        "authorization",
        "cookie",
        "proxy-authorization",
        "proxy-authenticate",
        "te",
        "set-cookie",
        "upgrade",
        "trailer",
        "expect",
    ];
    let lower = name.to_ascii_lowercase();
    let valid = !name.is_empty()
        && name.len() <= MAX_HTTP_HEADER_NAME_BYTES
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'!' | b'#'..=b'\'' | b'*' | b'+' | b'-' | b'.' | b'^'..=b'`' | b'|' | b'~')
        })
        && !BLOCKED.contains(&lower.as_str())
        && !lower.starts_with("proxy-");
    valid.then_some(()).ok_or(ToolRuntimeContractError::InvalidHttpHeader)
}

fn contains_prohibited_single_line_control(value: &str) -> bool {
    value.chars().any(|character| character <= '\u{001f}' || character == '\u{007f}')
}

fn byte_length_in_range(value: &str, minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&value.len())
}

fn parse_canonical_http_base_path(path: &str) -> Option<(Vec<String>, bool)> {
    if !(1..=MAX_HTTP_BASE_PATH_BYTES).contains(&path.len())
        || !path.starts_with('/')
        || path.contains(['?', '#', '\\', ' '])
        || contains_prohibited_single_line_control(path)
    {
        return None;
    }
    if path == "/" {
        return Some((Vec::new(), false));
    }

    let explicit_trailing_slash = path.ends_with('/');
    let content = if explicit_trailing_slash {
        path.strip_prefix('/')?.strip_suffix('/')?
    } else {
        path.strip_prefix('/')?
    };
    if content.is_empty() {
        return None;
    }
    let segments = content.split('/').map(str::to_owned).collect::<Vec<_>>();
    if segments.iter().any(|segment| !valid_encoded_http_base_path_segment(segment)) {
        return None;
    }
    Some((segments, explicit_trailing_slash))
}

fn valid_encoded_http_base_path_segment(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            if !is_http_path_pchar(bytes[index]) {
                return false;
            }
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let (Some(high), Some(low)) = (bytes.get(index + 1), bytes.get(index + 2)) else {
            return false;
        };
        let (Some(high), Some(low)) = (uppercase_hex_nibble(*high), uppercase_hex_nibble(*low))
        else {
            return false;
        };
        let decoded_byte = (high << 4) | low;
        if decoded_byte <= 0x1f || decoded_byte == 0x7f || matches!(decoded_byte, b'/' | b'\\') {
            return false;
        }
        decoded.push(decoded_byte);
        index += 3;
    }
    !matches!(decoded.as_slice(), b"." | b"..")
}

const fn is_http_path_pchar(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
        )
}

const fn uppercase_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn valid_http_logical_path_segment(value: &str) -> bool {
    byte_length_in_range(value, 1, MAX_HTTP_PATH_LITERAL_SEGMENT_BYTES)
        && !matches!(value, "." | "..")
        && !value.contains(['/', '\\'])
        && !contains_prohibited_single_line_control(value)
}

fn percent_encode_http_path_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

fn valid_runtime_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
        && !bytes
            .windows(2)
            .any(|pair| matches!(pair[0], b'_' | b'-') && matches!(pair[1], b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use aura_domain_common::ContentHash;
    use serde_json::json;

    use super::{
        CORE_CONTROLLED_ENVIRONMENT_NAMES, CommandCredentialEnvironmentSlotV1,
        CommandEnvironmentEntryV1, CommandFlagV1, CommandLiteralV1, CommandOptionNameV1,
        CredentialSlotKey, EnvironmentLiteralValueV1, EnvironmentVariableNameV1,
        FixedHttpsOriginV1, HttpBasePathV1, HttpCredentialSlotV1, HttpHeaderEntryV1,
        HttpHeaderLiteralValueV1, HttpHeaderNameV1, HttpJsonPropertiesV1, HttpJsonPropertyNameV1,
        HttpPathArgumentSegmentV1, HttpPathLiteralSegmentV1, HttpQueryEntryV1,
        HttpQueryLiteralValueV1, HttpQueryNameV1, PackageRelativePath, STDIO_PROTOCOL_VERSION_V1,
        StdioHelloPayloadV1, StdioProtocolDirectionV1, StdioProtocolFrameV1, ToolCallId,
        ToolExecutionAttemptId, ToolRuntimeContractError, validate_command_environment_v1,
        validate_http_header, validate_http_header_bindings_v1,
    };
    use aura_domain_common::jcs::canonical_sha256;
    use schemars::schema_for;

    #[test]
    fn package_path_rejects_windows_escape_and_device_names() {
        for invalid in [
            "../run.exe",
            "C:/run.exe",
            "bin\\run.exe",
            "NUL.txt",
            "bin/x:ads",
            "bad<name",
            "bad>name",
            "bad\"name",
            "bad|name",
            "bad?name",
            "bad*name",
        ] {
            assert!(matches!(
                PackageRelativePath::new(invalid),
                Err(ToolRuntimeContractError::InvalidPackagePath)
            ));
        }
        for device in
            ["COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³", "com¹.txt", "Com².bin", "lpt³.exe"]
        {
            assert!(
                PackageRelativePath::new(format!("runtime/{device}")).is_err(),
                "Windows device alias must be rejected: {device}"
            );
        }
        PackageRelativePath::new("bin/run.exe").expect("safe path");
        assert!(PackageRelativePath::new("a".repeat(256)).is_err());
        assert!(
            PackageRelativePath::new((0..33).map(|_| "a").collect::<Vec<_>>().join("/")).is_err()
        );
    }

    #[test]
    fn controlled_http_headers_are_rejected() {
        for name in [
            "Host",
            "Authorization",
            "Proxy-Authorization",
            "Cookie",
            "Set-Cookie",
            "Upgrade",
            "Trailer",
            "Expect",
        ] {
            assert!(validate_http_header(name).is_err(), "{name}");
        }
        validate_http_header("X-Aura-Client").expect("ordinary header");
        for name in [
            "PATH",
            "SystemRoot",
            "TEMP",
            "LOCALAPPDATA",
            "localappdata",
            "LocalAppData",
            "AURA_TOOL_CALL_ID",
            "aura_tool_call_id",
            "AURA_TOOL_EXECUTION_ATTEMPT_ID",
            "aura_tool_execution_attempt_id",
            "AURA_TOOL_OUTPUT_ROOT",
            "aura_tool_output_root",
        ] {
            assert!(EnvironmentVariableNameV1::new(name).is_err(), "{name}");
        }
        EnvironmentVariableNameV1::new("AURA_OUTPUT_MODE").expect("package environment");
        EnvironmentVariableNameV1::new("_AURA_MODE").expect("leading underscore");
        assert!(EnvironmentVariableNameV1::new("A".repeat(65)).is_err());
    }

    #[test]
    fn core_controlled_environment_names_are_one_list_for_the_check_and_the_schema() {
        // The published schema and the Rust reservation are the same list: a name
        // cannot be accepted by one and rejected by the other.
        let schema = serde_json::to_value(schema_for!(EnvironmentVariableNameV1))
            .expect("serialize the published environment name schema");
        let reserved = schema["not"]["enum"].as_array().expect("reserved enum");
        let names =
            reserved.iter().map(|value| value.as_str().expect("string name")).collect::<Vec<_>>();
        assert_eq!(names, CORE_CONTROLLED_ENVIRONMENT_NAMES);
        for name in CORE_CONTROLLED_ENVIRONMENT_NAMES {
            assert!(
                EnvironmentVariableNameV1::new(name).is_err(),
                "{name} must be reserved in the Rust check too"
            );
            assert!(
                serde_json::from_value::<EnvironmentVariableNameV1>(serde_json::json!(name))
                    .is_err(),
                "{name} must be reserved when a package is deserialized too"
            );
        }
    }

    #[test]
    fn a_credential_slot_cannot_bind_a_core_controlled_environment_name() {
        // The credential slot carries the same name type, so `LOCALAPPDATA` cannot
        // be smuggled in as a credential destination either.
        for wire in [
            json!({ "slot_key": "fixture", "environment_variable": "LOCALAPPDATA" }),
            json!({ "slot_key": "fixture", "environment_variable": "localappdata" }),
        ] {
            assert!(
                serde_json::from_value::<CommandCredentialEnvironmentSlotV1>(wire.clone()).is_err(),
                "credential slot accepted {wire}"
            );
        }
    }

    #[test]
    fn runtime_output_authorization_is_bound_to_the_exact_attempt() {
        let authorization = super::ToolRuntimeOutputAuthorizationV1::new(
            ToolCallId::new(),
            ToolExecutionAttemptId::new(),
        );
        authorization.validate().expect("valid output authorization");
        let mut wire = serde_json::to_value(&authorization).expect("serialize authorization");
        wire["tool_execution_attempt_id"] =
            serde_json::Value::String(ToolExecutionAttemptId::new().to_string());
        assert!(
            serde_json::from_value::<super::ToolRuntimeOutputAuthorizationV1>(wire).is_err(),
            "attempt substitution must invalidate the authorization hash"
        );
    }

    #[test]
    fn fixed_https_origin_uses_url_parser_and_requires_the_complete_origin() {
        FixedHttpsOriginV1::new("https://example.invalid").expect("canonical origin");
        for invalid in [
            "http://example.invalid",
            "https://user@example.invalid",
            "https://example.invalid/path",
            "https://example.invalid?query=1",
            "https://example.invalid/",
        ] {
            assert!(FixedHttpsOriginV1::new(invalid).is_err(), "{invalid}");
        }
        assert!(FixedHttpsOriginV1::new(format!("https://{}.invalid", "a".repeat(2_040))).is_err());
    }

    #[test]
    fn command_and_http_text_wrappers_enforce_frozen_byte_and_control_limits() {
        CommandLiteralV1::new("").expect("empty literal is valid");
        assert!(CommandLiteralV1::new("x\0y").is_err());
        assert!(CommandLiteralV1::new("x".repeat(8_193)).is_err());
        CommandFlagV1::new("--verbose").expect("flag");
        CommandOptionNameV1::new("--output").expect("option");
        assert!(CommandFlagV1::new("--verbose\n").is_err());

        HttpPathLiteralSegmentV1::new("documents").expect("path segment");
        assert!(HttpPathLiteralSegmentV1::new("documents/latest").is_err());
        HttpQueryNameV1::new("tag").expect("query name");
        assert!(HttpQueryNameV1::new("").is_err());
        HttpQueryLiteralValueV1::new("").expect("empty query value");
        assert!(HttpQueryLiteralValueV1::new("q".repeat(8_193)).is_err());
        HttpJsonPropertyNameV1::new("payload").expect("JSON property");
        assert!(HttpJsonPropertyNameV1::new("").is_err());
        HttpHeaderLiteralValueV1::new("fixture").expect("header value");
        assert!(HttpHeaderLiteralValueV1::new("contains\ttab").is_err());
        EnvironmentLiteralValueV1::new("fixture").expect("environment value");
        assert!(EnvironmentLiteralValueV1::new("x".repeat(8_193)).is_err());

        for control in ['\0', '\t', '\r', '\n', '\u{007f}'] {
            assert!(HttpQueryNameV1::new(format!("q{control}name")).is_err());
            assert!(HttpQueryLiteralValueV1::new(format!("q{control}value")).is_err());
            assert!(HttpJsonPropertyNameV1::new(format!("p{control}name")).is_err());
        }
    }

    #[test]
    fn http_base_path_enforces_p1a_without_collapsing_trailing_slashes() {
        for valid in ["/", "/v1", "/v1/items", "/v1/items/", "/v1/a%20b"] {
            HttpBasePathV1::new(valid).unwrap_or_else(|error| panic!("{valid}: {error}"));
        }
        for invalid in [
            "v1",
            "/v1 items",
            "/v1\\items",
            "/v1//items",
            "/v1/items//",
            "/v1/%",
            "/v1/%2",
            "/v1/%GG",
            "/v1/%2f",
            "/v1/./secret",
            "/v1/../secret",
            "/v1/%2E/secret",
            "/v1/%2E%2E/secret",
            "/v1/%2F/secret",
            "/v1/%5C/secret",
            "/v1/项目",
        ] {
            assert!(HttpBasePathV1::new(invalid).is_err(), "{invalid}");
        }

        let without_slash = HttpBasePathV1::new("/v1/items").expect("base path");
        let with_slash = HttpBasePathV1::new("/v1/items/").expect("base path");
        assert!(!without_slash.has_explicit_trailing_slash());
        assert!(with_slash.has_explicit_trailing_slash());
        assert_ne!(without_slash.as_str(), with_slash.as_str());
        #[derive(serde::Serialize)]
        struct RequestHashMaterial<'a> {
            canonical_path: &'a HttpBasePathV1,
        }
        #[derive(serde::Serialize)]
        struct GrantManifestHashMaterial<'a> {
            canonical_path: &'a HttpBasePathV1,
        }
        assert_ne!(
            canonical_sha256(&RequestHashMaterial { canonical_path: &without_slash })
                .expect("request hash without slash"),
            canonical_sha256(&RequestHashMaterial { canonical_path: &with_slash })
                .expect("request hash with slash")
        );
        assert_ne!(
            canonical_sha256(&GrantManifestHashMaterial { canonical_path: &without_slash })
                .expect("grant hash without slash"),
            canonical_sha256(&GrantManifestHashMaterial { canonical_path: &with_slash })
                .expect("grant hash with slash")
        );
    }

    #[test]
    fn logical_http_path_segments_are_encoded_exactly_once() {
        for invalid in [".", "..", "a/b", "a\\b", "a\nb"] {
            assert!(HttpPathLiteralSegmentV1::new(invalid).is_err(), "{invalid}");
            assert!(HttpPathArgumentSegmentV1::new(invalid).is_err(), "{invalid}");
        }
        let percent = HttpPathLiteralSegmentV1::new("group%2Fproject").expect("logical percent");
        assert_eq!(percent.percent_encoded(), "group%252Fproject");
        let unicode = HttpPathArgumentSegmentV1::new("项目 name").expect("logical UTF-8");
        assert_eq!(unicode.percent_encoded(), "%E9%A1%B9%E7%9B%AE%20name");
    }

    #[test]
    fn environment_and_header_bindings_are_unique_and_credentials_do_not_collide() {
        let environment = vec![
            serde_json::from_value::<CommandEnvironmentEntryV1>(json!({
                "name": "AURA_MODE",
                "value": { "type": "literal", "value": "one" }
            }))
            .expect("environment"),
            serde_json::from_value::<CommandEnvironmentEntryV1>(json!({
                "name": "AURA_MODE",
                "value": { "type": "literal", "value": "two" }
            }))
            .expect("duplicate environment"),
        ];
        assert_eq!(
            validate_command_environment_v1(&environment, None),
            Err(ToolRuntimeContractError::DuplicateEnvironmentName)
        );
        let credential = CommandCredentialEnvironmentSlotV1 {
            slot_key: CredentialSlotKey::new("api_key").expect("slot"),
            environment_variable: EnvironmentVariableNameV1::new("AURA_MODE")
                .expect("environment name"),
        };
        assert_eq!(
            validate_command_environment_v1(&environment[..1], Some(&credential)),
            Err(ToolRuntimeContractError::CredentialEnvironmentConflict)
        );

        let headers = vec![
            serde_json::from_value::<HttpHeaderEntryV1>(json!({
                "name": "X-Aura-Mode",
                "value": { "type": "literal", "value": "one" }
            }))
            .expect("header"),
            serde_json::from_value::<HttpHeaderEntryV1>(json!({
                "name": "x-aura-mode",
                "value": { "type": "literal", "value": "two" }
            }))
            .expect("duplicate header"),
        ];
        assert_eq!(
            validate_http_header_bindings_v1(&headers, None),
            Err(ToolRuntimeContractError::DuplicateHttpHeader)
        );
        let credential = HttpCredentialSlotV1::FixedHeader {
            slot_key: CredentialSlotKey::new("api_key").expect("slot"),
            header_name: HttpHeaderNameV1::new("X-Aura-Mode").expect("header name"),
        };
        assert_eq!(
            validate_http_header_bindings_v1(&headers[..1], Some(&credential)),
            Err(ToolRuntimeContractError::CredentialHttpHeaderConflict)
        );
    }

    #[test]
    fn http_json_properties_require_jcs_order_and_unique_names() {
        let ordered = serde_json::from_value::<HttpJsonPropertiesV1>(json!([
            { "name": "a", "value": { "type": "literal", "value": 1 } },
            { "name": "b", "value": { "type": "literal", "value": 2 } }
        ]));
        assert!(ordered.is_ok());
        for invalid in [
            json!([
                { "name": "b", "value": { "type": "literal", "value": 1 } },
                { "name": "a", "value": { "type": "literal", "value": 2 } }
            ]),
            json!([
                { "name": "a", "value": { "type": "literal", "value": 1 } },
                { "name": "a", "value": { "type": "literal", "value": 2 } }
            ]),
        ] {
            assert!(serde_json::from_value::<HttpJsonPropertiesV1>(invalid).is_err());
        }
    }

    #[test]
    fn headers_reject_repeated_values_while_queries_accept_them() {
        let repeated = json!({
            "name": "tags",
            "value": {
                "type": "repeated",
                "source": "/tags"
            }
        });
        serde_json::from_value::<HttpQueryEntryV1>(repeated.clone()).expect("query repeated value");
        assert!(serde_json::from_value::<HttpHeaderEntryV1>(repeated).is_err());
    }

    #[test]
    fn stdio_frame_uses_the_flat_tagged_payload_envelope() {
        let tool_call_id = ToolCallId::from_bytes([1; 16]);
        let attempt_id = ToolExecutionAttemptId::from_bytes([2; 16]);
        let frame = StdioProtocolFrameV1::Hello {
            protocol_version: STDIO_PROTOCOL_VERSION_V1,
            tool_call_id,
            tool_execution_attempt_id: attempt_id,
            sequence: 1,
            payload: StdioHelloPayloadV1 {
                runtime_name: "fixture-runtime".to_owned(),
                runtime_version: "1.0.0".to_owned(),
            },
        };
        frame.validate().expect("valid frame");
        assert_eq!(frame.direction(), StdioProtocolDirectionV1::RuntimeToCore);
        assert_eq!(frame.tool_call_id(), tool_call_id);
        assert_eq!(frame.tool_execution_attempt_id(), attempt_id);
        assert_eq!(
            serde_json::to_value(&frame).expect("frame wire"),
            json!({
                "message_type": "hello",
                "protocol_version": 1,
                "tool_call_id": tool_call_id,
                "tool_execution_attempt_id": attempt_id,
                "sequence": 1,
                "payload": {
                    "runtime_name": "fixture-runtime",
                    "runtime_version": "1.0.0"
                }
            })
        );
    }

    #[test]
    fn stdio_frame_rejects_unknown_fields_and_invalid_common_values() {
        let tool_call_id = ToolCallId::from_bytes([3; 16]);
        let attempt_id = ToolExecutionAttemptId::from_bytes([4; 16]);
        let unknown = json!({
            "message_type": "cancel",
            "protocol_version": 1,
            "tool_call_id": tool_call_id,
            "tool_execution_attempt_id": attempt_id,
            "sequence": 1,
            "payload": {},
            "unexpected": true
        });
        assert!(serde_json::from_value::<StdioProtocolFrameV1>(unknown).is_err());

        let invalid = StdioProtocolFrameV1::Execute {
            protocol_version: 2,
            tool_call_id,
            tool_execution_attempt_id: attempt_id,
            sequence: 0,
            payload: super::StdioExecutePayloadV1 {
                arguments: json!({}),
                arguments_hash: ContentHash::sha256(b"{}"),
            },
        };
        assert_eq!(invalid.validate(), Err(ToolRuntimeContractError::InvalidStdioProtocolVersion));
    }
}
